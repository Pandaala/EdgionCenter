use std::collections::BTreeSet;

use async_trait::async_trait;
use edgion_center_core::NormalizedProviderError;
use serde::{Deserialize, Serialize};

use crate::CloudFrontEtagRevisionMac;

pub type CloudFrontApiResult<T> = Result<T, NormalizedProviderError>;

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
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloudFrontTags {
    pub keys: BTreeSet<String>,
    pub center_resource_id: Option<String>,
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
