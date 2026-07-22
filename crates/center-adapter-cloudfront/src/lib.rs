//! Read-only Amazon CloudFront distribution inventory adapter.

use std::{collections::BTreeSet, sync::Arc, time::Duration};

use edgion_center_core::{
    CloudProvider, CloudResourceId, ProviderAccountScope, ProviderAccountSpec,
};

mod api;
mod aws_sdk;
mod model;
// CLD-28F/29A consume this private safety seam when guarded writes are added.
#[allow(dead_code)]
mod wire_fidelity;

pub use api::*;
pub use aws_sdk::*;
pub use model::*;

const DISTRIBUTION_PAGE_SIZE: u16 = 100;
const MAX_PROVIDER_PAGES: usize = 10_000;
const MAX_DISTRIBUTIONS: usize = 100_000;
const MAX_FRESHNESS_WINDOW_MS: i64 = 5 * 60 * 1_000;
const INVENTORY_TIMEOUT: Duration = Duration::from_secs(120);

/// Account- and credential-bound read-only CloudFront inventory adapter.
pub struct CloudFrontInventoryAdapter {
    provider_account_id: CloudResourceId,
    aws_account_id: String,
    partition: AwsPartition,
    account_generation: u64,
    credential_revision: String,
    api: Arc<dyn CloudFrontApi>,
}

impl CloudFrontInventoryAdapter {
    pub fn new(
        provider_account_id: CloudResourceId,
        account_generation: u64,
        account: &ProviderAccountSpec,
        api: Arc<dyn CloudFrontApi>,
    ) -> CloudFrontApiResult<Self> {
        provider_account_id
            .validate()
            .map_err(|_| validation("invalid_cloudfront_provider_account_id"))?;
        account
            .validate()
            .map_err(|_| validation("invalid_cloudfront_provider_account"))?;
        if account.provider != CloudProvider::Aws {
            return Err(validation("cloudfront_aws_provider_required"));
        }
        let ProviderAccountScope::Aws { account_id } = account
            .scope
            .as_ref()
            .ok_or_else(|| validation("cloudfront_aws_account_scope_required"))?
        else {
            return Err(validation("cloudfront_aws_account_scope_mismatch"));
        };
        if api.verified_account_id() != account_id {
            return Err(validation("cloudfront_verified_account_mismatch"));
        }
        let credential_revision = api.credential_revision().to_string();
        // Reuse the authority validator so constructor and observation enforce one contract.
        CloudFrontObservationAuthority::new(
            provider_account_id.clone(),
            account_id.clone(),
            api.verified_partition(),
            account_generation,
            credential_revision.clone(),
            "constructor-validation".to_string(),
            1,
            2,
        )?;
        Ok(Self {
            provider_account_id,
            aws_account_id: account_id.clone(),
            partition: api.verified_partition(),
            account_generation,
            credential_revision,
            api,
        })
    }

    pub async fn inventory(
        &self,
        observation_token: impl Into<String>,
        observed_at_unix_ms: i64,
        valid_until_unix_ms: i64,
    ) -> CloudFrontApiResult<CloudFrontInventory> {
        if valid_until_unix_ms.saturating_sub(observed_at_unix_ms) > MAX_FRESHNESS_WINDOW_MS {
            return Err(validation("cloudfront_observation_freshness_limit"));
        }
        tokio::time::timeout(
            INVENTORY_TIMEOUT,
            self.inventory_within_deadline(
                observation_token.into(),
                observed_at_unix_ms,
                valid_until_unix_ms,
            ),
        )
        .await
        .map_err(|_| validation("cloudfront_inventory_deadline_exceeded"))?
    }

    async fn inventory_within_deadline(
        &self,
        observation_token: String,
        observed_at_unix_ms: i64,
        valid_until_unix_ms: i64,
    ) -> CloudFrontApiResult<CloudFrontInventory> {
        let authority = CloudFrontObservationAuthority::new(
            self.provider_account_id.clone(),
            self.aws_account_id.clone(),
            self.partition,
            self.account_generation,
            self.credential_revision.clone(),
            observation_token,
            observed_at_unix_ms,
            valid_until_unix_ms,
        )?;
        authority.validate()?;

        let summaries = self.list_all_summaries().await?;
        let mut entries = Vec::with_capacity(summaries.len());
        for summary in summaries {
            entries.push(self.observe_entry(summary).await);
        }
        entries.sort_by(|left, right| left.summary.id.cmp(&right.summary.id));
        Ok(CloudFrontInventory { authority, entries })
    }

    async fn list_all_summaries(&self) -> CloudFrontApiResult<Vec<CloudFrontDistributionSummary>> {
        let mut summaries = Vec::new();
        let mut marker = None::<String>;
        let mut seen_markers = BTreeSet::new();
        let mut seen_ids = BTreeSet::new();
        for _ in 0..MAX_PROVIDER_PAGES {
            let page = self
                .api
                .list_distributions(marker.as_deref(), DISTRIBUTION_PAGE_SIZE)
                .await?;
            if page.is_truncated != page.next_marker.is_some() {
                return Err(validation("inconsistent_cloudfront_distribution_page"));
            }
            for summary in page.items {
                validate_summary(&summary, self.partition, &self.aws_account_id)?;
                if !seen_ids.insert(summary.id.clone()) {
                    return Err(validation("duplicate_cloudfront_distribution_id"));
                }
                summaries.push(summary);
                if summaries.len() > MAX_DISTRIBUTIONS {
                    return Err(validation("cloudfront_distribution_inventory_limit"));
                }
            }
            let Some(next_marker) = page.next_marker else {
                return Ok(summaries);
            };
            validate_provider_cursor(&next_marker)?;
            if marker.as_deref() == Some(next_marker.as_str())
                || !seen_markers.insert(next_marker.clone())
            {
                return Err(validation("cloudfront_distribution_pagination_loop"));
            }
            marker = Some(next_marker);
        }
        Err(validation("cloudfront_distribution_pagination_limit"))
    }

    async fn observe_entry(
        &self,
        summary: CloudFrontDistributionSummary,
    ) -> CloudFrontInventoryEntry {
        let observed = self.api.get_distribution(&summary.id).await;
        let detail = match observed {
            Ok(Some(detail)) => match self.complete_detail(&summary, detail) {
                Ok(detail) => detail,
                Err(error) => detail_issue(CloudFrontDetailIssueKind::Malformed, error.code()),
            },
            Ok(None) => detail_issue(
                CloudFrontDetailIssueKind::Missing,
                "cloudfront_distribution_detail_missing",
            ),
            Err(error) => detail_issue(classify_detail_error(&error), error.code()),
        };

        let ownership_hint = if matches!(detail, CloudFrontDetailObservation::Complete(_)) {
            self.observe_ownership(&summary.arn).await
        } else {
            CloudFrontOwnershipHint::Unknown {
                code: "cloudfront_detail_incomplete".to_string(),
            }
        };
        CloudFrontInventoryEntry {
            summary,
            detail,
            ownership_hint,
        }
    }

    fn complete_detail(
        &self,
        listed: &CloudFrontDistributionSummary,
        detail: CloudFrontDistributionDetail,
    ) -> CloudFrontApiResult<CloudFrontDetailObservation> {
        validate_detail(&detail, self.partition, &self.aws_account_id)?;
        if detail.summary.id != listed.id
            || detail.summary.arn != listed.arn
            || detail.summary.domain_name != listed.domain_name
        {
            return Err(validation("cloudfront_summary_detail_identity_mismatch"));
        }
        let _changed_since_summary = detail.summary.status != listed.status
            || detail.summary.enabled != listed.enabled
            || detail.summary.last_modified_unix_seconds != listed.last_modified_unix_seconds;
        Ok(CloudFrontDetailObservation::Complete(Box::new(detail)))
    }

    async fn observe_ownership(&self, arn: &str) -> CloudFrontOwnershipHint {
        match self.api.list_tags_for_resource(arn).await {
            Ok(tags) => {
                if tags.keys.iter().any(|key| !is_sanitized_tag_key(key)) {
                    return CloudFrontOwnershipHint::Unknown {
                        code: "invalid_cloudfront_tag_key".to_string(),
                    };
                }
                match tags.center_resource_id {
                    Some(value) => match CloudResourceId::new(value) {
                        Ok(center_resource_id) => {
                            CloudFrontOwnershipHint::Present { center_resource_id }
                        }
                        Err(_) => CloudFrontOwnershipHint::Unknown {
                            code: "invalid_cloudfront_ownership_tag".to_string(),
                        },
                    },
                    None => CloudFrontOwnershipHint::Absent,
                }
            }
            Err(error) => CloudFrontOwnershipHint::Unknown {
                code: error.code().to_string(),
            },
        }
    }
}

fn validate_provider_cursor(value: &str) -> CloudFrontApiResult<()> {
    if value.is_empty()
        || value.len() > 1024
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(validation("invalid_cloudfront_distribution_marker"));
    }
    Ok(())
}

fn is_sanitized_tag_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.trim() == value
        && !value.chars().any(char::is_control)
}
