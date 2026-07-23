use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use async_trait::async_trait;
use edgion_center_core::NormalizedProviderError;
use serde::{Deserialize, Serialize};

use crate::CloudFrontEtagRevisionMac;

pub type CloudFrontApiResult<T> = Result<T, NormalizedProviderError>;

/// Per-request evidence that a CloudFront mutation reached its provider dispatch boundary.
/// It carries no provider data and lets composition distinguish a pre-dispatch timeout from an
/// outcome that must be observed before retrying.
#[derive(Clone, Default)]
pub struct CloudFrontDispatchTracker(Arc<AtomicBool>);

impl CloudFrontDispatchTracker {
    pub fn was_dispatched(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    /// Marks the instant immediately before an AWS mutation is submitted. Calling this early is
    /// conservative: it can only turn an uncertain outcome into `UnknownOutcome`.
    pub fn mark_dispatched(&self) {
        self.0.store(true, Ordering::Release);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AwsPartition {
    Aws,
    AwsChina,
    AwsUsGov,
}

impl AwsPartition {
    pub fn arn_partition(self) -> &'static str {
        match self {
            Self::Aws => "aws",
            Self::AwsChina => "aws-cn",
            Self::AwsUsGov => "aws-us-gov",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontDistributionSummary {
    pub id: String,
    pub arn: String,
    pub domain_name: String,
    pub status: String,
    pub enabled: bool,
    pub last_modified_unix_seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudFrontDistributionPage {
    pub items: Vec<CloudFrontDistributionSummary>,
    pub is_truncated: bool,
    pub next_marker: Option<String>,
}

/// Inventory metadata only. The provider config is deliberately retained as sensitive raw wire
/// evidence inside the adapter, never projected into an origin/behavior planning DTO.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontDistributionDetail {
    pub summary: CloudFrontDistributionSummary,
    pub etag: String,
    pub etag_revision_mac: CloudFrontEtagRevisionMac,
    /// Current WAF association. This is an inventory projection only; CLD-29A owns mutations.
    pub web_acl_id: Option<String>,
    /// The only endpoint the retained lifecycle can update. `None` means an existing
    /// distribution is observable but does not match the fixed one-origin API shape.
    pub supported_origin: Option<CloudFrontHttpsOrigin>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloudFrontTags {
    pub keys: BTreeSet<String>,
    pub center_resource_id: Option<String>,
}

/// Fixed custom-origin input accepted by the minimal CloudFront lifecycle.
///
/// The lifecycle intentionally has no generic `DistributionConfig` input.  In particular it
/// cannot add aliases, policies, cache behaviours, origin groups, custom headers, or certificates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontHttpsOrigin {
    pub domain_name: String,
    pub https_port: u16,
}

/// Input for a single, enabled API distribution creation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontCreateDistribution {
    pub caller_reference: String,
    pub origin: CloudFrontHttpsOrigin,
}

/// The only endpoint fields the retained lifecycle may change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontOriginEndpointUpdate {
    pub domain_name: String,
    pub https_port: u16,
}

/// The sole CloudFront WAF association field. `None` explicitly detaches the current ACL.
/// Callers must validate any non-empty value as a same-account CLOUDFRONT-scope WAF ACL before
/// this reaches the CloudFront adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontWebAclUpdate {
    pub web_acl_id: Option<String>,
}

/// A sanitized result of a single CloudFront lifecycle request. Provider acceptance does not
/// imply the distribution is deployed; callers must observe `status == "Deployed"` separately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontLifecycleResult {
    pub distribution: CloudFrontDistributionDetail,
    pub deployed: bool,
}

/// Explicit confirmation for a destructive delete request.
///
/// The AWS adapter independently re-observes the configuration, ETag, disabled deployment, and
/// the product-visible reference constraints before it dispatches the delete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontDeleteGuard {
    pub confirmation: String,
}

/// Read-only inventory transport. It intentionally contains no planning or mutation methods.
#[async_trait]
pub trait CloudFrontApi: Send + Sync {
    fn verified_account_id(&self) -> &str;
    fn verified_partition(&self) -> AwsPartition;
    fn credential_revision(&self) -> &str;
    async fn list_distributions(
        &self,
        marker: Option<&str>,
        max_items: u16,
    ) -> CloudFrontApiResult<CloudFrontDistributionPage>;
    async fn get_distribution(
        &self,
        distribution_id: &str,
    ) -> CloudFrontApiResult<Option<CloudFrontDistributionDetail>>;
    async fn list_tags_for_resource(&self, arn: &str) -> CloudFrontApiResult<CloudFrontTags>;
}
