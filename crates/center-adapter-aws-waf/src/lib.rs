//! Bounded Amazon WAFv2 adapter.
//!
//! This crate is independent of Center persistence, binaries, Admin API, and
//! CloudFront. It exposes typed Web ACL operations only; raw WAF statement or
//! action JSON has no route through this public contract.

mod api;
mod aws_sdk;
mod model;

pub use api::{AwsWafApi, AwsWafApiResult};
pub use aws_sdk::{AwsWafSdkApi, AwsWafSdkApiOptions};
pub use model::*;

use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use edgion_center_core::{
    CloudProvider, CloudResourceId, ProviderAccountScope, ProviderAccountSpec,
    ProviderErrorCategory,
};

const PAGE_SIZE: u16 = 100;

/// Per-request evidence of whether a provider mutation may have been sent.
/// Composition uses it to distinguish safe pre-dispatch failures from an
/// ambiguous result after a deadline or authority change.
#[derive(Clone, Default)]
pub struct AwsWafMutationDispatch(Arc<AtomicBool>);

impl AwsWafMutationDispatch {
    pub fn was_dispatched(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    fn mark_dispatched(&self) {
        self.0.store(true, Ordering::Release);
    }
}

/// Account-bound WAFv2 adapter. A composition root supplies a transport whose
/// identity was verified before construction.
pub struct AwsWafAdapter {
    provider_account_id: CloudResourceId,
    aws_account_id: String,
    account_generation: u64,
    credential_revision: String,
    api: Arc<dyn AwsWafApi>,
}

impl AwsWafAdapter {
    pub fn new(
        provider_account_id: CloudResourceId,
        account_generation: u64,
        account: &ProviderAccountSpec,
        api: Arc<dyn AwsWafApi>,
    ) -> AwsWafApiResult<Self> {
        provider_account_id
            .validate()
            .map_err(|_| validation("invalid_aws_waf_provider_account_id"))?;
        account
            .validate()
            .map_err(|_| validation("invalid_aws_waf_provider_account"))?;
        if account.provider != CloudProvider::Aws {
            return Err(validation("aws_waf_aws_provider_required"));
        }
        let ProviderAccountScope::Aws { account_id } = account
            .scope
            .as_ref()
            .ok_or_else(|| validation("aws_waf_account_scope_required"))?
        else {
            return Err(validation("aws_waf_account_scope_mismatch"));
        };
        if account_generation == 0
            || api.verified_account_id() != account_id
            || api.credential_revision().is_empty()
            || api.credential_revision().len() > 512
        {
            return Err(validation("aws_waf_verified_account_mismatch"));
        }
        Ok(Self {
            provider_account_id,
            aws_account_id: account_id.clone(),
            account_generation,
            credential_revision: api.credential_revision().to_string(),
            api,
        })
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

    pub async fn inventory(&self, scope: AwsWafScope) -> AwsWafApiResult<Vec<AwsWafWebAcl>> {
        scope.validate()?;
        self.assert_authority()?;
        let mut result = Vec::new();
        let mut marker = None;
        let mut seen = BTreeSet::new();
        let mut seen_markers = BTreeSet::new();
        for _ in 0..MAX_WEB_ACLS {
            let page = self
                .api
                .list_web_acls(&scope, marker.as_deref(), PAGE_SIZE)
                .await?;
            for acl in page.items {
                self.validate_acl(&acl)?;
                if acl.scope != scope || !seen.insert(acl.id.clone()) {
                    return Err(conflict("aws_waf_web_acl_inventory_collision"));
                }
                result.push(acl);
                if result.len() > MAX_WEB_ACLS {
                    return Err(quota("aws_waf_web_acl_inventory_limit"));
                }
            }
            match page.next_marker {
                None => return Ok(result),
                Some(next)
                    if next.is_empty()
                        || next.len() > 1_024
                        || !seen_markers.insert(next.clone()) =>
                {
                    return Err(validation("invalid_aws_waf_inventory_marker"))
                }
                Some(next) => marker = Some(next),
            }
        }
        Err(validation("aws_waf_inventory_pagination_limit"))
    }

    pub async fn get_web_acl(
        &self,
        scope: AwsWafScope,
        id: &AwsWafWebAclId,
    ) -> AwsWafApiResult<Option<AwsWafWebAcl>> {
        scope.validate()?;
        self.assert_authority()?;
        let value = self.api.get_web_acl(&scope, id).await?;
        if let Some(value) = &value {
            self.validate_acl(value)?;
            if value.scope != scope || &value.id != id {
                return Err(unknown("aws_waf_web_acl_get_mismatch"));
            }
        }
        Ok(value)
    }

    pub async fn inventory_ip_sets(&self, scope: AwsWafScope) -> AwsWafApiResult<Vec<AwsWafIpSet>> {
        scope.validate()?;
        self.assert_authority()?;
        let mut marker = None;
        let mut seen = BTreeSet::new();
        let mut seen_markers = BTreeSet::new();
        let mut items = Vec::new();
        for _ in 0..MAX_WEB_ACLS {
            let page = self
                .api
                .list_ip_sets(&scope, marker.as_deref(), PAGE_SIZE)
                .await?;
            for value in page.items {
                value.validate(&self.aws_account_id)?;
                if value.scope != scope || !seen.insert(value.id.clone()) {
                    return Err(conflict("aws_waf_ip_set_inventory_collision"));
                }
                items.push(value);
            }
            match page.next_marker {
                None => return Ok(items),
                Some(next) if next.is_empty() || !seen_markers.insert(next.clone()) => {
                    return Err(validation("invalid_aws_waf_ip_set_marker"))
                }
                Some(next) => marker = Some(next),
            }
        }
        Err(validation("aws_waf_ip_set_pagination_limit"))
    }

    pub async fn get_ip_set(
        &self,
        scope: AwsWafScope,
        id: &AwsWafIpSetId,
    ) -> AwsWafApiResult<Option<AwsWafIpSet>> {
        scope.validate()?;
        self.assert_authority()?;
        let value = self.api.get_ip_set(&scope, id).await?;
        if let Some(value) = &value {
            value.validate(&self.aws_account_id)?;
            if value.scope != scope || &value.id != id {
                return Err(unknown("aws_waf_ip_set_get_mismatch"));
            }
        }
        Ok(value)
    }

    pub async fn list_associations(
        &self,
        scope: AwsWafScope,
        id: &AwsWafWebAclId,
    ) -> AwsWafApiResult<Vec<AwsWafAssociation>> {
        scope.validate()?;
        self.assert_authority()?;
        self.api.list_associations(&scope, id).await
    }

    /// Returns the provider's currently available managed rule groups and
    /// versions for the exact typed scope.
    pub async fn managed_rule_group_catalog(
        &self,
        scope: AwsWafScope,
    ) -> AwsWafApiResult<Vec<AwsWafManagedRuleGroupCatalogEntry>> {
        scope.validate()?;
        self.assert_authority()?;
        let values = self.api.list_managed_rule_groups(&scope).await?;
        if values.iter().any(|value| {
            !valid_catalog_identifier(&value.vendor_name)
                || !valid_catalog_identifier(&value.name)
                || value
                    .versions
                    .iter()
                    .any(|version| !valid_catalog_identifier(version))
        }) {
            return Err(unknown("aws_waf_managed_catalog_malformed"));
        }
        Ok(values)
    }

    /// Executes AWS WAF CheckCapacity and enforces the fixed composition
    /// ceiling. The caller must still validate catalog versions and generate
    /// Center ownership before a mutation is dispatched.
    pub async fn check_capacity(
        &self,
        scope: AwsWafScope,
        rules: &[AwsWafRule],
        account_wcu_ceiling: u32,
    ) -> AwsWafApiResult<AwsWafCapacityObservation> {
        scope.validate()?;
        self.assert_authority()?;
        if rules.len() > MAX_RULES {
            return Err(quota("aws_waf_rule_limit"));
        }
        let required_wcu = self.api.check_capacity(&scope, rules).await?;
        let observation = AwsWafCapacityObservation {
            scope,
            required_wcu,
            account_wcu_ceiling,
        };
        observation.authorize()?;
        Ok(observation)
    }

    pub async fn create_ip_set(&self, value: AwsWafIpSet) -> AwsWafApiResult<AwsWafIpSet> {
        self.create_ip_set_tracked(value, &AwsWafMutationDispatch::default())
            .await
    }

    pub async fn create_ip_set_tracked(
        &self,
        value: AwsWafIpSet,
        dispatch: &AwsWafMutationDispatch,
    ) -> AwsWafApiResult<AwsWafIpSet> {
        self.assert_authority()?;
        value.validate(&self.aws_account_id)?;
        dispatch.mark_dispatched();
        let created = self
            .api
            .create_ip_set(&value)
            .await
            .map_err(mutation_error)?;
        created
            .validate(&self.aws_account_id)
            .map_err(|_| unknown("aws_waf_ip_set_create_ack_malformed"))?;
        if created.scope != value.scope || created.name != value.name {
            return Err(unknown("aws_waf_ip_set_create_ack_mismatch"));
        }
        Ok(created)
    }

    pub async fn update_ip_set(
        &self,
        revision: AwsWafIpSetRevision,
        value: AwsWafIpSet,
    ) -> AwsWafApiResult<AwsWafIpSet> {
        self.update_ip_set_tracked(revision, value, &AwsWafMutationDispatch::default())
            .await
    }

    pub async fn update_ip_set_tracked(
        &self,
        revision: AwsWafIpSetRevision,
        value: AwsWafIpSet,
        dispatch: &AwsWafMutationDispatch,
    ) -> AwsWafApiResult<AwsWafIpSet> {
        self.assert_authority()?;
        value.validate(&self.aws_account_id)?;
        let Some(current) = self.api.get_ip_set(&revision.scope, &revision.id).await? else {
            return Err(provider_error(
                ProviderErrorCategory::NotFound,
                "aws_waf_ip_set_not_found",
            ));
        };
        if current.scope != value.scope
            || current.id != value.id
            || current.lock_token != revision.lock_token
        {
            return Err(conflict("aws_waf_ip_set_lock_token_conflict"));
        }
        dispatch.mark_dispatched();
        let updated = self
            .api
            .update_ip_set(&revision, &value)
            .await
            .map_err(mutation_error)?;
        updated
            .validate(&self.aws_account_id)
            .map_err(|_| unknown("aws_waf_ip_set_update_ack_malformed"))?;
        Ok(updated)
    }

    pub async fn delete_ip_set(&self, revision: AwsWafIpSetRevision) -> AwsWafApiResult<()> {
        self.delete_ip_set_tracked(revision, &AwsWafMutationDispatch::default())
            .await
    }

    pub async fn delete_ip_set_tracked(
        &self,
        revision: AwsWafIpSetRevision,
        dispatch: &AwsWafMutationDispatch,
    ) -> AwsWafApiResult<()> {
        self.assert_authority()?;
        let Some(current) = self.api.get_ip_set(&revision.scope, &revision.id).await? else {
            return Err(provider_error(
                ProviderErrorCategory::NotFound,
                "aws_waf_ip_set_not_found",
            ));
        };
        if current.lock_token != revision.lock_token {
            return Err(conflict("aws_waf_ip_set_lock_token_conflict"));
        }
        dispatch.mark_dispatched();
        self.api
            .delete_ip_set(&revision, &current.name)
            .await
            .map_err(mutation_error)
    }

    pub async fn create_web_acl(
        &self,
        request: AwsWafCreateWebAclRequest,
    ) -> AwsWafApiResult<AwsWafWebAcl> {
        self.create_web_acl_tracked(request, &AwsWafMutationDispatch::default())
            .await
    }

    pub async fn create_web_acl_tracked(
        &self,
        request: AwsWafCreateWebAclRequest,
        dispatch: &AwsWafMutationDispatch,
    ) -> AwsWafApiResult<AwsWafWebAcl> {
        self.assert_authority()?;
        request.scope.validate()?;
        request.capability.validate()?;
        let mut candidate = request.to_unbound_acl()?;
        candidate.id = AwsWafWebAclId::new("PLACEHOLDER")?;
        candidate.arn = placeholder_acl_arn(&request.scope, &self.aws_account_id);
        candidate.lock_token = AwsWafLockToken::new("placeholder")?;
        self.validate_candidate(&candidate, &request.capability, true)?;
        dispatch.mark_dispatched();
        let created = self
            .api
            .create_web_acl(&request)
            .await
            .map_err(mutation_error)?;
        self.validate_acl(&created)
            .map_err(|_| unknown("aws_waf_create_ack_malformed"))?;
        if created.scope != request.scope
            || created.name != request.name
            || created.default_action != request.default_action
        {
            return Err(unknown("aws_waf_create_ack_mismatch"));
        }
        Ok(created)
    }

    /// Replaces only Center-owned rules. Current external rules remain in their
    /// original order and are never accepted in the caller's desired vector.
    pub async fn update_center_rules(
        &self,
        update: AwsWafCenterRulesUpdate,
    ) -> AwsWafApiResult<AwsWafWebAcl> {
        self.update_center_rules_tracked(update, &AwsWafMutationDispatch::default())
            .await
    }

    /// Replaces an ACL only after an exact fresh revision check. Composition
    /// uses this for guarded ACL metadata/default-action lifecycle operations.
    pub async fn update_web_acl_tracked(
        &self,
        revision: AwsWafWebAclRevision,
        desired: AwsWafWebAcl,
        dispatch: &AwsWafMutationDispatch,
    ) -> AwsWafApiResult<AwsWafWebAcl> {
        self.assert_authority()?;
        let Some(current) = self.api.get_web_acl(&revision.scope, &revision.id).await? else {
            return Err(provider_error(
                ProviderErrorCategory::NotFound,
                "aws_waf_web_acl_not_found",
            ));
        };
        self.validate_acl(&current)?;
        if current.revision() != revision
            || desired.id != current.id
            || desired.scope != current.scope
        {
            return Err(conflict("aws_waf_lock_token_conflict"));
        }
        dispatch.mark_dispatched();
        let updated = self
            .api
            .update_web_acl(&revision, &desired)
            .await
            .map_err(mutation_error)?;
        self.validate_acl(&updated)
            .map_err(|_| unknown("aws_waf_update_ack_malformed"))?;
        Ok(updated)
    }

    pub async fn update_center_rules_tracked(
        &self,
        update: AwsWafCenterRulesUpdate,
        dispatch: &AwsWafMutationDispatch,
    ) -> AwsWafApiResult<AwsWafWebAcl> {
        self.assert_authority()?;
        update.revision.validate()?;
        update.capability.validate()?;
        let Some(current) = self
            .api
            .get_web_acl(&update.revision.scope, &update.revision.id)
            .await?
        else {
            return Err(provider_error(
                ProviderErrorCategory::NotFound,
                "aws_waf_web_acl_not_found",
            ));
        };
        self.validate_acl(&current)?;
        if current.revision() != update.revision {
            return Err(conflict("aws_waf_lock_token_conflict"));
        }
        if update
            .rules
            .iter()
            .any(|rule| !matches!(rule.owner, AwsWafRuleOwner::Center { .. }))
        {
            return Err(validation("aws_waf_external_rule_update_forbidden"));
        }
        let external = current
            .rules
            .iter()
            .filter(|rule| matches!(rule.owner, AwsWafRuleOwner::External))
            .cloned();
        let rules: Vec<_> = external.chain(update.rules).collect();
        let mut desired = current.clone();
        desired.default_action = update.default_action.unwrap_or(current.default_action);
        desired.rules = rules;
        desired.capacity = capacity(&desired.rules)?;
        self.validate_candidate(&desired, &update.capability, false)?;
        dispatch.mark_dispatched();
        let updated = self
            .api
            .update_web_acl(&update.revision, &desired)
            .await
            .map_err(mutation_error)?;
        self.validate_acl(&updated)
            .map_err(|_| unknown("aws_waf_update_ack_malformed"))?;
        if updated.id != current.id || updated.scope != current.scope {
            return Err(unknown("aws_waf_update_ack_mismatch"));
        }
        Ok(updated)
    }

    pub async fn delete_web_acl(&self, revision: AwsWafWebAclRevision) -> AwsWafApiResult<()> {
        self.delete_web_acl_tracked(revision, &AwsWafMutationDispatch::default())
            .await
    }

    pub async fn delete_web_acl_tracked(
        &self,
        revision: AwsWafWebAclRevision,
        dispatch: &AwsWafMutationDispatch,
    ) -> AwsWafApiResult<()> {
        self.assert_authority()?;
        revision.validate()?;
        let Some(current) = self.api.get_web_acl(&revision.scope, &revision.id).await? else {
            return Err(provider_error(
                ProviderErrorCategory::NotFound,
                "aws_waf_web_acl_not_found",
            ));
        };
        self.validate_acl(&current)?;
        if current.revision() != revision {
            return Err(conflict("aws_waf_lock_token_conflict"));
        }
        if !self
            .api
            .list_associations(&revision.scope, &revision.id)
            .await?
            .is_empty()
        {
            return Err(conflict("aws_waf_web_acl_associations_present"));
        }
        dispatch.mark_dispatched();
        self.api
            .delete_web_acl(&revision)
            .await
            .map_err(mutation_error)
    }

    pub async fn associate_regional_resource(
        &self,
        scope: AwsWafScope,
        target: AwsWafAssociationTarget,
        web_acl: AwsWafWebAclId,
    ) -> AwsWafApiResult<()> {
        self.associate_regional_resource_tracked(
            scope,
            target,
            web_acl,
            &AwsWafMutationDispatch::default(),
        )
        .await
    }

    pub async fn associate_regional_resource_tracked(
        &self,
        scope: AwsWafScope,
        target: AwsWafAssociationTarget,
        web_acl: AwsWafWebAclId,
        dispatch: &AwsWafMutationDispatch,
    ) -> AwsWafApiResult<()> {
        self.assert_authority()?;
        scope.validate()?;
        target.validate(&scope, &self.aws_account_id)?;
        let Some(acl) = self.api.get_web_acl(&scope, &web_acl).await? else {
            return Err(provider_error(
                ProviderErrorCategory::NotFound,
                "aws_waf_web_acl_not_found",
            ));
        };
        self.validate_acl(&acl)?;
        dispatch.mark_dispatched();
        self.api
            .associate_regional_resource(&scope, &target, &web_acl)
            .await
            .map_err(mutation_error)
    }

    pub async fn disassociate_regional_resource(
        &self,
        scope: AwsWafScope,
        target: AwsWafAssociationTarget,
    ) -> AwsWafApiResult<()> {
        self.disassociate_regional_resource_tracked(
            scope,
            target,
            &AwsWafMutationDispatch::default(),
        )
        .await
    }

    pub async fn disassociate_regional_resource_tracked(
        &self,
        scope: AwsWafScope,
        target: AwsWafAssociationTarget,
        dispatch: &AwsWafMutationDispatch,
    ) -> AwsWafApiResult<()> {
        self.assert_authority()?;
        scope.validate()?;
        target.validate(&scope, &self.aws_account_id)?;
        dispatch.mark_dispatched();
        self.api
            .disassociate_regional_resource(&scope, &target)
            .await
            .map_err(mutation_error)
    }

    fn assert_authority(&self) -> AwsWafApiResult<()> {
        if self.api.verified_account_id() == self.aws_account_id
            && self.api.credential_revision() == self.credential_revision
        {
            Ok(())
        } else {
            Err(unknown("aws_waf_provider_authority_changed"))
        }
    }
    fn validate_acl(&self, acl: &AwsWafWebAcl) -> AwsWafApiResult<()> {
        acl.validate(&self.aws_account_id, &inventory_capability(acl))
    }
    fn validate_candidate(
        &self,
        acl: &AwsWafWebAcl,
        capability: &AwsWafCapabilityEvidence,
        _: bool,
    ) -> AwsWafApiResult<()> {
        acl.validate(&self.aws_account_id, capability)
    }
}

fn inventory_capability(acl: &AwsWafWebAcl) -> AwsWafCapabilityEvidence {
    let managed_rule_groups = acl
        .rules
        .iter()
        .filter_map(|rule| match &rule.statement {
            AwsWafStatement::ManagedRuleGroup(group) => Some(group.key()),
            _ => None,
        })
        .collect();
    AwsWafCapabilityEvidence {
        managed_rule_groups,
        challenge_allowed: true,
        captcha_allowed: true,
        maximum_wcu: acl.capacity.max(1),
    }
}
fn valid_catalog_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}
fn placeholder_acl_arn(scope: &AwsWafScope, account: &str) -> String {
    let (region, scope_segment) = match scope {
        AwsWafScope::Cloudfront => ("us-east-1", "global"),
        AwsWafScope::Regional { region } => (region.as_str(), "regional"),
    };
    format!("arn:aws:wafv2:{region}:{account}:{scope_segment}/webacl/placeholder")
}
fn mutation_error(
    error: edgion_center_core::NormalizedProviderError,
) -> edgion_center_core::NormalizedProviderError {
    if matches!(error.category(), ProviderErrorCategory::Transient) {
        unknown("aws_waf_mutation_outcome_unknown")
    } else {
        error
    }
}
fn unknown(code: &str) -> edgion_center_core::NormalizedProviderError {
    provider_error(ProviderErrorCategory::UnknownOutcome, code)
}

#[cfg(test)]
mod tests;
