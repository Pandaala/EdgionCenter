use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use edgion_center_core::{
    CloudProvider, CloudResourceId, CredentialSource, ProviderAccountScope, ProviderAccountSpec,
    ProviderErrorCategory,
};

use super::*;

const ACCOUNT: &str = "123456789012";

struct FakeApi {
    account: String,
    revision: String,
    acls: Mutex<Vec<AwsWafWebAcl>>,
    ip_sets: Mutex<Vec<AwsWafIpSet>>,
    associations: Mutex<Vec<AwsWafAssociation>>,
    calls: Mutex<Vec<String>>,
    mutation_error: Mutex<Option<edgion_center_core::NormalizedProviderError>>,
}

#[async_trait]
impl AwsWafApi for FakeApi {
    fn verified_account_id(&self) -> &str {
        &self.account
    }
    fn credential_revision(&self) -> &str {
        &self.revision
    }
    async fn list_web_acls(
        &self,
        scope: &AwsWafScope,
        _marker: Option<&str>,
        _limit: u16,
    ) -> AwsWafApiResult<AwsWafWebAclPage> {
        self.calls.lock().unwrap().push(format!("list:{scope:?}"));
        Ok(AwsWafWebAclPage {
            items: self
                .acls
                .lock()
                .unwrap()
                .iter()
                .filter(|acl| &acl.scope == scope)
                .cloned()
                .collect(),
            next_marker: None,
        })
    }
    async fn get_web_acl(
        &self,
        scope: &AwsWafScope,
        id: &AwsWafWebAclId,
    ) -> AwsWafApiResult<Option<AwsWafWebAcl>> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("get:{}", id.as_str()));
        Ok(self
            .acls
            .lock()
            .unwrap()
            .iter()
            .find(|acl| &acl.scope == scope && &acl.id == id)
            .cloned())
    }
    async fn list_ip_sets(
        &self,
        scope: &AwsWafScope,
        _marker: Option<&str>,
        _limit: u16,
    ) -> AwsWafApiResult<AwsWafIpSetPage> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("list-ip:{scope:?}"));
        Ok(AwsWafIpSetPage {
            items: self
                .ip_sets
                .lock()
                .unwrap()
                .iter()
                .filter(|value| &value.scope == scope)
                .cloned()
                .collect(),
            next_marker: None,
        })
    }
    async fn get_ip_set(
        &self,
        scope: &AwsWafScope,
        id: &AwsWafIpSetId,
    ) -> AwsWafApiResult<Option<AwsWafIpSet>> {
        self.calls
            .lock()
            .unwrap()
            .push(format!("get-ip:{}", id.as_str()));
        Ok(self
            .ip_sets
            .lock()
            .unwrap()
            .iter()
            .find(|value| &value.scope == scope && &value.id == id)
            .cloned())
    }
    async fn create_ip_set(&self, value: &AwsWafIpSet) -> AwsWafApiResult<AwsWafIpSet> {
        self.calls.lock().unwrap().push("create-ip".to_string());
        if let Some(error) = self.mutation_error.lock().unwrap().take() {
            return Err(error);
        }
        self.ip_sets.lock().unwrap().push(value.clone());
        Ok(value.clone())
    }
    async fn update_ip_set(
        &self,
        revision: &AwsWafIpSetRevision,
        desired: &AwsWafIpSet,
    ) -> AwsWafApiResult<AwsWafIpSet> {
        self.calls.lock().unwrap().push("update-ip".to_string());
        if let Some(error) = self.mutation_error.lock().unwrap().take() {
            return Err(error);
        }
        let mut values = self.ip_sets.lock().unwrap();
        let current = values
            .iter_mut()
            .find(|value| value.id == revision.id && value.scope == revision.scope)
            .ok_or_else(|| conflict("fake_ip_set_missing"))?;
        if current.lock_token != revision.lock_token {
            return Err(conflict("fake_ip_set_lock_conflict"));
        }
        let mut updated = desired.clone();
        updated.lock_token = AwsWafLockToken::new("new-ip-lock").unwrap();
        *current = updated.clone();
        Ok(updated)
    }
    async fn delete_ip_set(
        &self,
        revision: &AwsWafIpSetRevision,
        _name: &str,
    ) -> AwsWafApiResult<()> {
        self.calls.lock().unwrap().push("delete-ip".to_string());
        if let Some(error) = self.mutation_error.lock().unwrap().take() {
            return Err(error);
        }
        let mut values = self.ip_sets.lock().unwrap();
        let index = values
            .iter()
            .position(|value| {
                value.id == revision.id
                    && value.scope == revision.scope
                    && value.lock_token == revision.lock_token
            })
            .ok_or_else(|| conflict("fake_ip_set_lock_conflict"))?;
        values.remove(index);
        Ok(())
    }
    async fn create_web_acl(
        &self,
        request: &AwsWafCreateWebAclRequest,
    ) -> AwsWafApiResult<AwsWafWebAcl> {
        self.calls.lock().unwrap().push("create".to_string());
        if let Some(error) = self.mutation_error.lock().unwrap().take() {
            return Err(error);
        }
        let id = AwsWafWebAclId::new("CREATED").unwrap();
        let acl = AwsWafWebAcl {
            id: id.clone(),
            name: request.name.clone(),
            arn: acl_arn(&request.scope, &id),
            scope: request.scope.clone(),
            default_action: request.default_action,
            visibility: request.visibility.clone(),
            capacity: capacity(&request.rules).unwrap(),
            lock_token: AwsWafLockToken::new("created-lock").unwrap(),
            rules: request.rules.clone(),
        };
        self.acls.lock().unwrap().push(acl.clone());
        Ok(acl)
    }
    async fn update_web_acl(
        &self,
        revision: &AwsWafWebAclRevision,
        desired: &AwsWafWebAcl,
    ) -> AwsWafApiResult<AwsWafWebAcl> {
        self.calls.lock().unwrap().push("update".to_string());
        if let Some(error) = self.mutation_error.lock().unwrap().take() {
            return Err(error);
        }
        let mut acls = self.acls.lock().unwrap();
        let current = acls
            .iter_mut()
            .find(|acl| acl.id == revision.id && acl.scope == revision.scope)
            .unwrap();
        if current.lock_token != revision.lock_token {
            return Err(conflict("fake_lock_conflict"));
        }
        let mut updated = desired.clone();
        updated.lock_token = AwsWafLockToken::new("new-lock").unwrap();
        *current = updated.clone();
        Ok(updated)
    }
    async fn delete_web_acl(&self, revision: &AwsWafWebAclRevision) -> AwsWafApiResult<()> {
        self.calls.lock().unwrap().push("delete".to_string());
        if let Some(error) = self.mutation_error.lock().unwrap().take() {
            return Err(error);
        }
        let mut acls = self.acls.lock().unwrap();
        let position = acls
            .iter()
            .position(|acl| {
                acl.id == revision.id
                    && acl.scope == revision.scope
                    && acl.lock_token == revision.lock_token
            })
            .ok_or_else(|| conflict("fake_lock_conflict"))?;
        acls.remove(position);
        Ok(())
    }
    async fn list_associations(
        &self,
        _scope: &AwsWafScope,
        web_acl: &AwsWafWebAclId,
    ) -> AwsWafApiResult<Vec<AwsWafAssociation>> {
        Ok(self
            .associations
            .lock()
            .unwrap()
            .iter()
            .filter(|value| &value.web_acl_id == web_acl)
            .cloned()
            .collect())
    }
    async fn associate_regional_resource(
        &self,
        scope: &AwsWafScope,
        target: &AwsWafAssociationTarget,
        web_acl: &AwsWafWebAclId,
    ) -> AwsWafApiResult<()> {
        self.calls.lock().unwrap().push("associate".to_string());
        self.associations.lock().unwrap().push(AwsWafAssociation {
            target: target.clone(),
            web_acl_id: web_acl.clone(),
        });
        if scope.is_cloudfront() {
            return Err(validation("fake_scope"));
        }
        Ok(())
    }
    async fn disassociate_regional_resource(
        &self,
        scope: &AwsWafScope,
        target: &AwsWafAssociationTarget,
    ) -> AwsWafApiResult<()> {
        self.calls.lock().unwrap().push("disassociate".to_string());
        if scope.is_cloudfront() {
            return Err(validation("fake_scope"));
        }
        self.associations
            .lock()
            .unwrap()
            .retain(|value| &value.target != target);
        Ok(())
    }
}

fn account() -> ProviderAccountSpec {
    ProviderAccountSpec {
        provider: CloudProvider::Aws,
        scope: Some(ProviderAccountScope::Aws {
            account_id: ACCOUNT.to_string(),
        }),
        credential_source: CredentialSource::Ambient,
    }
}
fn scope() -> AwsWafScope {
    AwsWafScope::Regional {
        region: "us-west-2".to_string(),
    }
}
fn acl_arn(scope: &AwsWafScope, id: &AwsWafWebAclId) -> String {
    let (region, scope_part) = match scope {
        AwsWafScope::Cloudfront => ("us-east-1", "global"),
        AwsWafScope::Regional { region } => (region.as_str(), "regional"),
    };
    format!(
        "arn:aws:wafv2:{region}:{ACCOUNT}:{scope_part}/webacl/{}",
        id.as_str()
    )
}
fn ip_set() -> AwsWafIpSet {
    let id = AwsWafIpSetId::new("IPSETONE").unwrap();
    AwsWafIpSet {
        id: id.clone(),
        name: "office-addresses".to_string(),
        arn: format!(
            "arn:aws:wafv2:us-west-2:{ACCOUNT}:regional/ipset/{}",
            id.as_str()
        ),
        scope: scope(),
        address_version: AwsWafIpAddressVersion::Ipv4,
        addresses: BTreeSet::from(["192.0.2.0/24".to_string()]),
        lock_token: AwsWafLockToken::new("ip-lock").unwrap(),
    }
}
fn ip_rule(name: &str, priority: u32, owner: AwsWafRuleOwner) -> AwsWafRule {
    AwsWafRule {
        name: name.to_string(),
        priority,
        action: AwsWafAction::Block,
        statement: AwsWafStatement::IpSetReference(AwsWafIpSetReference {
            arn: format!("arn:aws:wafv2:us-west-2:{ACCOUNT}:regional/ipset/allow-list"),
        }),
        visibility: AwsWafVisibilityConfig {
            cloudwatch_metrics_enabled: true,
            sampled_requests_enabled: false,
            metric_name: format!("metric-{name}"),
        },
        owner,
        managed_override_action: None,
    }
}
fn capability() -> AwsWafCapabilityEvidence {
    AwsWafCapabilityEvidence {
        managed_rule_groups: BTreeSet::from(["AWS/ManagedRulesCommonRuleSet".to_string()]),
        challenge_allowed: true,
        captcha_allowed: true,
        maximum_wcu: 100,
    }
}
fn acl(rules: Vec<AwsWafRule>) -> AwsWafWebAcl {
    let id = AwsWafWebAclId::new("ACLONE").unwrap();
    AwsWafWebAcl {
        id: id.clone(),
        name: "acl-one".to_string(),
        arn: acl_arn(&scope(), &id),
        scope: scope(),
        default_action: AwsWafDefaultAction::Block,
        visibility: AwsWafVisibilityConfig {
            cloudwatch_metrics_enabled: true,
            sampled_requests_enabled: false,
            metric_name: "acl-metric".to_string(),
        },
        capacity: capacity(&rules).unwrap(),
        lock_token: AwsWafLockToken::new("lock-one").unwrap(),
        rules,
    }
}
fn fake(acls: Vec<AwsWafWebAcl>) -> Arc<FakeApi> {
    Arc::new(FakeApi {
        account: ACCOUNT.to_string(),
        revision: "credential-v1".to_string(),
        acls: Mutex::new(acls),
        ip_sets: Mutex::new(Vec::new()),
        associations: Mutex::new(Vec::new()),
        calls: Mutex::new(Vec::new()),
        mutation_error: Mutex::new(None),
    })
}
fn adapter(api: Arc<FakeApi>) -> AwsWafAdapter {
    AwsWafAdapter::new(
        CloudResourceId::new("aws-waf-account").unwrap(),
        1,
        &account(),
        api,
    )
    .unwrap()
}

#[tokio::test]
async fn inventory_is_scope_isolated_and_lock_tokens_are_redacted() {
    let regional = acl(vec![ip_rule("external", 10, AwsWafRuleOwner::External)]);
    let global_id = AwsWafWebAclId::new("GLOBALONE").unwrap();
    let global = AwsWafWebAcl {
        id: global_id.clone(),
        name: "global".to_string(),
        arn: acl_arn(&AwsWafScope::Cloudfront, &global_id),
        scope: AwsWafScope::Cloudfront,
        default_action: AwsWafDefaultAction::Block,
        visibility: regional.visibility.clone(),
        capacity: 0,
        lock_token: AwsWafLockToken::new("top-secret-lock").unwrap(),
        rules: Vec::new(),
    };
    let api = fake(vec![regional, global]);
    let result = adapter(api).inventory(scope()).await.unwrap();
    assert_eq!(result.len(), 1);
    assert!(!format!("{:?}", result[0].lock_token).contains("lock-one"));
}

#[tokio::test]
async fn managed_groups_capacity_and_action_entitlements_are_guarded_before_dispatch() {
    let api = fake(Vec::new());
    let adapter = adapter(api.clone());
    let mut managed = ip_rule(
        "managed",
        1,
        AwsWafRuleOwner::Center {
            reference: "owned".to_string(),
        },
    );
    managed.statement = AwsWafStatement::ManagedRuleGroup(AwsWafManagedRuleGroup {
        vendor_name: "AWS".to_string(),
        name: "ManagedRulesCommonRuleSet".to_string(),
        version: None,
        capacity: 80,
        excluded_rules: BTreeSet::from(["NoUserAgent_HEADER".to_string()]),
        rule_action_overrides: vec![AwsWafManagedRuleOverride {
            name: "SizeRestrictions_BODY".to_string(),
            action: AwsWafAction::Count,
        }],
    });
    managed.managed_override_action = Some(AwsWafManagedRuleOverrideAction::None);
    let request = AwsWafCreateWebAclRequest {
        name: "new-acl".to_string(),
        scope: scope(),
        default_action: AwsWafDefaultAction::Block,
        visibility: AwsWafVisibilityConfig {
            cloudwatch_metrics_enabled: true,
            sampled_requests_enabled: false,
            metric_name: "new-acl".to_string(),
        },
        rules: vec![managed.clone()],
        capability: capability(),
    };
    adapter.create_web_acl(request).await.unwrap();
    managed.action = AwsWafAction::Captcha;
    let mut denied = capability();
    denied.captcha_allowed = false;
    let error = adapter
        .create_web_acl(AwsWafCreateWebAclRequest {
            name: "denied".to_string(),
            scope: scope(),
            default_action: AwsWafDefaultAction::Block,
            visibility: AwsWafVisibilityConfig {
                cloudwatch_metrics_enabled: true,
                sampled_requests_enabled: false,
                metric_name: "denied".to_string(),
            },
            rules: vec![managed.clone()],
            capability: denied,
        })
        .await
        .unwrap_err();
    assert_eq!(error.category(), ProviderErrorCategory::Authorization);
    let mut over_capacity = managed;
    over_capacity.action = AwsWafAction::Block;
    if let AwsWafStatement::ManagedRuleGroup(group) = &mut over_capacity.statement {
        group.capacity = 101;
    }
    let error = adapter
        .create_web_acl(AwsWafCreateWebAclRequest {
            name: "over-capacity".to_string(),
            scope: scope(),
            default_action: AwsWafDefaultAction::Allow,
            visibility: AwsWafVisibilityConfig {
                cloudwatch_metrics_enabled: true,
                sampled_requests_enabled: false,
                metric_name: "over-capacity".to_string(),
            },
            rules: vec![over_capacity],
            capability: capability(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.category(), ProviderErrorCategory::Quota);
    assert_eq!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.as_str() == "create")
            .count(),
        1
    );
}

#[tokio::test]
async fn typed_rate_rules_validate_scope_down_ip_set_and_wcu() {
    let api = fake(Vec::new());
    let adapter = adapter(api.clone());
    let rule = AwsWafRule {
        name: "rate-limit".to_string(),
        priority: 1,
        action: AwsWafAction::Block,
        statement: AwsWafStatement::RateBased {
            limit: 100,
            aggregate_key: AwsWafRateAggregateKey::Ip,
            scope_down_ip_set: Some(AwsWafIpSetReference {
                arn: format!("arn:aws:wafv2:us-west-2:{ACCOUNT}:regional/ipset/rate-allow-list"),
            }),
        },
        visibility: AwsWafVisibilityConfig {
            cloudwatch_metrics_enabled: true,
            sampled_requests_enabled: true,
            metric_name: "rate-limit".to_string(),
        },
        owner: AwsWafRuleOwner::Center {
            reference: "rate-limit".to_string(),
        },
        managed_override_action: None,
    };
    let created = adapter
        .create_web_acl(AwsWafCreateWebAclRequest {
            name: "rate-acl".to_string(),
            scope: scope(),
            default_action: AwsWafDefaultAction::Block,
            visibility: AwsWafVisibilityConfig {
                cloudwatch_metrics_enabled: true,
                sampled_requests_enabled: false,
                metric_name: "rate-acl".to_string(),
            },
            rules: vec![rule],
            capability: capability(),
        })
        .await
        .unwrap();
    assert_eq!(created.capacity, 3);
    assert_eq!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.as_str() == "create")
            .count(),
        1
    );
}

#[tokio::test]
async fn update_preserves_external_rules_and_rejects_priority_or_lock_conflicts() {
    let current = acl(vec![
        ip_rule("external", 10, AwsWafRuleOwner::External),
        ip_rule(
            "old-owned",
            20,
            AwsWafRuleOwner::Center {
                reference: "old".to_string(),
            },
        ),
    ]);
    let api = fake(vec![current.clone()]);
    let adapter = adapter(api.clone());
    let replacement = ip_rule(
        "new-owned",
        30,
        AwsWafRuleOwner::Center {
            reference: "new".to_string(),
        },
    );
    let updated = adapter
        .update_center_rules(AwsWafCenterRulesUpdate {
            revision: current.revision(),
            default_action: None,
            rules: vec![replacement],
            capability: capability(),
        })
        .await
        .unwrap();
    assert_eq!(
        updated
            .rules
            .iter()
            .map(|rule| rule.name.as_str())
            .collect::<Vec<_>>(),
        vec!["external", "new-owned"]
    );
    let collision = ip_rule(
        "collision",
        10,
        AwsWafRuleOwner::Center {
            reference: "collision".to_string(),
        },
    );
    let error = adapter
        .update_center_rules(AwsWafCenterRulesUpdate {
            revision: updated.revision(),
            default_action: None,
            rules: vec![collision],
            capability: capability(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.code(), "aws_waf_rule_priority_collision");
    let mut stale = updated.revision();
    stale.lock_token = AwsWafLockToken::new("old-lock").unwrap();
    let error = adapter
        .update_center_rules(AwsWafCenterRulesUpdate {
            revision: stale,
            default_action: None,
            rules: Vec::new(),
            capability: capability(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.code(), "aws_waf_lock_token_conflict");
}

#[tokio::test]
async fn mutations_dispatch_once_and_ambiguous_errors_require_observation() {
    let current = acl(vec![ip_rule(
        "owned",
        1,
        AwsWafRuleOwner::Center {
            reference: "owned".to_string(),
        },
    )]);
    let api = fake(vec![current.clone()]);
    *api.mutation_error.lock().unwrap() = Some(provider_error(
        ProviderErrorCategory::Transient,
        "transport_timeout",
    ));
    let error = adapter(api.clone())
        .update_center_rules(AwsWafCenterRulesUpdate {
            revision: current.revision(),
            default_action: None,
            rules: Vec::new(),
            capability: capability(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.category(), ProviderErrorCategory::UnknownOutcome);
    assert_eq!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.as_str() == "update")
            .count(),
        1
    );
}

#[tokio::test]
async fn regional_association_is_typed_and_cloudfront_scope_is_rejected() {
    let current = acl(Vec::new());
    let api = fake(vec![current.clone()]);
    let adapter = adapter(api);
    let target = AwsWafAssociationTarget {
        resource_arn: format!(
            "arn:aws:elasticloadbalancing:us-west-2:{ACCOUNT}:loadbalancer/app/test/123"
        ),
        resource_kind: AwsWafRegionalResourceKind::ApplicationLoadBalancer,
    };
    adapter
        .associate_regional_resource(scope(), target.clone(), current.id.clone())
        .await
        .unwrap();
    let error = adapter
        .associate_regional_resource(AwsWafScope::Cloudfront, target, current.id)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "aws_waf_regional_association_scope_required");
}

#[tokio::test]
async fn ip_set_update_checks_the_fresh_lock_before_single_dispatch() {
    let value = ip_set();
    let api = fake(Vec::new());
    api.ip_sets.lock().unwrap().push(value.clone());
    let adapter = adapter(api.clone());
    let mut stale = value.revision();
    stale.lock_token = AwsWafLockToken::new("stale-ip-lock").unwrap();
    let dispatch = AwsWafMutationDispatch::default();
    let error = adapter
        .update_ip_set_tracked(stale, value, &dispatch)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "aws_waf_ip_set_lock_token_conflict");
    assert!(!dispatch.was_dispatched());
    let calls = api.calls.lock().unwrap().clone();
    assert_eq!(
        calls
            .iter()
            .filter(|call| call.starts_with("get-ip:"))
            .count(),
        1
    );
    assert!(!calls.iter().any(|call| call == "update-ip"));
}

#[tokio::test]
async fn ip_set_mutation_is_dispatched_once_and_ambiguous_outcome_is_preserved() {
    let value = ip_set();
    let api = fake(Vec::new());
    *api.mutation_error.lock().unwrap() = Some(provider_error(
        ProviderErrorCategory::Transient,
        "transport_timeout",
    ));
    let dispatch = AwsWafMutationDispatch::default();
    let error = adapter(api.clone())
        .create_ip_set_tracked(value, &dispatch)
        .await
        .unwrap_err();
    assert_eq!(error.category(), ProviderErrorCategory::UnknownOutcome);
    assert!(dispatch.was_dispatched());
    assert_eq!(
        api.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.as_str() == "create-ip")
            .count(),
        1
    );
}

#[test]
fn ip_set_rejects_invalid_or_wrong_version_cidrs() {
    let mut value = ip_set();
    value.addresses = BTreeSet::from(["not-an-address".to_string()]);
    assert!(value.validate(ACCOUNT).is_err());
    value.addresses = BTreeSet::from(["2001:db8::/32".to_string()]);
    assert!(value.validate(ACCOUNT).is_err());
}

#[test]
fn arbitrary_statement_and_action_json_are_not_a_contract() {
    let raw = r#"{"kind":"raw","statement":{"anything":true}}"#;
    assert!(serde_json::from_str::<AwsWafStatement>(raw).is_err());
    let raw = r#"{"name":"x","priority":1,"action":{"arbitrary":"json"},"statement":{"kind":"rate_based","limit":100,"aggregate_key":"ip"},"visibility":{"cloudwatchMetricsEnabled":true,"sampledRequestsEnabled":false,"metricName":"x"},"owner":{"kind":"external"}}"#;
    assert!(serde_json::from_str::<AwsWafRule>(raw).is_err());
}

#[test]
fn authoritative_capacity_requires_nonzero_provider_ceiling() {
    assert!(AwsWafCapacityObservation {
        scope: scope(),
        required_wcu: 10,
        account_wcu_ceiling: 10
    }
    .authorize()
    .is_ok());
    assert_eq!(
        AwsWafCapacityObservation {
            scope: scope(),
            required_wcu: 11,
            account_wcu_ceiling: 10
        }
        .authorize()
        .unwrap_err()
        .code(),
        "aws_waf_check_capacity_exceeded"
    );
    assert!(AwsWafCapacityObservation {
        scope: scope(),
        required_wcu: 0,
        account_wcu_ceiling: 0
    }
    .authorize()
    .is_err());
}
