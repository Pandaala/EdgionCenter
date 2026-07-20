use std::collections::BTreeSet;

use aws_sdk_cloudfront::types::DistributionConfig;
use serde::{Deserialize, Serialize};

use crate::{
    validation, AwsCloudFrontApi, CloudFrontApi, CloudFrontApiResult, CloudFrontDetailObservation,
    CloudFrontMutationIntentMac, CloudFrontPlanningInventory, ObservedCloudFrontDistributionDetail,
};

const MUTATION_WINDOW_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub(crate) fn cloudfront_enablement_infrastructure_blockers() -> BTreeSet<String> {
    BTreeSet::from([
        "approval_authority_unavailable".to_string(),
        "executor_unavailable".to_string(),
        "ownership_authority_unavailable".to_string(),
        "provider_write_reliability_unavailable".to_string(),
        "secret_memory_zeroization_unavailable".to_string(),
    ])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudFrontEnablementAction {
    Enable,
    Disable,
}

impl CloudFrontEnablementAction {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Enable => "enable",
            Self::Disable => "disable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudFrontEnablementRisk {
    TrafficRestoring,
    TrafficStopping,
}

impl CloudFrontEnablementRisk {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::TrafficRestoring => "traffic_restoring",
            Self::TrafficStopping => "traffic_stopping",
        }
    }
}

/// Sanitized, explicitly non-dispatchable preview of an enable/disable composition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontEnablementMutationPlan {
    provider_account_id: edgion_center_core::CloudResourceId,
    aws_account_id: String,
    partition: crate::AwsPartition,
    account_generation: u64,
    credential_revision: String,
    valid_until_unix_ms: i64,
    distribution_id: String,
    distribution_arn: String,
    current_enabled: bool,
    desired_enabled: bool,
    action: CloudFrontEnablementAction,
    risk: CloudFrontEnablementRisk,
    intent_revision: CloudFrontMutationIntentMac,
    desired_wire_revision: crate::CloudFrontDesiredWireRevisionMac,
    plan_revision: crate::CloudFrontEnablementPlanRevisionMac,
    write_set: BTreeSet<String>,
    provider_config_reread: bool,
    wire_fidelity_proven: bool,
    dispatch_blockers: BTreeSet<String>,
    preview_only: bool,
}

impl CloudFrontEnablementMutationPlan {
    pub fn dispatch_blockers(&self) -> &BTreeSet<String> {
        &self.dispatch_blockers
    }

    pub fn is_preview_only(&self) -> bool {
        self.preview_only
    }

    pub fn wire_fidelity_proven(&self) -> bool {
        self.wire_fidelity_proven
    }

    pub fn plan_revision(&self) -> &crate::CloudFrontEnablementPlanRevisionMac {
        &self.plan_revision
    }

    pub fn action(&self) -> CloudFrontEnablementAction {
        self.action
    }

    pub fn risk(&self) -> CloudFrontEnablementRisk {
        self.risk
    }

    pub(crate) fn provider_account_id(&self) -> &edgion_center_core::CloudResourceId {
        &self.provider_account_id
    }

    pub(crate) fn account_generation(&self) -> u64 {
        self.account_generation
    }

    pub(crate) fn credential_revision(&self) -> &str {
        &self.credential_revision
    }

    pub(crate) fn aws_account_id(&self) -> &str {
        &self.aws_account_id
    }

    pub(crate) fn partition(&self) -> crate::AwsPartition {
        self.partition
    }

    pub(crate) fn distribution_id(&self) -> &str {
        &self.distribution_id
    }

    pub(crate) fn distribution_arn(&self) -> &str {
        &self.distribution_arn
    }

    pub(crate) fn valid_until_unix_ms(&self) -> i64 {
        self.valid_until_unix_ms
    }

    pub(crate) fn current_enabled(&self) -> bool {
        self.current_enabled
    }

    pub(crate) fn desired_enabled(&self) -> bool {
        self.desired_enabled
    }

    pub(crate) fn write_set(&self) -> &BTreeSet<String> {
        &self.write_set
    }

    #[cfg(test)]
    pub(crate) fn test_value(
        current_enabled: bool,
        desired_enabled: bool,
        dispatch_blockers: BTreeSet<String>,
    ) -> Self {
        let (action, risk) = if desired_enabled {
            (
                CloudFrontEnablementAction::Enable,
                CloudFrontEnablementRisk::TrafficRestoring,
            )
        } else {
            (
                CloudFrontEnablementAction::Disable,
                CloudFrontEnablementRisk::TrafficStopping,
            )
        };
        Self {
            provider_account_id: edgion_center_core::CloudResourceId::new("aws-main")
                .expect("provider account"),
            aws_account_id: "123456789012".to_string(),
            partition: crate::AwsPartition::Aws,
            account_generation: 7,
            credential_revision: "credential-3".to_string(),
            valid_until_unix_ms: 2_000,
            distribution_id: "E123EXAMPLE".to_string(),
            distribution_arn: "arn:aws:cloudfront::123456789012:distribution/E123EXAMPLE"
                .to_string(),
            current_enabled,
            desired_enabled,
            action,
            risk,
            intent_revision: CloudFrontMutationIntentMac::test_value('a'),
            desired_wire_revision: crate::CloudFrontDesiredWireRevisionMac::test_value('b'),
            plan_revision: crate::CloudFrontEnablementPlanRevisionMac::test_value('c'),
            write_set: if current_enabled != desired_enabled {
                BTreeSet::from(["enabled".to_string()])
            } else {
                BTreeSet::new()
            },
            provider_config_reread: true,
            wire_fidelity_proven: true,
            dispatch_blockers,
            preview_only: true,
        }
    }
}

fn validate_enablement_write_set(
    current: &DistributionConfig,
    mut desired: DistributionConfig,
) -> CloudFrontApiResult<()> {
    let caller_reference_unchanged = desired.caller_reference == current.caller_reference;
    desired.enabled = current.enabled;
    if desired != *current || !caller_reference_unchanged {
        return Err(validation("cloudfront_mutation_write_set_violation"));
    }
    Ok(())
}

/// Re-reads the full provider configuration and composes only the `Enabled` overlay.
///
/// This is a planning primitive, not mutation authority. CLD-05 ownership/adoption, CLD-06
/// approval, and CLD-37 write reliability must all land before a separate executor may consume
/// this shape. Wire fidelity is proven only for this fresh, bounded planning window.
pub async fn plan_cloudfront_enablement_mutation(
    api: &AwsCloudFrontApi,
    inventory: &CloudFrontPlanningInventory,
    distribution_id: &str,
    desired_enabled: bool,
    now_unix_ms: i64,
) -> CloudFrontApiResult<CloudFrontEnablementMutationPlan> {
    let inventory = inventory.inventory();
    let observed = validate_mutation_observation(
        inventory,
        api.verified_account_id(),
        api.verified_partition(),
        api.credential_revision(),
        distribution_id,
        now_unix_ms,
    )?;
    let authority = &inventory.authority;
    let snapshot = tokio::time::timeout(
        MUTATION_WINDOW_TIMEOUT,
        api.read_sensitive_sdk_config_snapshot(distribution_id),
    )
    .await
    .map_err(|_| validation("cloudfront_mutation_reread_deadline_exceeded"))??
    .ok_or_else(|| validation("cloudfront_mutation_distribution_disappeared"))?;
    validate_mutation_reread(observed, &snapshot.etag, &snapshot.projection)?;

    let current = snapshot.config;
    let mut desired = current.clone();
    desired.enabled = desired_enabled;
    validate_enablement_write_set(&current, desired.clone())?;
    let wire_evidence = api
        .admit_enablement_wire_fidelity(
            distribution_id,
            &snapshot.etag,
            &snapshot.wire,
            &current,
            &desired,
        )
        .await?;
    let mut dispatch_blockers = match &observed.mutation_eligibility {
        crate::CloudFrontMutationEligibility::Eligible => BTreeSet::new(),
        crate::CloudFrontMutationEligibility::Ineligible { reasons } => reasons.clone(),
    };
    dispatch_blockers.remove("wire_schema_not_lossless");
    dispatch_blockers.remove("full_config_revision_unavailable");
    dispatch_blockers.extend(cloudfront_enablement_infrastructure_blockers());
    let write_set = if current.enabled == desired_enabled {
        BTreeSet::new()
    } else {
        BTreeSet::from(["enabled".to_string()])
    };
    let (action, risk) = if desired_enabled {
        (
            CloudFrontEnablementAction::Enable,
            CloudFrontEnablementRisk::TrafficRestoring,
        )
    } else {
        (
            CloudFrontEnablementAction::Disable,
            CloudFrontEnablementRisk::TrafficStopping,
        )
    };
    let intent_revision = api.enablement_intent_mac(
        authority.provider_account_id(),
        authority.account_generation(),
        authority.credential_revision(),
        distribution_id,
        &snapshot.etag,
        desired_enabled,
    )?;
    let desired_wire_revision = api.desired_wire_revision_mac(
        authority.provider_account_id(),
        authority.account_generation(),
        authority.credential_revision(),
        distribution_id,
        &snapshot.etag,
        wire_evidence.desired_wire(),
    )?;
    let plan_revision = api.enablement_plan_revision_mac(
        authority.provider_account_id(),
        authority.account_generation(),
        authority.credential_revision(),
        distribution_id,
        &observed.detail.summary.arn,
        authority.valid_until_unix_ms(),
        action.label(),
        risk.label(),
        &intent_revision,
        &desired_wire_revision,
        &write_set,
        &dispatch_blockers,
    )?;
    let plan = CloudFrontEnablementMutationPlan {
        provider_account_id: authority.provider_account_id().clone(),
        aws_account_id: authority.aws_account_id().to_string(),
        partition: authority.partition(),
        account_generation: authority.account_generation(),
        credential_revision: authority.credential_revision().to_string(),
        valid_until_unix_ms: authority.valid_until_unix_ms(),
        distribution_id: observed.detail.summary.id.clone(),
        distribution_arn: observed.detail.summary.arn.clone(),
        current_enabled: current.enabled,
        desired_enabled,
        action,
        risk,
        intent_revision,
        desired_wire_revision,
        plan_revision,
        write_set,
        provider_config_reread: true,
        wire_fidelity_proven: true,
        dispatch_blockers,
        preview_only: true,
    };
    Ok(plan)
}

fn validate_mutation_observation<'a>(
    inventory: &'a crate::CloudFrontInventory,
    verified_account_id: &str,
    verified_partition: crate::AwsPartition,
    credential_revision: &str,
    distribution_id: &str,
    now_unix_ms: i64,
) -> CloudFrontApiResult<&'a ObservedCloudFrontDistributionDetail> {
    let authority = &inventory.authority;
    authority.validate()?;
    if !authority.is_fresh_at(now_unix_ms) {
        return Err(validation("cloudfront_mutation_inventory_expired"));
    }
    if authority.aws_account_id() != verified_account_id
        || authority.partition() != verified_partition
        || authority.credential_revision() != credential_revision
    {
        return Err(validation("cloudfront_mutation_provider_scope_mismatch"));
    }

    let entry = inventory
        .entries
        .iter()
        .find(|entry| entry.summary.id == distribution_id)
        .ok_or_else(|| validation("cloudfront_mutation_distribution_not_observed"))?;
    let CloudFrontDetailObservation::Complete(observed) = &entry.detail else {
        return Err(validation("cloudfront_mutation_detail_incomplete"));
    };
    if observed.changed_since_summary || observed.detail.summary.status != "Deployed" {
        return Err(validation("cloudfront_mutation_distribution_not_stable"));
    }

    Ok(observed)
}

fn validate_mutation_reread(
    observed: &ObservedCloudFrontDistributionDetail,
    reread_etag: &str,
    reread_projection: &crate::CloudFrontDistributionConfigProjection,
) -> CloudFrontApiResult<()> {
    if reread_etag != observed.detail.etag {
        return Err(validation("cloudfront_mutation_etag_changed"));
    }
    if reread_projection != &observed.detail.config {
        return Err(validation("cloudfront_mutation_config_changed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_config::BehaviorVersion;
    use aws_sdk_cloudfront::config::{Credentials, Region};
    use wiremock::{
        matchers::{body_string_contains, header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    fn minimal_config(enabled: bool) -> DistributionConfig {
        DistributionConfig::builder()
            .caller_reference("caller-reference")
            .comment("test")
            .enabled(enabled)
            .build()
            .expect("minimum generated SDK config")
    }

    #[test]
    fn enablement_composition_changes_only_enabled() {
        let current = minimal_config(false);
        let mut desired = current.clone();
        desired.enabled = true;
        assert!(validate_enablement_write_set(&current, desired).is_ok());
    }

    #[test]
    fn write_set_validation_rejects_other_field_changes() {
        let current = minimal_config(false);
        let mut desired = current.clone();
        desired.enabled = true;
        desired.comment = "changed".to_string();
        assert_eq!(
            validate_enablement_write_set(&current, desired)
                .expect_err("must reject")
                .code(),
            "cloudfront_mutation_write_set_violation"
        );
    }

    #[test]
    fn intent_mac_is_bound_to_the_complete_logical_account_scope() {
        let key = crate::CloudFrontFingerprintKey::new([7; 32]).expect("key");
        let account_a = edgion_center_core::CloudResourceId::new("account-a").expect("account");
        let account_b = edgion_center_core::CloudResourceId::new("account-b").expect("account");
        let mint = |provider_account: &edgion_center_core::CloudResourceId,
                    generation: u64,
                    credential_revision: &str| {
            key.mac_enablement_intent(
                provider_account,
                generation,
                credential_revision,
                crate::AwsPartition::Aws,
                "123456789012",
                "E123EXAMPLE",
                "E2DETAIL",
                false,
            )
            .expect("intent MAC")
        };
        let baseline = mint(&account_a, 7, "credential-3");
        assert_ne!(baseline, mint(&account_b, 7, "credential-3"));
        assert_ne!(baseline, mint(&account_a, 8, "credential-3"));
        assert_ne!(baseline, mint(&account_a, 7, "credential-4"));
    }

    #[test]
    fn observation_guards_fail_closed_before_provider_reread() {
        let inventory = test_inventory();
        let validate = |inventory: &crate::CloudFrontInventory,
                        account: &str,
                        partition: crate::AwsPartition,
                        credential: &str,
                        distribution: &str,
                        now: i64| {
            validate_mutation_observation(
                inventory,
                account,
                partition,
                credential,
                distribution,
                now,
            )
            .map(|_| ())
            .map_err(|error| error.code().to_string())
        };
        assert_eq!(
            validate(
                &inventory,
                "123456789012",
                crate::AwsPartition::Aws,
                "credential-3",
                "E123EXAMPLE",
                2_000,
            ),
            Err("cloudfront_mutation_inventory_expired".to_string())
        );
        for (account, partition, credential) in [
            ("999999999999", crate::AwsPartition::Aws, "credential-3"),
            (
                "123456789012",
                crate::AwsPartition::AwsChina,
                "credential-3",
            ),
            ("123456789012", crate::AwsPartition::Aws, "credential-4"),
        ] {
            assert_eq!(
                validate(
                    &inventory,
                    account,
                    partition,
                    credential,
                    "E123EXAMPLE",
                    1_500,
                ),
                Err("cloudfront_mutation_provider_scope_mismatch".to_string())
            );
        }
        assert_eq!(
            validate(
                &inventory,
                "123456789012",
                crate::AwsPartition::Aws,
                "credential-3",
                "ENOTOBSERVED",
                1_500,
            ),
            Err("cloudfront_mutation_distribution_not_observed".to_string())
        );

        let mut partial = test_inventory();
        partial.entries[0].detail =
            CloudFrontDetailObservation::Partial(crate::CloudFrontDetailIssue {
                kind: crate::CloudFrontDetailIssueKind::Unavailable,
                code: "test".to_string(),
            });
        assert_eq!(
            validate(
                &partial,
                "123456789012",
                crate::AwsPartition::Aws,
                "credential-3",
                "E123EXAMPLE",
                1_500,
            ),
            Err("cloudfront_mutation_detail_incomplete".to_string())
        );

        let mut unstable = test_inventory();
        let CloudFrontDetailObservation::Complete(observed) = &mut unstable.entries[0].detail
        else {
            unreachable!()
        };
        observed.changed_since_summary = true;
        assert_eq!(
            validate(
                &unstable,
                "123456789012",
                crate::AwsPartition::Aws,
                "credential-3",
                "E123EXAMPLE",
                1_500,
            ),
            Err("cloudfront_mutation_distribution_not_stable".to_string())
        );
    }

    #[test]
    fn reread_guards_reject_etag_and_projection_drift() {
        let inventory = test_inventory();
        let CloudFrontDetailObservation::Complete(observed) = &inventory.entries[0].detail else {
            unreachable!()
        };
        assert_eq!(
            validate_mutation_reread(observed, "E2CHANGED", &observed.detail.config)
                .expect_err("ETag drift")
                .code(),
            "cloudfront_mutation_etag_changed"
        );
        let mut changed = observed.detail.config.clone();
        changed.comment = "changed".to_string();
        assert_eq!(
            validate_mutation_reread(observed, "E2DETAIL", &changed)
                .expect_err("projection drift")
                .code(),
            "cloudfront_mutation_config_changed"
        );
    }

    fn test_inventory() -> crate::CloudFrontInventory {
        let summary = crate::tests::summary();
        crate::CloudFrontInventory {
            authority: crate::CloudFrontObservationAuthority::new(
                edgion_center_core::CloudResourceId::new("aws-main").expect("resource"),
                "123456789012".to_string(),
                crate::AwsPartition::Aws,
                7,
                "credential-3".to_string(),
                "observation-1".to_string(),
                1_000,
                2_000,
            )
            .expect("authority"),
            entries: vec![crate::CloudFrontInventoryEntry {
                summary: summary.clone(),
                detail: CloudFrontDetailObservation::Complete(Box::new(
                    crate::ObservedCloudFrontDistributionDetail {
                        detail: crate::tests::detail(summary),
                        mutation_eligibility: crate::CloudFrontMutationEligibility::Ineligible {
                            reasons: BTreeSet::from(["wire_schema_not_lossless".to_string()]),
                        },
                        changed_since_summary: false,
                    },
                )),
                ownership_hint: crate::CloudFrontOwnershipHint::Absent,
            }],
        }
    }

    #[tokio::test]
    async fn live_reread_builds_a_non_dispatchable_secret_free_plan() {
        let server = MockServer::start().await;
        let api = test_api(&server).await;
        Mock::given(method("GET"))
            .and(path("/2020-05-31/distribution/E123EXAMPLE/config"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("etag", "E2DETAIL")
                    .set_body_string(raw_config_xml("VALUE-SENTINEL")),
            )
            .expect(2)
            .mount(&server)
            .await;

        let summary = crate::tests::summary();
        let observed_snapshot = api
            .read_sensitive_sdk_config_snapshot("E123EXAMPLE")
            .await
            .expect("snapshot read")
            .expect("snapshot");
        let mut detail = crate::tests::detail(summary.clone());
        detail.config = observed_snapshot.projection;
        let inventory = CloudFrontPlanningInventory(crate::CloudFrontInventory {
            authority: crate::CloudFrontObservationAuthority::new(
                edgion_center_core::CloudResourceId::new("aws-main").expect("resource"),
                "123456789012".to_string(),
                crate::AwsPartition::Aws,
                7,
                "credential-3".to_string(),
                "observation-1".to_string(),
                1_000,
                2_000,
            )
            .expect("authority"),
            entries: vec![crate::CloudFrontInventoryEntry {
                summary,
                detail: CloudFrontDetailObservation::Complete(Box::new(
                    crate::ObservedCloudFrontDistributionDetail {
                        detail,
                        mutation_eligibility: crate::CloudFrontMutationEligibility::Ineligible {
                            reasons: BTreeSet::from([
                                "staging_distribution".to_string(),
                                "wire_schema_not_lossless".to_string(),
                            ]),
                        },
                        changed_since_summary: false,
                    },
                )),
                ownership_hint: crate::CloudFrontOwnershipHint::Absent,
            }],
        });

        let plan =
            plan_cloudfront_enablement_mutation(&api, &inventory, "E123EXAMPLE", false, 1_500)
                .await
                .expect("plan");
        assert!(plan.is_preview_only());
        assert!(plan.wire_fidelity_proven());
        assert!(!plan
            .dispatch_blockers()
            .contains("wire_schema_not_lossless"));
        assert!(!plan
            .dispatch_blockers()
            .contains("full_config_revision_unavailable"));
        assert!(plan.dispatch_blockers().contains("staging_distribution"));
        assert!(plan
            .dispatch_blockers()
            .contains("secret_memory_zeroization_unavailable"));
        let serialized = serde_json::to_string(&plan).expect("serialize plan");
        assert!(!serialized.contains("VALUE-SENTINEL"));
        assert!(!serialized.contains("X-Secret-Name-SENTINEL"));
        assert!(server
            .received_requests()
            .await
            .expect("requests")
            .iter()
            .filter(|request| request.url.path() != "/")
            .all(|request| request.method.as_str() == "GET"));
    }

    #[tokio::test]
    async fn update_transport_submits_once_and_returns_only_acceptance() {
        let server = MockServer::start().await;
        let api = test_api(&server).await;
        Mock::given(method("GET"))
            .and(path("/2020-05-31/distribution/E123EXAMPLE/config"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("etag", "E2DETAIL")
                    .set_body_string(raw_config_xml("VALUE-SENTINEL")),
            )
            .expect(1)
            .mount(&server)
            .await;
        let mut config = api
            .read_sensitive_sdk_config_snapshot("E123EXAMPLE")
            .await
            .expect("read config")
            .expect("existing config")
            .config;
        config.enabled = false;
        Mock::given(method("PUT"))
            .and(path("/2020-05-31/distribution/E123EXAMPLE/config"))
            .and(header("if-match", "E2DETAIL"))
            .and(body_string_contains("<Enabled>false</Enabled>"))
            .and(body_string_contains("X-Secret-Name-SENTINEL"))
            .and(body_string_contains("VALUE-SENTINEL"))
            .respond_with(|request: &wiremock::Request| {
                let request_body = String::from_utf8(request.body.clone()).expect("request XML");
                let config = request_body
                    .split_once("?>")
                    .map_or(request_body.as_str(), |(_, body)| body);
                ResponseTemplate::new(200)
                    .insert_header("etag", "E3ACCEPTED")
                    .set_body_string(format!(
                        r#"<?xml version="1.0" encoding="UTF-8"?><Distribution xmlns="http://cloudfront.amazonaws.com/doc/2020-05-31/">
<Id>E123EXAMPLE</Id><ARN>arn:aws:cloudfront::123456789012:distribution/E123EXAMPLE</ARN>
<Status>InProgress</Status><LastModifiedTime>2026-07-18T05:00:00Z</LastModifiedTime>
<InProgressInvalidationBatches>0</InProgressInvalidationBatches><DomainName>d123example.cloudfront.net</DomainName>
{config}</Distribution>"#
                    ))
            })
            .expect(1)
            .mount(&server)
            .await;

        let submission = api
            .update_distribution_once("E123EXAMPLE", "E2DETAIL", config)
            .await
            .expect("accepted update");
        assert_eq!(submission.distribution_id, "E123EXAMPLE");
        assert_eq!(submission.etag, "E3ACCEPTED");
        assert_eq!(submission.status, "InProgress");
        assert_ne!(submission.status, "Deployed");
    }

    #[tokio::test]
    async fn update_transport_conflict_requires_replan_and_never_retries() {
        let server = MockServer::start().await;
        let api = test_api(&server).await;
        Mock::given(method("PUT"))
            .and(path("/2020-05-31/distribution/E123EXAMPLE/config"))
            .respond_with(
                ResponseTemplate::new(412)
                    .set_body_string(provider_error_xml("PreconditionFailed")),
            )
            .expect(1)
            .mount(&server)
            .await;

        let Err(error) = api
            .update_distribution_once("E123EXAMPLE", "E2DETAIL", minimal_config(false))
            .await
        else {
            panic!("stale ETag must fail")
        };
        assert_eq!(
            error.category(),
            edgion_center_core::ProviderErrorCategory::Conflict
        );
        assert_eq!(error.code(), "cloudfront_update_requires_replan");
    }

    #[tokio::test]
    async fn update_transport_server_error_is_unknown_and_never_retries() {
        let server = MockServer::start().await;
        let api = test_api(&server).await;
        Mock::given(method("PUT"))
            .and(path("/2020-05-31/distribution/E123EXAMPLE/config"))
            .respond_with(
                ResponseTemplate::new(500).set_body_string(provider_error_xml("InternalFailure")),
            )
            .expect(1)
            .mount(&server)
            .await;

        let Err(error) = api
            .update_distribution_once("E123EXAMPLE", "E2DETAIL", minimal_config(false))
            .await
        else {
            panic!("ambiguous write must not retry")
        };
        assert_eq!(
            error.category(),
            edgion_center_core::ProviderErrorCategory::UnknownOutcome
        );
        assert_eq!(error.code(), "cloudfront_update_outcome_unknown");
    }

    #[tokio::test]
    async fn incomplete_update_success_is_unknown_outcome() {
        let server = MockServer::start().await;
        let api = test_api(&server).await;
        Mock::given(method("PUT"))
            .and(path("/2020-05-31/distribution/E123EXAMPLE/config"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("etag", "E3ACCEPTED")
                    .set_body_string("<Distribution></Distribution>"),
            )
            .expect(1)
            .mount(&server)
            .await;

        let Err(error) = api
            .update_distribution_once("E123EXAMPLE", "E2DETAIL", minimal_config(false))
            .await
        else {
            panic!("malformed success is ambiguous")
        };
        assert_eq!(
            error.category(),
            edgion_center_core::ProviderErrorCategory::UnknownOutcome
        );
    }

    #[tokio::test]
    async fn mismatched_update_config_is_unknown_outcome() {
        let server = MockServer::start().await;
        let api = test_api(&server).await;
        Mock::given(method("PUT"))
            .and(path("/2020-05-31/distribution/E123EXAMPLE/config"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("etag", "E3ACCEPTED")
                    .set_body_string(update_distribution_xml("InProgress", true)),
            )
            .expect(1)
            .mount(&server)
            .await;

        let Err(error) = api
            .update_distribution_once("E123EXAMPLE", "E2DETAIL", minimal_config(false))
            .await
        else {
            panic!("mismatched config is ambiguous")
        };
        assert_eq!(
            error.category(),
            edgion_center_core::ProviderErrorCategory::UnknownOutcome
        );
        assert!(error.code().starts_with("cloudfront_update_config_"));
    }

    async fn test_api(server: &MockServer) -> AwsCloudFrontApi {
        Mock::given(method("POST"))
            .and(path("/"))
            .and(body_string_contains("Action=GetCallerIdentity"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<GetCallerIdentityResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
<GetCallerIdentityResult><Arn>arn:aws:iam::123456789012:role/test</Arn>
<UserId>test</UserId><Account>123456789012</Account></GetCallerIdentityResult>
<ResponseMetadata><RequestId>test</RequestId></ResponseMetadata></GetCallerIdentityResponse>"#,
            ))
            .expect(1)
            .mount(server)
            .await;
        let sdk_config = aws_config::defaults(BehaviorVersion::latest())
            .credentials_provider(Credentials::new("key", "secret", None, None, "test"))
            .region(Region::new("us-east-1"))
            .load()
            .await;
        AwsCloudFrontApi::with_options(
            &sdk_config,
            crate::CloudFrontFingerprintKey::new([5; 32]).expect("key"),
            "credential-3",
            crate::AwsCloudFrontApiOptions {
                acm_endpoint_url: Some(server.uri()),
                cloudfront_endpoint_url: Some(server.uri()),
                sts_endpoint_url: Some(server.uri()),
            },
        )
        .await
        .expect("api")
    }

    fn update_distribution_xml(status: &str, enabled: bool) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><Distribution xmlns="http://cloudfront.amazonaws.com/doc/2020-05-31/">
<Id>E123EXAMPLE</Id><ARN>arn:aws:cloudfront::123456789012:distribution/E123EXAMPLE</ARN>
<Status>{status}</Status><LastModifiedTime>2026-07-18T05:00:00Z</LastModifiedTime>
<InProgressInvalidationBatches>0</InProgressInvalidationBatches><DomainName>d123example.cloudfront.net</DomainName>
<DistributionConfig><CallerReference>caller-reference</CallerReference><Comment>test</Comment>
<Enabled>{enabled}</Enabled></DistributionConfig></Distribution>"#
        )
    }

    fn provider_error_xml(code: &str) -> String {
        format!(
            r#"<?xml version="1.0"?><ErrorResponse><Error><Type>Sender</Type><Code>{code}</Code>
<Message>sanitized test error</Message></Error><RequestId>test</RequestId></ErrorResponse>"#
        )
    }

    fn raw_config_xml(secret: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><DistributionConfig xmlns="http://cloudfront.amazonaws.com/doc/2020-05-31/">
<CallerReference>caller-reference</CallerReference><Aliases><Quantity>0</Quantity><Items></Items></Aliases>
<DefaultRootObject></DefaultRootObject><Origins><Quantity>1</Quantity><Items><Origin>
<Id>origin-1</Id><DomainName>origin.example.test</DomainName><OriginPath></OriginPath>
<CustomHeaders><Quantity>1</Quantity><Items><OriginCustomHeader><HeaderName>X-Secret-Name-SENTINEL</HeaderName>
<HeaderValue>{secret}</HeaderValue></OriginCustomHeader></Items></CustomHeaders>
<CustomOriginConfig><HTTPPort>80</HTTPPort><HTTPSPort>443</HTTPSPort><OriginProtocolPolicy>https-only</OriginProtocolPolicy>
<OriginSslProtocols><Quantity>1</Quantity><Items><SslProtocol>TLSv1.2</SslProtocol></Items></OriginSslProtocols>
<OriginReadTimeout>30</OriginReadTimeout><OriginKeepaliveTimeout>5</OriginKeepaliveTimeout></CustomOriginConfig>
<ConnectionAttempts>3</ConnectionAttempts><ConnectionTimeout>10</ConnectionTimeout></Origin></Items></Origins>
<OriginGroups><Quantity>0</Quantity><Items></Items></OriginGroups><DefaultCacheBehavior><TargetOriginId>origin-1</TargetOriginId>
<TrustedSigners><Enabled>false</Enabled><Quantity>0</Quantity><Items></Items></TrustedSigners>
<TrustedKeyGroups><Enabled>false</Enabled><Quantity>0</Quantity><Items></Items></TrustedKeyGroups>
<ViewerProtocolPolicy>redirect-to-https</ViewerProtocolPolicy><AllowedMethods><Quantity>2</Quantity><Items><Method>GET</Method><Method>HEAD</Method></Items>
<CachedMethods><Quantity>2</Quantity><Items><Method>GET</Method><Method>HEAD</Method></Items></CachedMethods></AllowedMethods>
<SmoothStreaming>false</SmoothStreaming><Compress>true</Compress><LambdaFunctionAssociations><Quantity>0</Quantity><Items></Items></LambdaFunctionAssociations>
<FunctionAssociations><Quantity>0</Quantity><Items></Items></FunctionAssociations><CachePolicyId>managed-cache-policy</CachePolicyId></DefaultCacheBehavior>
<CacheBehaviors><Quantity>0</Quantity><Items></Items></CacheBehaviors><CustomErrorResponses><Quantity>0</Quantity><Items></Items></CustomErrorResponses>
<Comment></Comment><Logging><Enabled>false</Enabled><IncludeCookies>false</IncludeCookies><Bucket></Bucket><Prefix></Prefix></Logging>
<PriceClass>PriceClass_All</PriceClass><Enabled>true</Enabled><ViewerCertificate><CloudFrontDefaultCertificate>true</CloudFrontDefaultCertificate>
<MinimumProtocolVersion>TLSv1</MinimumProtocolVersion></ViewerCertificate><Restrictions><GeoRestriction><RestrictionType>none</RestrictionType>
<Quantity>0</Quantity><Items></Items></GeoRestriction></Restrictions><WebACLId></WebACLId><HttpVersion>http2</HttpVersion>
<IsIPV6Enabled>true</IsIPV6Enabled><Staging>false</Staging></DistributionConfig>"#
        )
    }
}
