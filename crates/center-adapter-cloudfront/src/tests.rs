use std::{collections::BTreeSet, sync::Arc};

use async_trait::async_trait;
use edgion_center_core::{
    CloudProvider, CloudResourceId, CredentialSource, ProviderAccountScope, ProviderAccountSpec,
};

use super::*;

pub(crate) const ACCOUNT_ID: &str = "123456789012";
const DISTRIBUTION_ID: &str = "E123EXAMPLE";

pub(crate) struct FakeApi {
    pub(crate) account_id: String,
    pub(crate) partition: AwsPartition,
    pub(crate) pages: Vec<CloudFrontDistributionPage>,
    pub(crate) detail: Option<CloudFrontDistributionDetail>,
    pub(crate) tags: CloudFrontTags,
}

#[async_trait]
impl CloudFrontApi for FakeApi {
    fn verified_account_id(&self) -> &str {
        &self.account_id
    }

    fn verified_partition(&self) -> AwsPartition {
        self.partition
    }

    fn credential_revision(&self) -> &str {
        "credential-3"
    }

    async fn list_distributions(
        &self,
        marker: Option<&str>,
        _max_items: u16,
    ) -> CloudFrontApiResult<CloudFrontDistributionPage> {
        let index = marker
            .map(|value| value.parse::<usize>().unwrap())
            .unwrap_or(0);
        Ok(self.pages[index].clone())
    }

    async fn get_distribution(
        &self,
        _distribution_id: &str,
    ) -> CloudFrontApiResult<Option<CloudFrontDistributionDetail>> {
        Ok(self.detail.clone())
    }

    async fn list_policies(
        &self,
        _kind: CloudFrontPolicyKind,
        _scope: CloudFrontPolicyScope,
        _marker: Option<&str>,
        _max_items: u16,
    ) -> CloudFrontApiResult<CloudFrontPolicyPage> {
        Ok(CloudFrontPolicyPage {
            items: Vec::new(),
            next_marker: None,
        })
    }

    async fn list_tags_for_resource(&self, _arn: &str) -> CloudFrontApiResult<CloudFrontTags> {
        Ok(self.tags.clone())
    }
}

#[tokio::test]
async fn inventory_keeps_scope_freshness_and_ownership_as_non_authoritative_evidence() {
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
        tags: CloudFrontTags {
            keys: BTreeSet::from(["edgion.center/resource-id".to_string()]),
            center_resource_id: Some("cloudfront-observed".to_string()),
        },
    });
    let adapter = adapter(api);

    let inventory = adapter
        .inventory("observation-7", 1_000, 2_000)
        .await
        .unwrap();

    assert_eq!(inventory.authority.aws_account_id(), ACCOUNT_ID);
    assert_eq!(inventory.authority.account_generation(), 7);
    assert_eq!(inventory.authority.credential_revision(), "credential-3");
    assert_eq!(inventory.authority.observation_token(), "observation-7");
    assert!(inventory.authority.is_fresh_at(1_999));
    assert!(!inventory.authority.is_fresh_at(2_000));
    let CloudFrontDetailObservation::Complete(observed) = &inventory.entries[0].detail else {
        panic!("detail must be complete")
    };
    assert!(!observed.mutation_eligibility.is_eligible());
    assert!(matches!(
        inventory.entries[0].ownership_hint,
        CloudFrontOwnershipHint::Present { .. }
    ));
}

#[tokio::test]
async fn pagination_loop_fails_closed() {
    let api = Arc::new(FakeApi {
        account_id: ACCOUNT_ID.to_string(),
        partition: AwsPartition::Aws,
        pages: vec![CloudFrontDistributionPage {
            items: Vec::new(),
            is_truncated: true,
            next_marker: Some("0".to_string()),
        }],
        detail: None,
        tags: CloudFrontTags::default(),
    });

    let error = adapter(api)
        .inventory("observation-8", 1_000, 2_000)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "cloudfront_distribution_pagination_loop");
}

#[tokio::test]
async fn caller_cannot_extend_observation_freshness_without_bound() {
    let api = Arc::new(FakeApi {
        account_id: ACCOUNT_ID.to_string(),
        partition: AwsPartition::Aws,
        pages: vec![CloudFrontDistributionPage {
            items: Vec::new(),
            is_truncated: false,
            next_marker: None,
        }],
        detail: None,
        tags: CloudFrontTags::default(),
    });

    let error = adapter(api)
        .inventory(
            "observation-long",
            1_000,
            1_000 + MAX_FRESHNESS_WINDOW_MS + 1,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "cloudfront_observation_freshness_limit");
}

#[test]
fn cloudfront_origin_group_accepts_the_provider_supported_429_failover_code() {
    let mut observed = detail(summary());
    observed
        .config
        .origin_groups
        .push(CloudFrontOriginGroupProjection {
            id: "group-1".to_string(),
            primary_origin_id: "origin-1".to_string(),
            secondary_origin_id: "origin-2".to_string(),
            failover_status_codes: BTreeSet::from([429, 503]),
            unsupported_features: BTreeSet::new(),
        });
    let mut secondary = observed.config.origins[0].clone();
    secondary.id = "origin-2".to_string();
    observed.config.origins.push(secondary);
    observed.config.default_cache_behavior.target_origin_id = "group-1".to_string();

    validate_detail(&observed, AwsPartition::Aws, ACCOUNT_ID).unwrap();
}

#[test]
fn observed_origin_group_write_shape_is_retained_but_mutation_ineligible() {
    let mut observed = detail(summary());
    let mut secondary = observed.config.origins[0].clone();
    secondary.id = "origin-2".to_string();
    observed.config.origins.push(secondary);
    observed
        .config
        .origin_groups
        .push(CloudFrontOriginGroupProjection {
            id: "group-1".to_string(),
            primary_origin_id: "origin-1".to_string(),
            secondary_origin_id: "origin-2".to_string(),
            failover_status_codes: BTreeSet::from([503]),
            unsupported_features: BTreeSet::new(),
        });
    observed.config.default_cache_behavior.target_origin_id = "group-1".to_string();
    observed.config.default_cache_behavior.allowed_methods = BTreeSet::from([
        "GET".to_string(),
        "HEAD".to_string(),
        "OPTIONS".to_string(),
        "POST".to_string(),
        "PUT".to_string(),
        "PATCH".to_string(),
        "DELETE".to_string(),
    ]);
    observed.config.default_cache_behavior.cached_methods =
        BTreeSet::from(["GET".to_string(), "HEAD".to_string()]);

    validate_detail(&observed, AwsPartition::Aws, ACCOUNT_ID).unwrap();
    let CloudFrontMutationEligibility::Ineligible { reasons } = mutation_eligibility(&observed)
    else {
        panic!("provider-valid group write shape must remain read-only");
    };
    assert!(reasons.contains("origin_group_write_methods"));
    assert!(reasons.contains("origin_group_uncached_options"));
}

#[test]
fn cache_behavior_projection_accepts_only_provider_method_and_viewer_policy_shapes() {
    for viewer_policy in ["allow-all", "https-only", "redirect-to-https"] {
        let mut observed = detail(summary());
        observed
            .config
            .default_cache_behavior
            .viewer_protocol_policy = viewer_policy.to_string();
        validate_detail(&observed, AwsPartition::Aws, ACCOUNT_ID).unwrap();
    }

    for (allowed, cached) in [
        (
            BTreeSet::from(["GET".to_string(), "HEAD".to_string()]),
            BTreeSet::from(["GET".to_string(), "HEAD".to_string()]),
        ),
        (
            BTreeSet::from(["GET".to_string(), "HEAD".to_string(), "OPTIONS".to_string()]),
            BTreeSet::from(["GET".to_string(), "HEAD".to_string(), "OPTIONS".to_string()]),
        ),
        (
            BTreeSet::from([
                "GET".to_string(),
                "HEAD".to_string(),
                "OPTIONS".to_string(),
                "POST".to_string(),
                "PUT".to_string(),
                "PATCH".to_string(),
                "DELETE".to_string(),
            ]),
            BTreeSet::from(["GET".to_string(), "HEAD".to_string(), "OPTIONS".to_string()]),
        ),
    ] {
        let mut observed = detail(summary());
        observed.config.default_cache_behavior.allowed_methods = allowed;
        observed.config.default_cache_behavior.cached_methods = cached;
        validate_detail(&observed, AwsPartition::Aws, ACCOUNT_ID).unwrap();
    }

    let mut invalid_viewer_policy = detail(summary());
    invalid_viewer_policy
        .config
        .default_cache_behavior
        .viewer_protocol_policy = "match-viewer".to_string();
    assert_eq!(
        validate_detail(&invalid_viewer_policy, AwsPartition::Aws, ACCOUNT_ID)
            .unwrap_err()
            .code(),
        "invalid_cloudfront_cache_behavior"
    );

    let mut invalid_methods = detail(summary());
    invalid_methods
        .config
        .default_cache_behavior
        .allowed_methods =
        BTreeSet::from(["GET".to_string(), "HEAD".to_string(), "POST".to_string()]);
    assert_eq!(
        validate_detail(&invalid_methods, AwsPartition::Aws, ACCOUNT_ID)
            .unwrap_err()
            .code(),
        "invalid_cloudfront_cache_behavior"
    );

    let mut invalid_cached_methods = detail(summary());
    invalid_cached_methods
        .config
        .default_cache_behavior
        .cached_methods = BTreeSet::from(["GET".to_string()]);
    assert_eq!(
        validate_detail(&invalid_cached_methods, AwsPartition::Aws, ACCOUNT_ID)
            .unwrap_err()
            .code(),
        "invalid_cloudfront_cache_behavior"
    );

    let mut options_not_cached = detail(summary());
    options_not_cached
        .config
        .default_cache_behavior
        .allowed_methods
        .insert("OPTIONS".to_string());
    validate_detail(&options_not_cached, AwsPartition::Aws, ACCOUNT_ID).unwrap();
}

#[test]
fn cache_behavior_projection_requires_modern_policy_but_retains_marked_legacy_shape() {
    let mut missing_policy = detail(summary());
    missing_policy.config.default_cache_behavior.cache_policy_id = None;
    assert_eq!(
        validate_detail(&missing_policy, AwsPartition::Aws, ACCOUNT_ID)
            .unwrap_err()
            .code(),
        "missing_cloudfront_cache_policy_id"
    );

    let mut legacy = missing_policy;
    legacy
        .config
        .default_cache_behavior
        .unsupported_features
        .insert("legacy_forwarded_values".to_string());
    validate_detail(&legacy, AwsPartition::Aws, ACCOUNT_ID).unwrap();
    assert!(matches!(
        mutation_eligibility(&legacy),
        CloudFrontMutationEligibility::Ineligible { ref reasons }
            if reasons.contains("legacy_forwarded_values")
    ));
}

#[test]
fn cache_behavior_projection_conservatively_validates_default_and_ordered_paths() {
    let mut valid = detail(summary());
    let mut ordered = valid.config.default_cache_behavior.clone();
    ordered.path_pattern = Some("/assets/*.js".to_string());
    valid.config.ordered_cache_behaviors.push(ordered);
    validate_detail(&valid, AwsPartition::Aws, ACCOUNT_ID).unwrap();

    let mut default_with_path = detail(summary());
    default_with_path.config.default_cache_behavior.path_pattern = Some("/api/*".to_string());
    assert_eq!(
        validate_detail(&default_with_path, AwsPartition::Aws, ACCOUNT_ID)
            .unwrap_err()
            .code(),
        "invalid_cloudfront_cache_behavior"
    );

    for invalid_path in ["", "/bad path/*", "/bad%2/*", "/bad#fragment"] {
        let mut observed = detail(summary());
        let mut ordered = observed.config.default_cache_behavior.clone();
        ordered.path_pattern = Some(invalid_path.to_string());
        observed.config.ordered_cache_behaviors.push(ordered);
        assert_eq!(
            validate_detail(&observed, AwsPartition::Aws, ACCOUNT_ID)
                .unwrap_err()
                .code(),
            "invalid_cloudfront_cache_behavior_path"
        );
    }
}

#[test]
fn serialized_authority_must_be_revalidated_and_fingerprints_are_scope_bound() {
    let authority = CloudFrontObservationAuthority::new(
        CloudResourceId::new("aws-main").unwrap(),
        ACCOUNT_ID.to_string(),
        AwsPartition::Aws,
        7,
        "credential-3".to_string(),
        "observation-9".to_string(),
        1_000,
        2_000,
    )
    .unwrap();
    let mut value = serde_json::to_value(authority).unwrap();
    value["accountGeneration"] = serde_json::json!(0);
    let decoded: CloudFrontObservationAuthority = serde_json::from_value(value).unwrap();
    assert_eq!(
        decoded.validate().unwrap_err().code(),
        "invalid_cloudfront_account_generation"
    );

    let key = CloudFrontFingerprintKey::new([7; 32]).unwrap();
    let first = key
        .mac_etag_revision(AwsPartition::Aws, ACCOUNT_ID, DISTRIBUTION_ID, "E1")
        .unwrap();
    let second = key
        .mac_etag_revision(AwsPartition::Aws, "999999999999", DISTRIBUTION_ID, "E1")
        .unwrap();
    assert_ne!(first, second);
}

pub(crate) fn adapter(api: Arc<FakeApi>) -> CloudFrontInventoryAdapter {
    CloudFrontInventoryAdapter::new(
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
    .unwrap()
}

pub(crate) fn summary() -> CloudFrontDistributionSummary {
    CloudFrontDistributionSummary {
        id: DISTRIBUTION_ID.to_string(),
        arn: format!("arn:aws:cloudfront::{ACCOUNT_ID}:distribution/{DISTRIBUTION_ID}"),
        domain_name: "d123example.cloudfront.net".to_string(),
        status: "Deployed".to_string(),
        enabled: true,
        last_modified_unix_seconds: 1_000,
    }
}

pub(crate) fn detail(summary: CloudFrontDistributionSummary) -> CloudFrontDistributionDetail {
    CloudFrontDistributionDetail {
        summary,
        etag: "E2DETAIL".to_string(),
        etag_revision_mac: CloudFrontFingerprintKey::new([9; 32])
            .unwrap()
            .mac_etag_revision(AwsPartition::Aws, ACCOUNT_ID, DISTRIBUTION_ID, "E2DETAIL")
            .unwrap(),
        config: CloudFrontDistributionConfigProjection {
            caller_reference: "caller-reference".to_string(),
            aliases: BTreeSet::new(),
            default_root_object: String::new(),
            origins: vec![CloudFrontOriginProjection {
                id: "origin-1".to_string(),
                domain_name: "origin.example.test".to_string(),
                origin_path: String::new(),
                kind: CloudFrontOriginKind::Custom,
                http_port: Some(80),
                https_port: Some(443),
                protocol_policy: Some("https-only".to_string()),
                tls_protocols: BTreeSet::from(["TLSv1.2".to_string()]),
                connection_attempts: 3,
                connection_timeout_seconds: 10,
                response_timeout_seconds: Some(30),
                keepalive_timeout_seconds: Some(5),
                custom_header_count: 0,
                unsupported_features: BTreeSet::new(),
            }],
            origin_groups: Vec::new(),
            default_cache_behavior: CloudFrontCacheBehaviorProjection {
                path_pattern: None,
                target_origin_id: "origin-1".to_string(),
                viewer_protocol_policy: "redirect-to-https".to_string(),
                allowed_methods: BTreeSet::from(["GET".to_string(), "HEAD".to_string()]),
                cached_methods: BTreeSet::from(["GET".to_string(), "HEAD".to_string()]),
                compress: true,
                cache_policy_id: Some("managed-cache-policy".to_string()),
                origin_request_policy_id: None,
                response_headers_policy_id: None,
                field_level_encryption_id: None,
                realtime_log_config_arn: None,
                unsupported_features: BTreeSet::new(),
            },
            ordered_cache_behaviors: Vec::new(),
            custom_error_responses: Vec::new(),
            comment: String::new(),
            logging: CloudFrontLoggingProjection {
                enabled: false,
                include_cookies: false,
                bucket: String::new(),
                prefix: String::new(),
            },
            price_class: "PriceClass_All".to_string(),
            enabled: true,
            viewer_certificate: CloudFrontViewerCertificateProjection {
                cloudfront_default_certificate: true,
                certificate_arn: None,
                certificate_source: Some("cloudfront".to_string()),
                ssl_support_method: None,
                minimum_protocol_version: "TLSv1".to_string(),
            },
            geo_restriction: CloudFrontGeoRestrictionProjection {
                restriction_type: "none".to_string(),
                locations: BTreeSet::new(),
            },
            web_acl_id: String::new(),
            http_version: "http2".to_string(),
            ipv6_enabled: true,
            staging: false,
            continuous_deployment_policy_id: String::new(),
            unsupported_features: BTreeSet::new(),
        },
    }
}
