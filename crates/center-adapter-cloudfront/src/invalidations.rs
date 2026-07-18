//! Observation-bound CloudFront invalidation planning without provider dispatch authority.

use std::collections::BTreeSet;

use edgion_center_core::OperationId;
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    model::validation, CloudFrontApi, CloudFrontApiResult,
    CloudFrontDistributionObservationBinding, CloudFrontInvalidationDetail,
    CloudFrontInvalidationStatus, CloudFrontInventoryAdapter, CloudFrontPlanningInventory,
};

const MAX_INVALIDATION_PATH_LEN: usize = 4_000;
const MAX_INVALIDATION_PATHS_PER_PLAN: usize = 1_000;
const INVALIDATION_PAGE_SIZE: u16 = 100;
const MAX_INVALIDATION_PAGES: usize = 10_000;
const MAX_INVALIDATION_OBSERVATIONS: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct CloudFrontInvalidationPath(String);

impl CloudFrontInvalidationPath {
    pub fn new(value: impl Into<String>) -> CloudFrontApiResult<Self> {
        let value = canonicalize_path(&value.into())?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn is_wildcard(&self) -> bool {
        self.0.ends_with('*')
    }

    fn is_all_paths(&self) -> bool {
        self.0 == "/*"
    }
}

fn canonicalize_path(value: &str) -> CloudFrontApiResult<String> {
    if value.is_empty()
        || value.len() > MAX_INVALIDATION_PATH_LEN
        || !value.starts_with('/')
        || !value.is_ascii()
        || value.contains('?')
        || value.contains('#')
        || value.chars().any(char::is_control)
        || value.bytes().any(|byte| byte == b' ' || byte == b'\\')
    {
        return Err(validation("invalid_cloudfront_invalidation_path"));
    }
    let wildcard_count = value.bytes().filter(|byte| *byte == b'*').count();
    if wildcard_count > 1 || (wildcard_count == 1 && !value.ends_with('*')) {
        return Err(validation("unsupported_cloudfront_invalidation_wildcard"));
    }
    let bytes = value.as_bytes();
    let mut output = String::with_capacity(value.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            if !matches!(bytes[index], 0x21..=0x7e) {
                return Err(validation("invalid_cloudfront_invalidation_path"));
            }
            if bytes[index] == b'~' {
                return Err(validation("unsupported_cloudfront_invalidation_tilde"));
            }
            if is_rfc1738_unsafe(bytes[index]) {
                return Err(validation("invalid_cloudfront_invalidation_path"));
            }
            output.push(char::from(bytes[index]));
            index += 1;
            continue;
        }
        let mut decoded_run = Vec::new();
        while index < bytes.len() && bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return Err(validation(
                    "invalid_cloudfront_invalidation_percent_encoding",
                ));
            }
            let decoded = (hex(bytes[index + 1])? << 4) | hex(bytes[index + 2])?;
            if decoded.is_ascii() && !is_rfc1738_unsafe(decoded) {
                return Err(validation(
                    "unnecessary_cloudfront_invalidation_percent_encoding",
                ));
            }
            decoded_run.push(decoded);
            output.push('%');
            output.push(char::from(bytes[index + 1].to_ascii_uppercase()));
            output.push(char::from(bytes[index + 2].to_ascii_uppercase()));
            index += 3;
        }
        if decoded_run.iter().any(|byte| !byte.is_ascii())
            && std::str::from_utf8(&decoded_run).is_err()
        {
            return Err(validation(
                "invalid_cloudfront_invalidation_percent_encoding",
            ));
        }
    }
    Ok(output)
}

fn is_rfc1738_unsafe(value: u8) -> bool {
    matches!(
        value,
        b' ' | b'"'
            | b'#'
            | b'%'
            | b'<'
            | b'>'
            | b'['
            | b'\\'
            | b']'
            | b'^'
            | b'`'
            | b'{'
            | b'|'
            | b'}'
    )
}

fn hex(value: u8) -> CloudFrontApiResult<u8> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(validation(
            "invalid_cloudfront_invalidation_percent_encoding",
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudFrontInvalidationImpact {
    Targeted,
    Wildcard,
    AllPaths,
}

#[derive(Debug, Clone)]
pub struct CloudFrontInvalidationPlanRequest {
    pub distribution_id: String,
    pub operation_id: OperationId,
    pub intent: CloudFrontInvalidationIntent,
    pub now_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudFrontInvalidationIntent {
    Paths(Vec<String>),
    AllPaths,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontInvalidationPlan {
    binding: CloudFrontDistributionObservationBinding,
    operation_id: OperationId,
    request_digest: String,
    caller_reference: String,
    paths: Vec<CloudFrontInvalidationPath>,
    impact: CloudFrontInvalidationImpact,
    provider_cost_may_apply: bool,
    query_variant_coverage_unproven: bool,
    broad_impact_approval_required: bool,
    dispatch_blockers: BTreeSet<String>,
    dispatch_authorized: bool,
}

impl CloudFrontInvalidationPlan {
    pub fn request_digest(&self) -> &str {
        &self.request_digest
    }

    pub fn caller_reference(&self) -> &str {
        &self.caller_reference
    }

    pub fn paths(&self) -> &[CloudFrontInvalidationPath] {
        &self.paths
    }

    pub fn impact(&self) -> CloudFrontInvalidationImpact {
        self.impact
    }
}

/// A complete, exact-read invalidation observation for one distribution and read window.
pub struct CloudFrontInvalidationPlanningInventory {
    binding: CloudFrontDistributionObservationBinding,
    items: Vec<CloudFrontInvalidationDetail>,
}

impl CloudFrontInvalidationPlanningInventory {
    pub fn items(&self) -> &[CloudFrontInvalidationDetail] {
        &self.items
    }

    pub fn active_non_wildcard_item_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.status == CloudFrontInvalidationStatus::InProgress)
            .flat_map(|item| &item.paths)
            .filter(|path| !path.ends_with('*'))
            .count()
    }

    pub fn active_wildcard_path_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.status == CloudFrontInvalidationStatus::InProgress)
            .flat_map(|item| &item.paths)
            .filter(|path| path.ends_with('*'))
            .count()
    }
}

impl CloudFrontInventoryAdapter {
    pub async fn planning_invalidation_inventory(
        &self,
        inventory: &CloudFrontPlanningInventory,
        distribution_id: &str,
        now_unix_ms: i64,
    ) -> CloudFrontApiResult<CloudFrontInvalidationPlanningInventory> {
        let binding = CloudFrontDistributionObservationBinding::from_inventory(
            inventory,
            distribution_id,
            now_unix_ms,
        )?;
        let authority = binding.authority();
        if authority.provider_account_id() != &self.provider_account_id
            || authority.aws_account_id() != self.aws_account_id
            || authority.partition() != self.partition
            || authority.account_generation() != self.account_generation
            || authority.credential_revision() != self.credential_revision
        {
            return Err(validation(
                "cloudfront_invalidation_adapter_authority_mismatch",
            ));
        }
        tokio::time::timeout(
            crate::INVENTORY_TIMEOUT,
            observe_invalidations(self.api.as_ref(), binding, distribution_id),
        )
        .await
        .map_err(|_| validation("cloudfront_invalidation_inventory_deadline_exceeded"))?
    }
}

async fn observe_invalidations(
    api: &dyn CloudFrontApi,
    binding: CloudFrontDistributionObservationBinding,
    distribution_id: &str,
) -> CloudFrontApiResult<CloudFrontInvalidationPlanningInventory> {
    let mut marker = None::<String>;
    let mut seen_markers = BTreeSet::new();
    let mut seen_ids = BTreeSet::new();
    let mut items = Vec::new();
    for _ in 0..MAX_INVALIDATION_PAGES {
        let page = api
            .list_invalidations(distribution_id, marker.as_deref(), INVALIDATION_PAGE_SIZE)
            .await?;
        if page.distribution_id != distribution_id
            || page.is_truncated != page.next_marker.is_some()
        {
            return Err(validation("cloudfront_invalidation_page_scope_mismatch"));
        }
        for summary in page.items {
            if summary.distribution_id != distribution_id || !seen_ids.insert(summary.id.clone()) {
                return Err(validation("duplicate_cloudfront_invalidation_observation"));
            }
            let detail = api
                .get_invalidation(distribution_id, &summary.id)
                .await?
                .ok_or_else(|| validation("cloudfront_invalidation_observation_changed"))?;
            if detail.distribution_id != summary.distribution_id
                || detail.id != summary.id
                || detail.created_at_unix_seconds != summary.created_at_unix_seconds
                || (summary.status == CloudFrontInvalidationStatus::Completed
                    && detail.status != CloudFrontInvalidationStatus::Completed)
            {
                return Err(validation("cloudfront_invalidation_observation_changed"));
            }
            validate_observed_invalidation(&detail)?;
            items.push(detail);
            if items.len() > MAX_INVALIDATION_OBSERVATIONS {
                return Err(validation("cloudfront_invalidation_inventory_limit"));
            }
        }
        let Some(next) = page.next_marker else {
            return Ok(CloudFrontInvalidationPlanningInventory { binding, items });
        };
        if marker.as_deref() == Some(next.as_str()) || !seen_markers.insert(next.clone()) {
            return Err(validation("cloudfront_invalidation_pagination_loop"));
        }
        marker = Some(next);
    }
    Err(validation("cloudfront_invalidation_pagination_limit"))
}

fn validate_observed_invalidation(
    detail: &CloudFrontInvalidationDetail,
) -> CloudFrontApiResult<()> {
    if detail.caller_reference.is_empty()
        || detail.caller_reference.len() > 4096
        || detail.caller_reference.chars().any(char::is_control)
        || detail.paths.is_empty()
    {
        return Err(validation("invalid_cloudfront_invalidation_observation"));
    }
    if detail.paths.iter().any(|item| {
        item.is_empty()
            || item.len() > 8192
            || item.trim() != item
            || item.chars().any(char::is_control)
    }) {
        return Err(validation("invalid_cloudfront_invalidation_item"));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum CloudFrontInvalidationReconciliation {
    NotObservedInNonSnapshotScan,
    Observed {
        provider_invalidation_id: String,
        status: CloudFrontInvalidationStatus,
    },
}

pub fn reconcile_invalidation_plan(
    plan: &CloudFrontInvalidationPlan,
    inventory: &CloudFrontInvalidationPlanningInventory,
    now_unix_ms: i64,
) -> CloudFrontApiResult<CloudFrontInvalidationReconciliation> {
    inventory.binding.validate_at(now_unix_ms)?;
    if plan.binding.provider_account_id() != inventory.binding.provider_account_id()
        || plan.binding.aws_account_id() != inventory.binding.aws_account_id()
        || plan.binding.partition() != inventory.binding.partition()
        || plan.binding.account_generation() != inventory.binding.account_generation()
        || plan.binding.distribution_id() != inventory.binding.distribution_id()
    {
        return Err(validation(
            "cloudfront_invalidation_reconciliation_scope_mismatch",
        ));
    }
    let matches = inventory
        .items
        .iter()
        .filter(|item| item.caller_reference == plan.caller_reference)
        .collect::<Vec<_>>();
    if matches.is_empty() {
        return Ok(CloudFrontInvalidationReconciliation::NotObservedInNonSnapshotScan);
    }
    if matches.len() != 1 {
        return Err(validation(
            "duplicate_cloudfront_invalidation_caller_reference",
        ));
    }
    let observed = matches[0];
    let observed_paths = observed
        .paths
        .iter()
        .map(CloudFrontInvalidationPath::new)
        .collect::<CloudFrontApiResult<BTreeSet<_>>>()?
        .into_iter()
        .collect::<Vec<_>>();
    if observed_paths.len() != observed.paths.len() || observed_paths != plan.paths {
        return Err(validation("cloudfront_invalidation_identity_conflict"));
    }
    Ok(CloudFrontInvalidationReconciliation::Observed {
        provider_invalidation_id: observed.id.clone(),
        status: observed.status,
    })
}

pub fn build_invalidation_plan(
    request: CloudFrontInvalidationPlanRequest,
    inventory: &CloudFrontPlanningInventory,
) -> CloudFrontApiResult<CloudFrontInvalidationPlan> {
    request
        .operation_id
        .validate()
        .map_err(|_| validation("invalid_cloudfront_invalidation_operation_id"))?;
    let binding = CloudFrontDistributionObservationBinding::from_inventory(
        inventory,
        &request.distribution_id,
        request.now_unix_ms,
    )?;
    let paths = match request.intent {
        CloudFrontInvalidationIntent::AllPaths => {
            vec![CloudFrontInvalidationPath::new("/*")?]
        }
        CloudFrontInvalidationIntent::Paths(paths) => {
            if paths.is_empty() || paths.len() > MAX_INVALIDATION_PATHS_PER_PLAN {
                return Err(validation("cloudfront_invalidation_path_count_limit"));
            }
            let paths = paths
                .into_iter()
                .map(CloudFrontInvalidationPath::new)
                .collect::<CloudFrontApiResult<BTreeSet<_>>>()?
                .into_iter()
                .collect::<Vec<_>>();
            if paths.iter().any(CloudFrontInvalidationPath::is_all_paths) {
                return Err(validation("cloudfront_all_paths_intent_required"));
            }
            paths
        }
    };
    let impact = if paths[0].is_all_paths() {
        CloudFrontInvalidationImpact::AllPaths
    } else if paths.iter().any(CloudFrontInvalidationPath::is_wildcard) {
        CloudFrontInvalidationImpact::Wildcard
    } else {
        CloudFrontInvalidationImpact::Targeted
    };
    let request_digest = invalidation_request_digest(&binding, &paths)?;
    let caller_reference = format!(
        "edgion-invalidation-{:x}",
        Sha256::digest(
            format!(
                "{}\0{}\0{}",
                binding.distribution_id(),
                request.operation_id.as_str(),
                request_digest
            )
            .as_bytes()
        )
    );
    let broad_impact_approval_required = impact != CloudFrontInvalidationImpact::Targeted;
    let query_variant_coverage_unproven = impact == CloudFrontInvalidationImpact::Targeted;
    let mut dispatch_blockers = BTreeSet::from([
        "cloudfront_distribution_ownership_proof_missing".to_string(),
        "cloudfront_invalidation_executor_missing".to_string(),
        "cloudfront_invalidation_quota_evidence_missing".to_string(),
        "cloudfront_operation_identity_binding_missing".to_string(),
    ]);
    if broad_impact_approval_required {
        dispatch_blockers.insert("cloudfront_broad_impact_approval_missing".to_string());
    }
    if query_variant_coverage_unproven {
        dispatch_blockers.insert("cloudfront_query_variant_coverage_unproven".to_string());
    }
    Ok(CloudFrontInvalidationPlan {
        binding,
        operation_id: request.operation_id,
        request_digest,
        caller_reference,
        paths,
        impact,
        provider_cost_may_apply: true,
        query_variant_coverage_unproven,
        broad_impact_approval_required,
        dispatch_blockers,
        dispatch_authorized: false,
    })
}

fn invalidation_request_digest(
    binding: &CloudFrontDistributionObservationBinding,
    paths: &[CloudFrontInvalidationPath],
) -> CloudFrontApiResult<String> {
    let canonical = serde_json::to_vec(&(
        binding.provider_account_id(),
        binding.aws_account_id(),
        binding.partition(),
        binding.distribution_id(),
        paths,
    ))
    .map_err(|_| validation("cloudfront_invalidation_digest_failed"))?;
    Ok(format!("sha256:{:x}", Sha256::digest(canonical)))
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use super::*;
    use crate::tests::{adapter, detail, summary, FakeApi, ACCOUNT_ID};
    use crate::{
        AwsPartition, CloudFrontDistributionDetail, CloudFrontDistributionPage,
        CloudFrontInvalidationPage, CloudFrontInvalidationSummary, CloudFrontPolicyKind,
        CloudFrontPolicyPage, CloudFrontPolicyScope, CloudFrontTags,
    };
    use async_trait::async_trait;
    use edgion_center_core::{
        CloudProvider, CloudResourceId, CredentialSource, ProviderAccountScope, ProviderAccountSpec,
    };

    struct InvalidationFakeApi {
        base: FakeApi,
        invalidation_pages: BTreeMap<Option<String>, CloudFrontInvalidationPage>,
        invalidation_details: BTreeMap<String, CloudFrontInvalidationDetail>,
    }

    #[async_trait]
    impl CloudFrontApi for InvalidationFakeApi {
        fn verified_account_id(&self) -> &str {
            self.base.verified_account_id()
        }

        fn verified_partition(&self) -> AwsPartition {
            self.base.verified_partition()
        }

        fn credential_revision(&self) -> &str {
            self.base.credential_revision()
        }

        async fn list_distributions(
            &self,
            marker: Option<&str>,
            max_items: u16,
        ) -> CloudFrontApiResult<CloudFrontDistributionPage> {
            self.base.list_distributions(marker, max_items).await
        }

        async fn get_distribution(
            &self,
            distribution_id: &str,
        ) -> CloudFrontApiResult<Option<CloudFrontDistributionDetail>> {
            self.base.get_distribution(distribution_id).await
        }

        async fn list_policies(
            &self,
            kind: CloudFrontPolicyKind,
            scope: CloudFrontPolicyScope,
            marker: Option<&str>,
            max_items: u16,
        ) -> CloudFrontApiResult<CloudFrontPolicyPage> {
            self.base
                .list_policies(kind, scope, marker, max_items)
                .await
        }

        async fn list_invalidations(
            &self,
            _distribution_id: &str,
            marker: Option<&str>,
            _max_items: u16,
        ) -> CloudFrontApiResult<CloudFrontInvalidationPage> {
            self.invalidation_pages
                .get(&marker.map(ToString::to_string))
                .cloned()
                .ok_or_else(|| validation("missing_fake_invalidation_page"))
        }

        async fn get_invalidation(
            &self,
            _distribution_id: &str,
            invalidation_id: &str,
        ) -> CloudFrontApiResult<Option<CloudFrontInvalidationDetail>> {
            Ok(self.invalidation_details.get(invalidation_id).cloned())
        }

        async fn list_tags_for_resource(&self, arn: &str) -> CloudFrontApiResult<CloudFrontTags> {
            self.base.list_tags_for_resource(arn).await
        }
    }

    fn inventory() -> CloudFrontPlanningInventory {
        let summary = summary();
        let api = Arc::new(FakeApi {
            account_id: ACCOUNT_ID.to_string(),
            partition: AwsPartition::Aws,
            pages: vec![CloudFrontDistributionPage {
                items: vec![summary.clone()],
                is_truncated: false,
                next_marker: None,
            }],
            detail: Some(detail(summary)),
            tags: CloudFrontTags::default(),
        });
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            adapter(api)
                .planning_inventory("invalidation-observation", 1_000, 2_000)
                .await
                .unwrap()
        })
    }

    #[test]
    fn targeted_plan_is_canonical_stable_and_never_dispatchable() {
        let inventory = inventory();
        let build = |paths: Vec<&str>| {
            build_invalidation_plan(
                CloudFrontInvalidationPlanRequest {
                    distribution_id: "E123EXAMPLE".to_string(),
                    operation_id: OperationId::new("invalidate-release-42").unwrap(),
                    intent: CloudFrontInvalidationIntent::Paths(
                        paths.into_iter().map(ToString::to_string).collect(),
                    ),
                    now_unix_ms: 1_500,
                },
                &inventory,
            )
            .unwrap()
        };
        let left = build(vec!["/assets/app.js", "/index.html", "/assets/app.js"]);
        let right = build(vec!["/index.html", "/assets/app.js"]);
        assert_eq!(left.paths, right.paths);
        assert_eq!(left.request_digest, right.request_digest);
        assert_eq!(left.caller_reference, right.caller_reference);
        let changed = build(vec!["/different.html"]);
        assert_ne!(left.request_digest, changed.request_digest);
        assert_ne!(left.caller_reference, changed.caller_reference);
        assert_eq!(left.impact, CloudFrontInvalidationImpact::Targeted);
        assert!(!left.broad_impact_approval_required);
        assert!(left.query_variant_coverage_unproven);
        assert!(left
            .dispatch_blockers
            .contains("cloudfront_query_variant_coverage_unproven"));
        assert!(!left.dispatch_authorized);

        let observed = CloudFrontInvalidationPlanningInventory {
            binding: left.binding.clone(),
            items: vec![CloudFrontInvalidationDetail {
                distribution_id: "E123EXAMPLE".to_string(),
                id: "I123EXAMPLE".to_string(),
                status: CloudFrontInvalidationStatus::InProgress,
                created_at_unix_seconds: 1,
                caller_reference: left.caller_reference.clone(),
                paths: left
                    .paths
                    .iter()
                    .map(|path| path.as_str().to_string())
                    .collect(),
            }],
        };
        assert!(matches!(
            reconcile_invalidation_plan(&left, &observed, 1_500).unwrap(),
            CloudFrontInvalidationReconciliation::Observed {
                status: CloudFrontInvalidationStatus::InProgress,
                ..
            }
        ));
        let mut conflicting = observed;
        conflicting.items[0].paths = vec!["/different.html".to_string()];
        assert_eq!(
            reconcile_invalidation_plan(&left, &conflicting, 1_500)
                .unwrap_err()
                .code(),
            "cloudfront_invalidation_identity_conflict"
        );
    }

    #[test]
    fn wildcard_and_all_paths_are_explicit_high_impact_shapes() {
        let inventory = inventory();
        for (intent, impact) in [
            (
                CloudFrontInvalidationIntent::Paths(vec!["/assets/*".to_string()]),
                CloudFrontInvalidationImpact::Wildcard,
            ),
            (
                CloudFrontInvalidationIntent::AllPaths,
                CloudFrontInvalidationImpact::AllPaths,
            ),
        ] {
            let plan = build_invalidation_plan(
                CloudFrontInvalidationPlanRequest {
                    distribution_id: "E123EXAMPLE".to_string(),
                    operation_id: OperationId::new("invalidate-broad").unwrap(),
                    intent,
                    now_unix_ms: 1_500,
                },
                &inventory,
            )
            .unwrap();
            assert_eq!(plan.impact, impact);
            assert!(plan.broad_impact_approval_required);
            assert!(plan
                .dispatch_blockers
                .contains("cloudfront_broad_impact_approval_missing"));
        }
        assert_eq!(
            build_invalidation_plan(
                CloudFrontInvalidationPlanRequest {
                    distribution_id: "E123EXAMPLE".to_string(),
                    operation_id: OperationId::new("invalidate-mixed").unwrap(),
                    intent: CloudFrontInvalidationIntent::Paths(vec!["/*".to_string()]),
                    now_unix_ms: 1_500,
                },
                &inventory,
            )
            .unwrap_err()
            .code(),
            "cloudfront_all_paths_intent_required"
        );
    }

    #[test]
    fn paths_are_conservative_and_evidence_expires() {
        for invalid in [
            "index.html",
            "/a*b",
            "/a?x=1",
            "/bad%2",
            "/bad path",
            "/bad~path",
            "/bad%7Epath",
            "/safe%2Etxt",
            "/bad%00",
            "/bad%FF",
            "/bad\"name",
            "/bad<name",
            "/bad|name",
        ] {
            assert!(
                CloudFrontInvalidationPath::new(invalid).is_err(),
                "{invalid}"
            );
        }
        assert_eq!(
            CloudFrontInvalidationPath::new("/asset%20item/%e4%b8%ad")
                .unwrap()
                .as_str(),
            "/asset%20item/%E4%B8%AD"
        );
        let inventory = inventory();
        assert_eq!(
            build_invalidation_plan(
                CloudFrontInvalidationPlanRequest {
                    distribution_id: "E123EXAMPLE".to_string(),
                    operation_id: OperationId::new("invalidate-stale").unwrap(),
                    intent: CloudFrontInvalidationIntent::Paths(vec!["/index.html".to_string()]),
                    now_unix_ms: 2_000,
                },
                &inventory,
            )
            .unwrap_err()
            .code(),
            "stale_cloudfront_distribution_observation"
        );
    }

    #[test]
    fn full_inventory_scans_pages_accepts_status_progress_and_counts_active_items() {
        let summary = summary();
        let distribution_detail = detail(summary.clone());
        let invalidation_summary = CloudFrontInvalidationSummary {
            distribution_id: "E123EXAMPLE".to_string(),
            id: "I1".to_string(),
            status: CloudFrontInvalidationStatus::InProgress,
            created_at_unix_seconds: 1,
        };
        let api = Arc::new(InvalidationFakeApi {
            base: FakeApi {
                account_id: ACCOUNT_ID.to_string(),
                partition: AwsPartition::Aws,
                pages: vec![CloudFrontDistributionPage {
                    items: vec![summary],
                    is_truncated: false,
                    next_marker: None,
                }],
                detail: Some(distribution_detail),
                tags: CloudFrontTags::default(),
            },
            invalidation_pages: BTreeMap::from([
                (
                    None,
                    CloudFrontInvalidationPage {
                        distribution_id: "E123EXAMPLE".to_string(),
                        items: vec![invalidation_summary.clone()],
                        is_truncated: true,
                        next_marker: Some("I1".to_string()),
                    },
                ),
                (
                    Some("I1".to_string()),
                    CloudFrontInvalidationPage {
                        distribution_id: "E123EXAMPLE".to_string(),
                        items: vec![CloudFrontInvalidationSummary {
                            distribution_id: "E123EXAMPLE".to_string(),
                            id: "I2".to_string(),
                            status: CloudFrontInvalidationStatus::InProgress,
                            created_at_unix_seconds: 2,
                        }],
                        is_truncated: false,
                        next_marker: None,
                    },
                ),
            ]),
            invalidation_details: BTreeMap::from([
                (
                    "I1".to_string(),
                    CloudFrontInvalidationDetail {
                        distribution_id: "E123EXAMPLE".to_string(),
                        id: "I1".to_string(),
                        status: CloudFrontInvalidationStatus::Completed,
                        created_at_unix_seconds: 1,
                        caller_reference: "x".repeat(129),
                        paths: vec![
                            "/image.jpg?a=1".to_string(),
                            "/literal*middle".to_string(),
                            "#tag".to_string(),
                        ],
                    },
                ),
                (
                    "I2".to_string(),
                    CloudFrontInvalidationDetail {
                        distribution_id: "E123EXAMPLE".to_string(),
                        id: "I2".to_string(),
                        status: CloudFrontInvalidationStatus::InProgress,
                        created_at_unix_seconds: 2,
                        caller_reference: "active-operation".to_string(),
                        paths: vec!["/exact".to_string(), "/wild/*".to_string()],
                    },
                ),
            ]),
        });
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(async {
            let adapter = CloudFrontInventoryAdapter::new(
                CloudResourceId::new("aws-main").unwrap(),
                7,
                &ProviderAccountSpec {
                    provider: CloudProvider::Aws,
                    scope: Some(ProviderAccountScope::Aws {
                        account_id: ACCOUNT_ID.to_string(),
                    }),
                    credential_source: CredentialSource::Ambient,
                },
                api,
            )
            .unwrap();
            let distribution = adapter
                .planning_inventory("invalidation-pages", 1_000, 2_000)
                .await
                .unwrap();
            let observed = adapter
                .planning_invalidation_inventory(&distribution, "E123EXAMPLE", 1_500)
                .await
                .unwrap();
            assert_eq!(observed.items().len(), 2);
            assert_eq!(observed.active_non_wildcard_item_count(), 1);
            assert_eq!(observed.active_wildcard_path_count(), 1);
        });
    }
}
