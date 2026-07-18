//! Checked-in AWS endpoint catalog used by trusted process composition.

use async_trait::async_trait;
use edgion_center_core::{CloudResourceId, DnsZoneId};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::{
    model::validation, AwsPartition, CloudFrontAliasCatalogRecord, CloudFrontAliasCatalogSource,
    CloudFrontAliasCatalogTargetKind, CloudFrontApiResult,
};

const EMBEDDED_CATALOG: &[u8] = include_bytes!("../catalog/aws-cloudfront-aliases-v1.json");
const CATALOG_SCHEMA_VERSION: u32 = 1;
const MAX_CATALOG_ENTRIES: usize = 16;
const CATALOG_SOURCE_ID: &str = "aws-cloudfront-alias-catalog";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CatalogDocument {
    schema_version: u32,
    source_id: String,
    entries: Vec<CatalogEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CatalogEntry {
    partition: AwsPartition,
    target_kind: CatalogTargetKind,
    dns_suffix: String,
    hosted_zone_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CatalogTargetKind {
    StandardDistribution,
}

/// Release-versioned endpoint catalog compiled into the Center process.
///
/// The digest is derived from the exact artifact bytes rather than accepted
/// from configuration or a request. Construction validates the complete
/// artifact so composition can fail before exposing a cloud API.
#[derive(Debug, Clone)]
pub struct EmbeddedAwsEndpointCatalog {
    source_id: CloudResourceId,
    revision: String,
    entries: Vec<CatalogEntry>,
}

impl EmbeddedAwsEndpointCatalog {
    pub fn load() -> CloudFrontApiResult<Self> {
        Self::from_bytes(EMBEDDED_CATALOG)
    }

    fn from_bytes(bytes: &[u8]) -> CloudFrontApiResult<Self> {
        let document: CatalogDocument = serde_json::from_slice(bytes)
            .map_err(|_| validation("invalid_cloudfront_alias_catalog_artifact"))?;
        if document.schema_version != CATALOG_SCHEMA_VERSION
            || document.source_id != CATALOG_SOURCE_ID
            || document.entries.is_empty()
            || document.entries.len() > MAX_CATALOG_ENTRIES
        {
            return Err(validation("invalid_cloudfront_alias_catalog_artifact"));
        }
        let source_id = CloudResourceId::new(document.source_id)
            .map_err(|_| validation("invalid_cloudfront_alias_catalog_source"))?;

        for (index, entry) in document.entries.iter().enumerate() {
            if entry.partition != AwsPartition::Aws
                || entry.target_kind != CatalogTargetKind::StandardDistribution
                || entry.dns_suffix != "cloudfront.net"
                || DnsZoneId::new(entry.hosted_zone_id.clone()).is_err()
                || document.entries[..index].iter().any(|previous| {
                    previous.partition == entry.partition
                        && previous.target_kind == entry.target_kind
                })
            {
                return Err(validation("invalid_cloudfront_alias_catalog_artifact"));
            }
        }

        Ok(Self {
            source_id,
            revision: format!("sha256:{:x}", Sha256::digest(bytes)),
            entries: document.entries,
        })
    }

    pub fn revision(&self) -> &str {
        &self.revision
    }
}

#[async_trait]
impl CloudFrontAliasCatalogSource for EmbeddedAwsEndpointCatalog {
    async fn standard_distribution_alias(
        &self,
        partition: AwsPartition,
    ) -> CloudFrontApiResult<Option<CloudFrontAliasCatalogRecord>> {
        Ok(self
            .entries
            .iter()
            .find(|entry| {
                entry.partition == partition
                    && entry.target_kind == CatalogTargetKind::StandardDistribution
            })
            .map(|entry| CloudFrontAliasCatalogRecord {
                source_id: self.source_id.clone(),
                revision: self.revision.clone(),
                partition: entry.partition,
                target_kind: CloudFrontAliasCatalogTargetKind::StandardDistribution,
                dns_suffix: entry.dns_suffix.clone(),
                hosted_zone_id: entry.hosted_zone_id.clone(),
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn embedded_catalog_is_complete_and_digest_versioned() {
        let catalog = EmbeddedAwsEndpointCatalog::load().unwrap();
        assert_eq!(
            catalog.revision(),
            "sha256:41317ba271a469cca324feedb4651778bfb0226136cdab262360553a39b7a4d5"
        );
        let record = catalog
            .standard_distribution_alias(AwsPartition::Aws)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(record.source_id.as_str(), "aws-cloudfront-alias-catalog");
        assert_eq!(record.dns_suffix, "cloudfront.net");
        assert_eq!(record.hosted_zone_id, "Z2FDTNDATAQYW2");
        assert!(catalog
            .standard_distribution_alias(AwsPartition::AwsChina)
            .await
            .unwrap()
            .is_none());
    }

    #[test]
    fn malformed_duplicate_and_untrusted_entries_fail_closed() {
        for artifact in [
            br#"{"schemaVersion":1,"sourceId":"catalog","entries":[],"unknown":true}"#.as_slice(),
            br#"{"schemaVersion":2,"sourceId":"catalog","entries":[{"partition":"aws","targetKind":"standard_distribution","dnsSuffix":"cloudfront.net","hostedZoneId":"Z1"}]}"#.as_slice(),
            br#"{"schemaVersion":1,"sourceId":"catalog","entries":[{"partition":"aws_china","targetKind":"standard_distribution","dnsSuffix":"cloudfront.net","hostedZoneId":"Z1"}]}"#.as_slice(),
            br#"{"schemaVersion":1,"sourceId":"catalog","entries":[{"partition":"aws","targetKind":"standard_distribution","dnsSuffix":"cloudfront.net","hostedZoneId":"Z1"},{"partition":"aws","targetKind":"standard_distribution","dnsSuffix":"cloudfront.net","hostedZoneId":"Z2"}]}"#.as_slice(),
        ] {
            assert_eq!(
                EmbeddedAwsEndpointCatalog::from_bytes(artifact)
                    .unwrap_err()
                    .code(),
                "invalid_cloudfront_alias_catalog_artifact"
            );
        }
    }
}
