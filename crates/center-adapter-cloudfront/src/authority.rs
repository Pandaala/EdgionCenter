use std::collections::BTreeSet;

use async_trait::async_trait;
use edgion_center_core::{CloudResourceId, ManagementPolicy};
use serde::{Deserialize, Serialize};

use crate::{
    mutations::cloudfront_enablement_infrastructure_blockers, validation, AwsPartition,
    CloudFrontApiResult, CloudFrontEnablementAction, CloudFrontEnablementMutationPlan,
    CloudFrontEnablementRisk, CloudFrontOwnershipHint, CloudFrontPlanningInventory,
};

const MAX_REVISION_LEN: usize = 512;
const MAX_ACTOR_LEN: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CloudFrontOwnershipBasis {
    Created {
        creation_operation_id: String,
    },
    AdoptedActive {
        adoption_plan_revision: String,
        adoption_approval_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontOwnershipClaim {
    center_resource_id: CloudResourceId,
    provider_account_id: CloudResourceId,
    account_generation: u64,
    credential_revision: String,
    partition: AwsPartition,
    aws_account_id: String,
    distribution_id: String,
    distribution_arn: String,
    ownership_revision: String,
    management_policy: ManagementPolicy,
    basis: CloudFrontOwnershipBasis,
    observed_at_unix_ms: i64,
    valid_until_unix_ms: i64,
}

impl CloudFrontOwnershipClaim {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        center_resource_id: CloudResourceId,
        provider_account_id: CloudResourceId,
        account_generation: u64,
        credential_revision: impl Into<String>,
        partition: AwsPartition,
        aws_account_id: impl Into<String>,
        distribution_id: impl Into<String>,
        distribution_arn: impl Into<String>,
        ownership_revision: impl Into<String>,
        management_policy: ManagementPolicy,
        basis: CloudFrontOwnershipBasis,
        observed_at_unix_ms: i64,
        valid_until_unix_ms: i64,
    ) -> CloudFrontApiResult<Self> {
        let claim = Self {
            center_resource_id,
            provider_account_id,
            account_generation,
            credential_revision: credential_revision.into(),
            partition,
            aws_account_id: aws_account_id.into(),
            distribution_id: distribution_id.into(),
            distribution_arn: distribution_arn.into(),
            ownership_revision: ownership_revision.into(),
            management_policy,
            basis,
            observed_at_unix_ms,
            valid_until_unix_ms,
        };
        claim.validate_shape()?;
        Ok(claim)
    }

    pub fn center_resource_id(&self) -> &CloudResourceId {
        &self.center_resource_id
    }
    pub fn provider_account_id(&self) -> &CloudResourceId {
        &self.provider_account_id
    }
    pub fn account_generation(&self) -> u64 {
        self.account_generation
    }
    pub fn credential_revision(&self) -> &str {
        &self.credential_revision
    }
    pub fn partition(&self) -> AwsPartition {
        self.partition
    }
    pub fn aws_account_id(&self) -> &str {
        &self.aws_account_id
    }
    pub fn distribution_id(&self) -> &str {
        &self.distribution_id
    }
    pub fn distribution_arn(&self) -> &str {
        &self.distribution_arn
    }
    pub fn ownership_revision(&self) -> &str {
        &self.ownership_revision
    }
    pub fn management_policy(&self) -> ManagementPolicy {
        self.management_policy
    }
    pub fn basis(&self) -> &CloudFrontOwnershipBasis {
        &self.basis
    }
    pub fn observed_at_unix_ms(&self) -> i64 {
        self.observed_at_unix_ms
    }
    pub fn valid_until_unix_ms(&self) -> i64 {
        self.valid_until_unix_ms
    }

    fn validate_shape(&self) -> CloudFrontApiResult<()> {
        self.center_resource_id
            .validate()
            .map_err(|_| validation("invalid_cloudfront_ownership_center_resource"))?;
        self.provider_account_id
            .validate()
            .map_err(|_| validation("invalid_cloudfront_ownership_provider_account"))?;
        validate_revision(
            &self.credential_revision,
            "invalid_cloudfront_ownership_credential",
        )?;
        validate_revision(
            &self.ownership_revision,
            "invalid_cloudfront_ownership_revision",
        )?;
        validate_distribution_scope(
            self.partition,
            &self.aws_account_id,
            &self.distribution_id,
            &self.distribution_arn,
        )?;
        match &self.basis {
            CloudFrontOwnershipBasis::Created {
                creation_operation_id,
            } => validate_revision(
                creation_operation_id,
                "invalid_cloudfront_creation_operation",
            )?,
            CloudFrontOwnershipBasis::AdoptedActive {
                adoption_plan_revision,
                adoption_approval_id,
            } => {
                validate_revision(
                    adoption_plan_revision,
                    "invalid_cloudfront_adoption_plan_revision",
                )?;
                validate_revision(adoption_approval_id, "invalid_cloudfront_adoption_approval")?;
            }
        }
        if self.account_generation == 0
            || self.management_policy != ManagementPolicy::Managed
            || self.observed_at_unix_ms <= 0
            || self.valid_until_unix_ms <= self.observed_at_unix_ms
        {
            return Err(validation("invalid_cloudfront_ownership_claim"));
        }
        Ok(())
    }

    fn validate_at(&self, now_unix_ms: i64) -> CloudFrontApiResult<()> {
        self.validate_shape()?;
        if now_unix_ms < self.observed_at_unix_ms || now_unix_ms >= self.valid_until_unix_ms {
            return Err(validation("cloudfront_ownership_claim_expired"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudFrontEnablementAcknowledgement {
    TrafficInterruption,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontApprovalConsumptionRef {
    operation_id: String,
    consumption_revision: String,
}

impl CloudFrontApprovalConsumptionRef {
    pub fn new(
        operation_id: impl Into<String>,
        consumption_revision: impl Into<String>,
    ) -> CloudFrontApiResult<Self> {
        let value = Self {
            operation_id: operation_id.into(),
            consumption_revision: consumption_revision.into(),
        };
        validate_revision(&value.operation_id, "invalid_cloudfront_approval_operation")?;
        validate_revision(
            &value.consumption_revision,
            "invalid_cloudfront_approval_consumption_revision",
        )?;
        Ok(value)
    }

    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }
    pub fn consumption_revision(&self) -> &str {
        &self.consumption_revision
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontEnablementApprovalRecord {
    approval_id: String,
    approval_revision: String,
    plan_revision: String,
    center_resource_id: CloudResourceId,
    ownership_revision: String,
    action: CloudFrontEnablementAction,
    risk: CloudFrontEnablementRisk,
    acknowledgements: BTreeSet<CloudFrontEnablementAcknowledgement>,
    approved_by: String,
    policy_revision: String,
    approved_at_unix_ms: i64,
    valid_until_unix_ms: i64,
    consumption: Option<CloudFrontApprovalConsumptionRef>,
}

impl CloudFrontEnablementApprovalRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        approval_id: impl Into<String>,
        approval_revision: impl Into<String>,
        plan_revision: impl Into<String>,
        center_resource_id: CloudResourceId,
        ownership_revision: impl Into<String>,
        action: CloudFrontEnablementAction,
        risk: CloudFrontEnablementRisk,
        acknowledgements: BTreeSet<CloudFrontEnablementAcknowledgement>,
        approved_by: impl Into<String>,
        policy_revision: impl Into<String>,
        approved_at_unix_ms: i64,
        valid_until_unix_ms: i64,
        consumption: Option<CloudFrontApprovalConsumptionRef>,
    ) -> CloudFrontApiResult<Self> {
        let record = Self {
            approval_id: approval_id.into(),
            approval_revision: approval_revision.into(),
            plan_revision: plan_revision.into(),
            center_resource_id,
            ownership_revision: ownership_revision.into(),
            action,
            risk,
            acknowledgements,
            approved_by: approved_by.into(),
            policy_revision: policy_revision.into(),
            approved_at_unix_ms,
            valid_until_unix_ms,
            consumption,
        };
        record.validate_shape()?;
        Ok(record)
    }

    pub fn approval_id(&self) -> &str {
        &self.approval_id
    }
    pub fn approval_revision(&self) -> &str {
        &self.approval_revision
    }
    pub fn plan_revision(&self) -> &str {
        &self.plan_revision
    }
    pub fn center_resource_id(&self) -> &CloudResourceId {
        &self.center_resource_id
    }
    pub fn ownership_revision(&self) -> &str {
        &self.ownership_revision
    }
    pub fn action(&self) -> CloudFrontEnablementAction {
        self.action
    }
    pub fn risk(&self) -> CloudFrontEnablementRisk {
        self.risk
    }
    pub fn acknowledgements(&self) -> &BTreeSet<CloudFrontEnablementAcknowledgement> {
        &self.acknowledgements
    }
    pub fn approved_by(&self) -> &str {
        &self.approved_by
    }
    pub fn policy_revision(&self) -> &str {
        &self.policy_revision
    }
    pub fn approved_at_unix_ms(&self) -> i64 {
        self.approved_at_unix_ms
    }
    pub fn valid_until_unix_ms(&self) -> i64 {
        self.valid_until_unix_ms
    }
    pub fn consumption(&self) -> Option<&CloudFrontApprovalConsumptionRef> {
        self.consumption.as_ref()
    }

    fn validate_shape(&self) -> CloudFrontApiResult<()> {
        for (value, code) in [
            (&self.approval_id, "invalid_cloudfront_approval_id"),
            (
                &self.approval_revision,
                "invalid_cloudfront_approval_revision",
            ),
            (
                &self.plan_revision,
                "invalid_cloudfront_approval_plan_revision",
            ),
            (
                &self.policy_revision,
                "invalid_cloudfront_approval_policy_revision",
            ),
            (
                &self.ownership_revision,
                "invalid_cloudfront_approval_ownership_revision",
            ),
        ] {
            validate_revision(value, code)?;
        }
        self.center_resource_id
            .validate()
            .map_err(|_| validation("invalid_cloudfront_approval_center_resource"))?;
        if self.plan_revision.len() != 64
            || !self
                .plan_revision
                .bytes()
                .all(|value| value.is_ascii_hexdigit())
            || self.approved_by.is_empty()
            || self.approved_by.len() > MAX_ACTOR_LEN
            || self.approved_by.trim() != self.approved_by
            || self.approved_by.chars().any(char::is_control)
            || self.approved_at_unix_ms <= 0
            || self.valid_until_unix_ms <= self.approved_at_unix_ms
        {
            return Err(validation("invalid_cloudfront_enablement_approval"));
        }
        let expected = match self.action {
            CloudFrontEnablementAction::Enable => BTreeSet::new(),
            CloudFrontEnablementAction::Disable => {
                BTreeSet::from([CloudFrontEnablementAcknowledgement::TrafficInterruption])
            }
        };
        if self.acknowledgements != expected
            || (self.action == CloudFrontEnablementAction::Enable
                && self.risk != CloudFrontEnablementRisk::TrafficRestoring)
            || (self.action == CloudFrontEnablementAction::Disable
                && self.risk != CloudFrontEnablementRisk::TrafficStopping)
        {
            return Err(validation(
                "invalid_cloudfront_approval_risk_acknowledgement",
            ));
        }
        Ok(())
    }

    fn validate_at(&self, now_unix_ms: i64) -> CloudFrontApiResult<()> {
        self.validate_shape()?;
        if now_unix_ms < self.approved_at_unix_ms || now_unix_ms >= self.valid_until_unix_ms {
            return Err(validation("cloudfront_enablement_approval_expired"));
        }
        if self.consumption.is_some() {
            return Err(validation(
                "cloudfront_enablement_approval_already_consumed",
            ));
        }
        Ok(())
    }
}

#[async_trait]
pub trait CloudFrontEnablementPreauthorizationVerifier: Send + Sync {
    /// Verifies ownership and approval from one authoritative snapshot or transaction.
    async fn verify_joint_current(
        &self,
        ownership: &CloudFrontOwnershipClaim,
        approval: &CloudFrontEnablementApprovalRecord,
    ) -> CloudFrontApiResult<()>;
}

pub trait CloudFrontAuthorityClock: Send + Sync {
    fn now_unix_ms(&self) -> CloudFrontApiResult<i64>;
}

/// Opaque preauthorization only. It cannot be serialized, cloned, debugged, or dispatched.
pub struct CloudFrontEnablementPreauthorization {
    center_resource_id: CloudResourceId,
    inventory_observation_token: String,
    plan_revision: String,
    ownership_revision: String,
    approval_id: String,
    approval_revision: String,
    valid_until_unix_ms: i64,
}

impl CloudFrontEnablementPreauthorization {
    pub fn center_resource_id(&self) -> &CloudResourceId {
        &self.center_resource_id
    }
    pub fn plan_revision(&self) -> &str {
        &self.plan_revision
    }
    pub fn inventory_observation_token(&self) -> &str {
        &self.inventory_observation_token
    }
    pub fn ownership_revision(&self) -> &str {
        &self.ownership_revision
    }
    pub fn approval_revision(&self) -> &str {
        &self.approval_revision
    }
    pub fn approval_id(&self) -> &str {
        &self.approval_id
    }
    pub fn valid_until_unix_ms(&self) -> i64 {
        self.valid_until_unix_ms
    }
}

pub async fn preauthorize_cloudfront_enablement<V, C>(
    inventory: &CloudFrontPlanningInventory,
    plan: &CloudFrontEnablementMutationPlan,
    ownership: CloudFrontOwnershipClaim,
    approval: CloudFrontEnablementApprovalRecord,
    verifier: &V,
    clock: &C,
) -> CloudFrontApiResult<CloudFrontEnablementPreauthorization>
where
    V: CloudFrontEnablementPreauthorizationVerifier,
    C: CloudFrontAuthorityClock,
{
    validate_preauthorization(inventory, plan, &ownership, &approval, clock.now_unix_ms()?)?;
    verifier.verify_joint_current(&ownership, &approval).await?;
    validate_preauthorization(inventory, plan, &ownership, &approval, clock.now_unix_ms()?)?;
    let inventory_authority = &inventory.inventory().authority;
    Ok(CloudFrontEnablementPreauthorization {
        center_resource_id: ownership.center_resource_id.clone(),
        inventory_observation_token: inventory_authority.observation_token().to_string(),
        plan_revision: plan.plan_revision().as_str().to_string(),
        ownership_revision: ownership.ownership_revision.clone(),
        approval_id: approval.approval_id.clone(),
        approval_revision: approval.approval_revision.clone(),
        valid_until_unix_ms: plan
            .valid_until_unix_ms()
            .min(ownership.valid_until_unix_ms)
            .min(approval.valid_until_unix_ms)
            .min(inventory_authority.valid_until_unix_ms()),
    })
}

fn validate_preauthorization(
    inventory: &CloudFrontPlanningInventory,
    plan: &CloudFrontEnablementMutationPlan,
    ownership: &CloudFrontOwnershipClaim,
    approval: &CloudFrontEnablementApprovalRecord,
    now_unix_ms: i64,
) -> CloudFrontApiResult<()> {
    ownership.validate_at(now_unix_ms)?;
    approval.validate_at(now_unix_ms)?;
    if plan.dispatch_blockers() != &cloudfront_enablement_infrastructure_blockers()
        || plan.current_enabled() == plan.desired_enabled()
        || plan.write_set() != &BTreeSet::from(["enabled".to_string()])
    {
        return Err(validation("cloudfront_enablement_plan_not_preauthorizable"));
    }
    if now_unix_ms >= plan.valid_until_unix_ms()
        || ownership.provider_account_id() != plan.provider_account_id()
        || ownership.account_generation() != plan.account_generation()
        || ownership.credential_revision() != plan.credential_revision()
        || ownership.partition() != plan.partition()
        || ownership.aws_account_id() != plan.aws_account_id()
        || ownership.distribution_id() != plan.distribution_id()
        || ownership.distribution_arn() != plan.distribution_arn()
        || approval.plan_revision() != plan.plan_revision().as_str()
        || approval.center_resource_id() != ownership.center_resource_id()
        || approval.ownership_revision() != ownership.ownership_revision()
        || approval.action() != plan.action()
        || approval.risk() != plan.risk()
    {
        return Err(validation("cloudfront_enablement_authority_scope_mismatch"));
    }
    let inventory = inventory.inventory();
    if inventory.authority.provider_account_id() != ownership.provider_account_id()
        || inventory.authority.account_generation() != ownership.account_generation()
        || inventory.authority.credential_revision() != ownership.credential_revision()
        || inventory.authority.partition() != ownership.partition()
        || inventory.authority.aws_account_id() != ownership.aws_account_id()
        || !inventory.authority.is_fresh_at(now_unix_ms)
    {
        return Err(validation("cloudfront_enablement_inventory_scope_mismatch"));
    }
    let entry = inventory
        .entries
        .iter()
        .find(|entry| entry.summary.id == ownership.distribution_id())
        .ok_or_else(|| validation("cloudfront_enablement_ownership_hint_missing"))?;
    match &entry.ownership_hint {
        CloudFrontOwnershipHint::Present { center_resource_id }
            if center_resource_id == ownership.center_resource_id() => {}
        _ => return Err(validation("cloudfront_enablement_ownership_hint_mismatch")),
    }
    Ok(())
}

fn validate_revision(value: &str, code: &str) -> CloudFrontApiResult<()> {
    if value.is_empty()
        || value.len() > MAX_REVISION_LEN
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(validation(code));
    }
    Ok(())
}

fn validate_distribution_scope(
    partition: AwsPartition,
    account_id: &str,
    distribution_id: &str,
    arn: &str,
) -> CloudFrontApiResult<()> {
    if account_id.len() != 12
        || !account_id.bytes().all(|value| value.is_ascii_digit())
        || distribution_id.is_empty()
        || distribution_id.len() > 128
        || !distribution_id
            .bytes()
            .all(|value| value.is_ascii_uppercase() || value.is_ascii_digit())
        || arn
            != format!(
                "arn:{}:cloudfront::{account_id}:distribution/{distribution_id}",
                partition.arn_partition()
            )
    {
        return Err(validation(
            "invalid_cloudfront_ownership_distribution_scope",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Mutex};

    use edgion_center_core::NormalizedProviderError;

    use super::*;
    use crate::{
        CloudFrontDetailObservation, CloudFrontInventory, CloudFrontInventoryEntry,
        CloudFrontMutationEligibility, ObservedCloudFrontDistributionDetail,
    };

    struct FakeVerifier {
        denied: bool,
    }

    #[async_trait]
    impl CloudFrontEnablementPreauthorizationVerifier for FakeVerifier {
        async fn verify_joint_current(
            &self,
            _: &CloudFrontOwnershipClaim,
            _: &CloudFrontEnablementApprovalRecord,
        ) -> CloudFrontApiResult<()> {
            if self.denied {
                Err(validation("approval_denied"))
            } else {
                Ok(())
            }
        }
    }

    struct SequenceClock(Mutex<VecDeque<i64>>);

    impl SequenceClock {
        fn fixed(value: i64) -> Self {
            Self(Mutex::new(VecDeque::from([value, value])))
        }
    }

    impl CloudFrontAuthorityClock for SequenceClock {
        fn now_unix_ms(&self) -> CloudFrontApiResult<i64> {
            self.0
                .lock()
                .map_err(|_| validation("clock_unavailable"))?
                .pop_front()
                .ok_or_else(|| validation("clock_exhausted"))
        }
    }

    fn inventory(hint: CloudFrontOwnershipHint) -> CloudFrontPlanningInventory {
        let summary = crate::tests::summary();
        let detail = crate::tests::detail(summary.clone());
        CloudFrontPlanningInventory(CloudFrontInventory {
            authority: crate::CloudFrontObservationAuthority::new(
                CloudResourceId::new("aws-main").unwrap(),
                "123456789012".to_string(),
                AwsPartition::Aws,
                7,
                "credential-3".to_string(),
                "observation-1".to_string(),
                1_000,
                1_750,
            )
            .unwrap(),
            entries: vec![CloudFrontInventoryEntry {
                summary,
                detail: CloudFrontDetailObservation::Complete(Box::new(
                    ObservedCloudFrontDistributionDetail {
                        detail,
                        mutation_eligibility: CloudFrontMutationEligibility::Eligible,
                        changed_since_summary: false,
                    },
                )),
                ownership_hint: hint,
            }],
        })
    }

    fn ownership(basis: CloudFrontOwnershipBasis) -> CloudFrontOwnershipClaim {
        CloudFrontOwnershipClaim::new(
            CloudResourceId::new("edge-app-1").unwrap(),
            CloudResourceId::new("aws-main").unwrap(),
            7,
            "credential-3",
            AwsPartition::Aws,
            "123456789012",
            "E123EXAMPLE",
            "arn:aws:cloudfront::123456789012:distribution/E123EXAMPLE",
            "ownership-4",
            ManagementPolicy::Managed,
            basis,
            1_100,
            1_900,
        )
        .unwrap()
    }

    fn created() -> CloudFrontOwnershipClaim {
        ownership(CloudFrontOwnershipBasis::Created {
            creation_operation_id: "operation-create-1".to_string(),
        })
    }

    fn approval(plan: &CloudFrontEnablementMutationPlan) -> CloudFrontEnablementApprovalRecord {
        let acknowledgements = match plan.action() {
            CloudFrontEnablementAction::Enable => BTreeSet::new(),
            CloudFrontEnablementAction::Disable => {
                BTreeSet::from([CloudFrontEnablementAcknowledgement::TrafficInterruption])
            }
        };
        CloudFrontEnablementApprovalRecord::new(
            "approval-1",
            "approval-revision-2",
            plan.plan_revision().as_str(),
            CloudResourceId::new("edge-app-1").unwrap(),
            "ownership-4",
            plan.action(),
            plan.risk(),
            acknowledgements,
            "operator@example.com",
            "approval-policy-3",
            1_200,
            1_800,
            None,
        )
        .unwrap()
    }

    fn matching_inventory() -> CloudFrontPlanningInventory {
        inventory(CloudFrontOwnershipHint::Present {
            center_resource_id: CloudResourceId::new("edge-app-1").unwrap(),
        })
    }

    fn plan(current_enabled: bool, desired_enabled: bool) -> CloudFrontEnablementMutationPlan {
        CloudFrontEnablementMutationPlan::test_value(
            current_enabled,
            desired_enabled,
            cloudfront_enablement_infrastructure_blockers(),
        )
    }

    async fn authorize(
        plan: &CloudFrontEnablementMutationPlan,
        ownership: CloudFrontOwnershipClaim,
        approval: CloudFrontEnablementApprovalRecord,
        verifier: &FakeVerifier,
        clock: &SequenceClock,
    ) -> Result<CloudFrontEnablementPreauthorization, NormalizedProviderError> {
        preauthorize_cloudfront_enablement(
            &matching_inventory(),
            plan,
            ownership,
            approval,
            verifier,
            clock,
        )
        .await
    }

    #[tokio::test]
    async fn created_and_active_adopted_claims_can_be_preauthorized() {
        let verifier = FakeVerifier { denied: false };
        for basis in [
            CloudFrontOwnershipBasis::Created {
                creation_operation_id: "operation-create-1".to_string(),
            },
            CloudFrontOwnershipBasis::AdoptedActive {
                adoption_plan_revision: "adoption-plan-1".to_string(),
                adoption_approval_id: "adoption-approval-1".to_string(),
            },
        ] {
            let plan = plan(false, true);
            let authority = authorize(
                &plan,
                ownership(basis),
                approval(&plan),
                &verifier,
                &SequenceClock::fixed(1_500),
            )
            .await
            .unwrap();
            assert_eq!(authority.center_resource_id().as_str(), "edge-app-1");
            assert_eq!(authority.plan_revision(), plan.plan_revision().as_str());
            assert_eq!(authority.inventory_observation_token(), "observation-1");
            assert_eq!(authority.approval_id(), "approval-1");
            assert_eq!(authority.valid_until_unix_ms(), 1_750);
        }
    }

    #[tokio::test]
    async fn scope_action_revision_expiry_and_hint_mismatches_fail_closed() {
        let verifier = FakeVerifier { denied: false };
        let plan = plan(false, true);

        let mut wrong_scope = created();
        wrong_scope.account_generation = 8;
        assert!(authorize(
            &plan,
            wrong_scope,
            approval(&plan),
            &verifier,
            &SequenceClock::fixed(1_500)
        )
        .await
        .is_err());

        let mut wrong_approval = approval(&plan);
        wrong_approval.plan_revision = "d".repeat(64);
        assert!(authorize(
            &plan,
            created(),
            wrong_approval,
            &verifier,
            &SequenceClock::fixed(1_500)
        )
        .await
        .is_err());

        let mut stale_ownership_approval = approval(&plan);
        stale_ownership_approval.ownership_revision = "ownership-3".to_string();
        assert!(authorize(
            &plan,
            created(),
            stale_ownership_approval,
            &verifier,
            &SequenceClock::fixed(1_500)
        )
        .await
        .is_err());

        assert!(authorize(
            &plan,
            created(),
            approval(&plan),
            &verifier,
            &SequenceClock::fixed(1_900)
        )
        .await
        .is_err());

        let result = preauthorize_cloudfront_enablement(
            &inventory(CloudFrontOwnershipHint::Absent),
            &plan,
            created(),
            approval(&plan),
            &verifier,
            &SequenceClock::fixed(1_500),
        )
        .await;
        let Err(error) = result else {
            panic!("absent ownership hint must fail")
        };
        assert_eq!(
            error.code(),
            "cloudfront_enablement_ownership_hint_mismatch"
        );
    }

    #[tokio::test]
    async fn verifier_rejection_and_async_expiry_fail_closed() {
        let plan = plan(false, true);
        assert!(authorize(
            &plan,
            created(),
            approval(&plan),
            &FakeVerifier { denied: true },
            &SequenceClock::fixed(1_500)
        )
        .await
        .is_err());
        assert!(authorize(
            &plan,
            created(),
            approval(&plan),
            &FakeVerifier { denied: false },
            &SequenceClock(Mutex::new(VecDeque::from([1_500, 1_800])))
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn no_op_or_provider_specific_blocker_cannot_be_preauthorized() {
        let verifier = FakeVerifier { denied: false };
        for plan in [
            plan(true, true),
            CloudFrontEnablementMutationPlan::test_value(false, true, {
                let mut blockers = cloudfront_enablement_infrastructure_blockers();
                blockers.insert("staging_distribution".to_string());
                blockers
            }),
        ] {
            assert!(authorize(
                &plan,
                created(),
                approval(&plan),
                &verifier,
                &SequenceClock::fixed(1_500)
            )
            .await
            .is_err());
        }
    }

    #[test]
    fn disable_requires_exact_traffic_interruption_acknowledgement() {
        let plan = plan(true, false);
        assert!(CloudFrontEnablementApprovalRecord::new(
            "approval-1",
            "approval-revision-2",
            plan.plan_revision().as_str(),
            CloudResourceId::new("edge-app-1").unwrap(),
            "ownership-4",
            plan.action(),
            plan.risk(),
            BTreeSet::new(),
            "operator@example.com",
            "policy-1",
            1_200,
            1_800,
            None,
        )
        .is_err());
    }

    #[test]
    fn persisted_records_are_sanitized_and_consumption_is_future_bound() {
        let plan = plan(false, true);
        let claim = created();
        let mut record = approval(&plan);
        record.consumption =
            Some(CloudFrontApprovalConsumptionRef::new("operation-9", "consumption-3").unwrap());
        let json = serde_json::to_string(&(claim, record)).unwrap();
        assert!(!json.contains("VALUE-SENTINEL"));
        assert!(!json.contains("X-Secret-Name-SENTINEL"));
        assert!(!json.contains("<DistributionConfig"));
        assert!(json.contains("operation-9"));
    }
}
