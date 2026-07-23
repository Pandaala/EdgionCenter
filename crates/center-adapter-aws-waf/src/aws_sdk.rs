//! AWS SDK transport for the bounded WAFv2 seam.
//!
//! The adapter owns no Center credential resolution. Callers provide an SDK
//! configuration created through the ambient/assume-role factory and this
//! transport verifies its account with STS before issuing WAF calls.

use std::{collections::BTreeSet, time::Duration};

use async_trait::async_trait;
use aws_config::SdkConfig;
use aws_sdk_wafv2::{
    config::{retry::RetryConfig, timeout::TimeoutConfig, Region},
    error::ProvideErrorMetadata,
    types::{
        AllowAction, BlockAction, CaptchaAction, ChallengeAction, CountAction, DefaultAction,
        ExcludedRule, IpAddressVersion, IpSetReferenceStatement, ManagedRuleGroupStatement,
        OverrideAction, RateBasedStatement, RateBasedStatementAggregateKeyType, Rule, RuleAction,
        RuleActionOverride, Scope, Statement, VisibilityConfig,
    },
};
use edgion_center_core::{NormalizedProviderError, ProviderErrorCategory};

use crate::{
    AwsWafAction, AwsWafApi, AwsWafApiResult, AwsWafAssociation, AwsWafAssociationTarget,
    AwsWafCapabilityEvidence, AwsWafCreateWebAclRequest, AwsWafDefaultAction,
    AwsWafIpAddressVersion, AwsWafIpSet, AwsWafIpSetId, AwsWafIpSetPage, AwsWafIpSetReference,
    AwsWafIpSetRevision, AwsWafLockToken, AwsWafManagedRuleGroup,
    AwsWafManagedRuleGroupCatalogEntry, AwsWafManagedRuleOverride, AwsWafManagedRuleOverrideAction,
    AwsWafRateAggregateKey, AwsWafRegionalResourceKind, AwsWafRule, AwsWafRuleOwner, AwsWafScope,
    AwsWafStatement, AwsWafVisibilityConfig, AwsWafWebAcl, AwsWafWebAclId, AwsWafWebAclPage,
    AwsWafWebAclRevision,
};

const READ_MAX_ATTEMPTS: u32 = 3;
const MUTATION_MAX_ATTEMPTS: u32 = 1;
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_THROTTLE_RETRY_AFTER_MS: u64 = 1_000;
const CATALOG_PAGE_SIZE: i32 = 100;
const MAX_CATALOG_PAGES: usize = 100;

/// Test-only endpoint seam. Production constructors leave this at its default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AwsWafSdkApiOptions {
    pub waf_endpoint_url: Option<String>,
    pub sts_endpoint_url: Option<String>,
}

pub struct AwsWafSdkApi {
    account_id: String,
    verified_partition: String,
    sdk_config: SdkConfig,
    options: AwsWafSdkApiOptions,
    credential_revision: String,
}

impl AwsWafSdkApi {
    pub async fn new(sdk_config: &SdkConfig) -> AwsWafApiResult<Self> {
        Self::with_options(sdk_config, AwsWafSdkApiOptions::default()).await
    }

    pub async fn with_options(
        sdk_config: &SdkConfig,
        options: AwsWafSdkApiOptions,
    ) -> AwsWafApiResult<Self> {
        if sdk_config.endpoint_url().is_some() {
            return Err(validation("configured_aws_waf_endpoint_override_forbidden"));
        }
        let timeout = timeout_config();
        let mut sts = aws_sdk_sts::config::Builder::from(sdk_config)
            .retry_config(
                aws_sdk_sts::config::retry::RetryConfig::standard()
                    .with_max_attempts(READ_MAX_ATTEMPTS),
            )
            .timeout_config(timeout);
        if let Some(endpoint) = &options.sts_endpoint_url {
            validate_test_endpoint(endpoint)?;
            sts = sts.endpoint_url(endpoint);
        }
        let identity = aws_sdk_sts::Client::from_conf(sts.build())
            .get_caller_identity()
            .send()
            .await
            .map_err(|error| {
                map_read_error(error.as_service_error().and_then(|value| value.code()))
            })?;
        let account_id = identity
            .account()
            .filter(|value| is_aws_account_id(value))
            .ok_or_else(|| validation("invalid_sts_account_id"))?
            .to_string();
        let verified_partition = identity
            .arn()
            .and_then(aws_arn_partition)
            .ok_or_else(|| validation("invalid_sts_arn_partition"))?
            .to_string();
        if let Some(endpoint) = &options.waf_endpoint_url {
            validate_test_endpoint(endpoint)?;
        }
        Ok(Self {
            account_id,
            verified_partition,
            sdk_config: sdk_config.clone(),
            options,
            // The SDK chain refreshes credentials internally. Composition must rebuild this
            // transport on a known credential-authority revision.
            credential_revision: "aws-sdk-ambient-v1".to_string(),
        })
    }

    fn client(&self, scope: &AwsWafScope, mutation: bool) -> aws_sdk_wafv2::Client {
        let retry = RetryConfig::standard().with_max_attempts(if mutation {
            MUTATION_MAX_ATTEMPTS
        } else {
            READ_MAX_ATTEMPTS
        });
        let region = match scope {
            AwsWafScope::Cloudfront => Region::new("us-east-1"),
            AwsWafScope::Regional { region } => Region::new(region.clone()),
        };
        let mut builder = aws_sdk_wafv2::config::Builder::from(&self.sdk_config)
            .region(region)
            .retry_config(retry)
            .timeout_config(timeout_config());
        if let Some(endpoint) = &self.options.waf_endpoint_url {
            builder = builder.endpoint_url(endpoint);
        }
        aws_sdk_wafv2::Client::from_conf(builder.build())
    }

    async fn get_web_acl_named(
        &self,
        scope: &AwsWafScope,
        id: &AwsWafWebAclId,
        name: &str,
    ) -> AwsWafApiResult<Option<AwsWafWebAcl>> {
        let output = self
            .client(scope, false)
            .get_web_acl()
            .name(name)
            .id(id.as_str())
            .scope(sdk_scope(scope))
            .send()
            .await
            .map_err(|error| {
                map_read_error(error.as_service_error().and_then(|value| value.code()))
            })?;
        match (output.web_acl(), output.lock_token()) {
            (Some(acl), Some(token)) => {
                require_arn_partition(acl.arn(), &self.verified_partition)?;
                from_sdk_acl(acl, token, scope, &self.account_id).map(Some)
            }
            _ => Ok(None),
        }
    }

    async fn get_ip_set_named(
        &self,
        scope: &AwsWafScope,
        id: &AwsWafIpSetId,
        name: &str,
    ) -> AwsWafApiResult<Option<AwsWafIpSet>> {
        let output = self
            .client(scope, false)
            .get_ip_set()
            .name(name)
            .id(id.as_str())
            .scope(sdk_scope(scope))
            .send()
            .await
            .map_err(|error| {
                map_read_error(error.as_service_error().and_then(|value| value.code()))
            })?;
        match (output.ip_set(), output.lock_token()) {
            (Some(value), Some(token)) => {
                require_arn_partition(value.arn(), &self.verified_partition)?;
                from_sdk_ip_set(value, token, scope, &self.account_id).map(Some)
            }
            _ => Ok(None),
        }
    }

    async fn resolve_web_acl_name(
        &self,
        scope: &AwsWafScope,
        id: &AwsWafWebAclId,
    ) -> AwsWafApiResult<Option<String>> {
        let mut marker = None;
        let mut seen_markers = BTreeSet::new();
        let mut found = None;
        for _ in 0..MAX_CATALOG_PAGES {
            let output = self
                .client(scope, false)
                .list_web_acls()
                .scope(sdk_scope(scope))
                .set_next_marker(marker.clone())
                .limit(CATALOG_PAGE_SIZE)
                .send()
                .await
                .map_err(|error| {
                    map_read_error(error.as_service_error().and_then(|value| value.code()))
                })?;
            for summary in output.web_acls() {
                if summary.id() == Some(id.as_str()) {
                    let name = summary
                        .name()
                        .ok_or_else(|| unknown("aws_waf_list_name_missing"))?
                        .to_string();
                    if found.replace(name).is_some() {
                        return Err(validation("aws_waf_web_acl_id_ambiguous"));
                    }
                }
            }
            match validated_catalog_marker(output.next_marker(), &mut seen_markers)? {
                Some(next) => marker = Some(next),
                None => return Ok(found),
            }
        }
        Err(validation("aws_waf_web_acl_name_lookup_pagination_limit"))
    }

    async fn resolve_ip_set_name(
        &self,
        scope: &AwsWafScope,
        id: &AwsWafIpSetId,
    ) -> AwsWafApiResult<Option<String>> {
        let mut marker = None;
        let mut seen_markers = BTreeSet::new();
        let mut found = None;
        for _ in 0..MAX_CATALOG_PAGES {
            let output = self
                .client(scope, false)
                .list_ip_sets()
                .scope(sdk_scope(scope))
                .set_next_marker(marker.clone())
                .limit(CATALOG_PAGE_SIZE)
                .send()
                .await
                .map_err(|error| {
                    map_read_error(error.as_service_error().and_then(|value| value.code()))
                })?;
            for summary in output.ip_sets() {
                if summary.id() == Some(id.as_str()) {
                    let name = summary
                        .name()
                        .ok_or_else(|| unknown("aws_waf_ip_set_list_name_missing"))?
                        .to_string();
                    if found.replace(name).is_some() {
                        return Err(validation("aws_waf_ip_set_id_ambiguous"));
                    }
                }
            }
            match validated_catalog_marker(output.next_marker(), &mut seen_markers)? {
                Some(next) => marker = Some(next),
                None => return Ok(found),
            }
        }
        Err(validation("aws_waf_ip_set_name_lookup_pagination_limit"))
    }
}

#[async_trait]
impl AwsWafApi for AwsWafSdkApi {
    fn verified_account_id(&self) -> &str {
        &self.account_id
    }
    fn credential_revision(&self) -> &str {
        &self.credential_revision
    }

    async fn list_web_acls(
        &self,
        scope: &AwsWafScope,
        marker: Option<&str>,
        limit: u16,
    ) -> AwsWafApiResult<AwsWafWebAclPage> {
        let output = self
            .client(scope, false)
            .list_web_acls()
            .scope(sdk_scope(scope))
            .set_next_marker(marker.map(str::to_owned))
            .limit(i32::from(limit))
            .send()
            .await
            .map_err(|error| {
                map_read_error(error.as_service_error().and_then(|value| value.code()))
            })?;
        let mut items = Vec::new();
        for summary in output.web_acls() {
            let id = AwsWafWebAclId::new(
                summary
                    .id()
                    .ok_or_else(|| unknown("aws_waf_list_id_missing"))?,
            )
            .map_err(|_| validation("invalid_aws_waf_web_acl_id"))?;
            let name = summary
                .name()
                .ok_or_else(|| unknown("aws_waf_list_name_missing"))?;
            let Some(acl) = self.get_web_acl_named(scope, &id, name).await? else {
                return Err(unknown("aws_waf_list_get_race"));
            };
            items.push(acl);
        }
        Ok(AwsWafWebAclPage {
            items,
            next_marker: output.next_marker().map(str::to_owned),
        })
    }

    async fn get_web_acl(
        &self,
        scope: &AwsWafScope,
        id: &AwsWafWebAclId,
    ) -> AwsWafApiResult<Option<AwsWafWebAcl>> {
        let Some(name) = self.resolve_web_acl_name(scope, id).await? else {
            return Ok(None);
        };
        self.get_web_acl_named(scope, id, &name).await
    }

    async fn list_ip_sets(
        &self,
        scope: &AwsWafScope,
        marker: Option<&str>,
        limit: u16,
    ) -> AwsWafApiResult<AwsWafIpSetPage> {
        let output = self
            .client(scope, false)
            .list_ip_sets()
            .scope(sdk_scope(scope))
            .set_next_marker(marker.map(str::to_owned))
            .limit(i32::from(limit))
            .send()
            .await
            .map_err(|error| {
                map_read_error(error.as_service_error().and_then(|value| value.code()))
            })?;
        let mut items = Vec::new();
        for summary in output.ip_sets() {
            let id = AwsWafIpSetId::new(
                summary
                    .id()
                    .ok_or_else(|| unknown("aws_waf_ip_set_id_missing"))?,
            )
            .map_err(|_| validation("invalid_aws_waf_ip_set_id"))?;
            let name = summary
                .name()
                .ok_or_else(|| unknown("aws_waf_ip_set_list_name_missing"))?;
            items.push(
                self.get_ip_set_named(scope, &id, name)
                    .await?
                    .ok_or_else(|| unknown("aws_waf_ip_set_list_get_race"))?,
            );
        }
        Ok(AwsWafIpSetPage {
            items,
            next_marker: output.next_marker().map(str::to_owned),
        })
    }

    async fn get_ip_set(
        &self,
        scope: &AwsWafScope,
        id: &AwsWafIpSetId,
    ) -> AwsWafApiResult<Option<AwsWafIpSet>> {
        let Some(name) = self.resolve_ip_set_name(scope, id).await? else {
            return Ok(None);
        };
        self.get_ip_set_named(scope, id, &name).await
    }

    async fn create_ip_set(&self, value: &AwsWafIpSet) -> AwsWafApiResult<AwsWafIpSet> {
        let mut request = self
            .client(&value.scope, true)
            .create_ip_set()
            .name(&value.name)
            .scope(sdk_scope(&value.scope))
            .ip_address_version(to_ip_version(value.address_version));
        for address in &value.addresses {
            request = request.addresses(address);
        }
        let output = request.send().await.map_err(|error| {
            map_mutation_error(error.as_service_error().and_then(|value| value.code()))
        })?;
        let id = AwsWafIpSetId::new(
            output
                .summary()
                .and_then(|summary| summary.id())
                .ok_or_else(|| unknown("aws_waf_ip_set_create_id_missing"))?,
        )
        .map_err(|_| unknown("aws_waf_ip_set_create_id_malformed"))?;
        self.get_ip_set_named(&value.scope, &id, &value.name)
            .await?
            .ok_or_else(|| unknown("aws_waf_ip_set_create_observation_missing"))
    }

    async fn update_ip_set(
        &self,
        revision: &AwsWafIpSetRevision,
        value: &AwsWafIpSet,
    ) -> AwsWafApiResult<AwsWafIpSet> {
        require_arn_partition(&value.arn, &self.verified_partition)?;
        let mut request = self
            .client(&revision.scope, true)
            .update_ip_set()
            .name(&value.name)
            .id(revision.id.as_str())
            .scope(sdk_scope(&revision.scope))
            .lock_token(revision.lock_token.as_str());
        for address in &value.addresses {
            request = request.addresses(address);
        }
        request.send().await.map_err(|error| {
            map_mutation_error(error.as_service_error().and_then(|value| value.code()))
        })?;
        self.get_ip_set_named(&revision.scope, &revision.id, &value.name)
            .await?
            .ok_or_else(|| unknown("aws_waf_ip_set_update_observation_missing"))
    }

    async fn delete_ip_set(
        &self,
        revision: &AwsWafIpSetRevision,
        name: &str,
    ) -> AwsWafApiResult<()> {
        self.client(&revision.scope, true)
            .delete_ip_set()
            .name(name)
            .id(revision.id.as_str())
            .scope(sdk_scope(&revision.scope))
            .lock_token(revision.lock_token.as_str())
            .send()
            .await
            .map_err(|error| {
                map_mutation_error(error.as_service_error().and_then(|value| value.code()))
            })?;
        Ok(())
    }

    async fn list_managed_rule_groups(
        &self,
        scope: &AwsWafScope,
    ) -> AwsWafApiResult<Vec<AwsWafManagedRuleGroupCatalogEntry>> {
        let mut result = Vec::new();
        let mut marker = None;
        let mut seen_markers = BTreeSet::new();
        let mut seen_groups = BTreeSet::new();
        for _ in 0..MAX_CATALOG_PAGES {
            let groups = self
                .client(scope, false)
                .list_available_managed_rule_groups()
                .scope(sdk_scope(scope))
                .set_next_marker(marker.clone())
                .limit(CATALOG_PAGE_SIZE)
                .send()
                .await
                .map_err(|error| {
                    map_read_error(error.as_service_error().and_then(|value| value.code()))
                })?;
            for group in groups.managed_rule_groups() {
                let vendor_name = group
                    .vendor_name()
                    .ok_or_else(|| validation("aws_waf_managed_group_vendor_missing"))?
                    .to_string();
                let name = group
                    .name()
                    .ok_or_else(|| validation("aws_waf_managed_group_name_missing"))?
                    .to_string();
                if !seen_groups.insert((vendor_name.clone(), name.clone())) {
                    return Err(validation("aws_waf_managed_catalog_duplicate"));
                }
                let mut versions = BTreeSet::new();
                let mut version_marker = None;
                let mut seen_version_markers = BTreeSet::new();
                for _ in 0..MAX_CATALOG_PAGES {
                    let output = self
                        .client(scope, false)
                        .list_available_managed_rule_group_versions()
                        .scope(sdk_scope(scope))
                        .vendor_name(&vendor_name)
                        .name(&name)
                        .set_next_marker(version_marker.clone())
                        .limit(CATALOG_PAGE_SIZE)
                        .send()
                        .await
                        .map_err(|error| {
                            map_read_error(error.as_service_error().and_then(|value| value.code()))
                        })?;
                    for version in output.versions() {
                        versions.insert(
                            version
                                .name()
                                .ok_or_else(|| validation("aws_waf_managed_group_version_missing"))?
                                .to_string(),
                        );
                    }
                    match validated_catalog_marker(output.next_marker(), &mut seen_version_markers)?
                    {
                        Some(next) => version_marker = Some(next),
                        None => break,
                    }
                }
                if version_marker.is_some() {
                    return Err(validation(
                        "aws_waf_managed_catalog_version_pagination_limit",
                    ));
                }
                result.push(AwsWafManagedRuleGroupCatalogEntry {
                    vendor_name,
                    name,
                    versions,
                });
            }
            match validated_catalog_marker(groups.next_marker(), &mut seen_markers)? {
                Some(next) => marker = Some(next),
                None => return Ok(result),
            }
        }
        Err(validation("aws_waf_managed_catalog_pagination_limit"))
    }
    async fn check_capacity(
        &self,
        scope: &AwsWafScope,
        rules: &[AwsWafRule],
    ) -> AwsWafApiResult<u32> {
        let mut request = self
            .client(scope, false)
            .check_capacity()
            .scope(sdk_scope(scope));
        for rule in rules {
            request = request.rules(to_rule(rule)?);
        }
        let output = request.send().await.map_err(|error| {
            map_read_error(error.as_service_error().and_then(|value| value.code()))
        })?;
        u32::try_from(output.capacity()).map_err(|_| validation("invalid_aws_waf_check_capacity"))
    }

    async fn create_web_acl(
        &self,
        request: &AwsWafCreateWebAclRequest,
    ) -> AwsWafApiResult<AwsWafWebAcl> {
        let mut builder = self
            .client(&request.scope, true)
            .create_web_acl()
            .name(&request.name)
            .scope(sdk_scope(&request.scope))
            .default_action(to_default_action(request.default_action))
            .visibility_config(to_visibility(&request.visibility)?);
        for rule in &request.rules {
            builder = builder.rules(to_rule(rule)?);
        }
        let output = builder.send().await.map_err(|error| {
            map_mutation_error(error.as_service_error().and_then(|value| value.code()))
        })?;
        let acl = output
            .summary()
            .ok_or_else(|| unknown("aws_waf_create_summary_missing"))?;
        let id = AwsWafWebAclId::new(
            acl.id()
                .ok_or_else(|| unknown("aws_waf_create_id_missing"))?,
        )
        .map_err(|_| unknown("aws_waf_create_id_malformed"))?;
        self.get_web_acl_named(&request.scope, &id, &request.name)
            .await?
            .ok_or_else(|| unknown("aws_waf_create_observation_missing"))
    }

    async fn update_web_acl(
        &self,
        revision: &AwsWafWebAclRevision,
        desired: &AwsWafWebAcl,
    ) -> AwsWafApiResult<AwsWafWebAcl> {
        require_arn_partition(&desired.arn, &self.verified_partition)?;
        let mut builder = self
            .client(&revision.scope, true)
            .update_web_acl()
            .name(&desired.name)
            .id(revision.id.as_str())
            .scope(sdk_scope(&revision.scope))
            .lock_token(revision.lock_token.as_str())
            .default_action(to_default_action(desired.default_action))
            .visibility_config(to_visibility(&desired.visibility)?);
        for rule in &desired.rules {
            builder = builder.rules(to_rule(rule)?);
        }
        builder.send().await.map_err(|error| {
            map_mutation_error(error.as_service_error().and_then(|value| value.code()))
        })?;
        self.get_web_acl_named(&revision.scope, &revision.id, &desired.name)
            .await?
            .ok_or_else(|| unknown("aws_waf_update_observation_missing"))
    }

    async fn delete_web_acl(&self, revision: &AwsWafWebAclRevision) -> AwsWafApiResult<()> {
        let Some(current) = self.get_web_acl(&revision.scope, &revision.id).await? else {
            return Err(not_found("aws_waf_web_acl_not_found"));
        };
        self.client(&revision.scope, true)
            .delete_web_acl()
            .name(&current.name)
            .id(revision.id.as_str())
            .scope(sdk_scope(&revision.scope))
            .lock_token(revision.lock_token.as_str())
            .send()
            .await
            .map_err(|error| {
                map_mutation_error(error.as_service_error().and_then(|value| value.code()))
            })?;
        Ok(())
    }

    async fn list_associations(
        &self,
        scope: &AwsWafScope,
        web_acl: &AwsWafWebAclId,
    ) -> AwsWafApiResult<Vec<AwsWafAssociation>> {
        if scope.is_cloudfront() {
            return Err(validation("aws_waf_cloudfront_association_deferred"));
        }
        let Some(acl) = self.get_web_acl(scope, web_acl).await? else {
            return Err(not_found("aws_waf_web_acl_not_found"));
        };
        let output = self
            .client(scope, false)
            .list_resources_for_web_acl()
            .web_acl_arn(&acl.arn)
            .send()
            .await
            .map_err(|error| {
                map_read_error(error.as_service_error().and_then(|value| value.code()))
            })?;
        output
            .resource_arns()
            .iter()
            .map(|arn| {
                association_from_arn(
                    arn,
                    web_acl.clone(),
                    scope,
                    &self.account_id,
                    &self.verified_partition,
                )
            })
            .collect()
    }

    async fn associate_regional_resource(
        &self,
        scope: &AwsWafScope,
        target: &AwsWafAssociationTarget,
        web_acl: &AwsWafWebAclId,
    ) -> AwsWafApiResult<()> {
        require_arn_partition(&target.resource_arn, &self.verified_partition)?;
        let Some(acl) = self.get_web_acl(scope, web_acl).await? else {
            return Err(not_found("aws_waf_web_acl_not_found"));
        };
        self.client(scope, true)
            .associate_web_acl()
            .web_acl_arn(acl.arn)
            .resource_arn(&target.resource_arn)
            .send()
            .await
            .map_err(|error| {
                map_mutation_error(error.as_service_error().and_then(|value| value.code()))
            })?;
        Ok(())
    }

    async fn disassociate_regional_resource(
        &self,
        scope: &AwsWafScope,
        target: &AwsWafAssociationTarget,
    ) -> AwsWafApiResult<()> {
        require_arn_partition(&target.resource_arn, &self.verified_partition)?;
        self.client(scope, true)
            .disassociate_web_acl()
            .resource_arn(&target.resource_arn)
            .send()
            .await
            .map_err(|error| {
                map_mutation_error(error.as_service_error().and_then(|value| value.code()))
            })?;
        Ok(())
    }
}

fn timeout_config() -> TimeoutConfig {
    TimeoutConfig::builder()
        .operation_attempt_timeout(ATTEMPT_TIMEOUT)
        .operation_timeout(OPERATION_TIMEOUT)
        .build()
}
fn sdk_scope(scope: &AwsWafScope) -> Scope {
    match scope {
        AwsWafScope::Cloudfront => Scope::Cloudfront,
        AwsWafScope::Regional { .. } => Scope::Regional,
    }
}
fn to_visibility(value: &AwsWafVisibilityConfig) -> AwsWafApiResult<VisibilityConfig> {
    VisibilityConfig::builder()
        .cloud_watch_metrics_enabled(value.cloudwatch_metrics_enabled)
        .sampled_requests_enabled(value.sampled_requests_enabled)
        .metric_name(&value.metric_name)
        .build()
        .map_err(|_| validation("invalid_aws_waf_visibility"))
}
fn to_default_action(value: AwsWafDefaultAction) -> DefaultAction {
    match value {
        AwsWafDefaultAction::Allow => DefaultAction::builder()
            .allow(AllowAction::builder().build())
            .build(),
        AwsWafDefaultAction::Block => DefaultAction::builder()
            .block(BlockAction::builder().build())
            .build(),
    }
}
fn to_ip_version(value: AwsWafIpAddressVersion) -> IpAddressVersion {
    match value {
        AwsWafIpAddressVersion::Ipv4 => IpAddressVersion::Ipv4,
        AwsWafIpAddressVersion::Ipv6 => IpAddressVersion::Ipv6,
    }
}
fn from_sdk_ip_set(
    value: &aws_sdk_wafv2::types::IpSet,
    token: &str,
    scope: &AwsWafScope,
    account: &str,
) -> AwsWafApiResult<AwsWafIpSet> {
    if value
        .description()
        .is_some_and(|description| !description.is_empty())
    {
        // UpdateIPSet cannot preserve description through the bounded model.
        return Err(validation("unsupported_aws_waf_ip_set_variant"));
    }
    let address_version = match value.ip_address_version() {
        IpAddressVersion::Ipv4 => AwsWafIpAddressVersion::Ipv4,
        IpAddressVersion::Ipv6 => AwsWafIpAddressVersion::Ipv6,
        _ => return Err(validation("invalid_aws_waf_ip_set_version")),
    };
    let result = AwsWafIpSet {
        id: AwsWafIpSetId::new(value.id()).map_err(|_| validation("invalid_aws_waf_ip_set_id"))?,
        name: value.name().to_string(),
        arn: value.arn().to_string(),
        scope: scope.clone(),
        address_version,
        addresses: value.addresses().iter().cloned().collect(),
        lock_token: AwsWafLockToken::new(token)
            .map_err(|_| validation("invalid_aws_waf_lock_token"))?,
    };
    result.validate(account)?;
    Ok(result)
}
fn to_action(value: AwsWafAction) -> RuleAction {
    match value {
        AwsWafAction::Allow => RuleAction::builder()
            .allow(AllowAction::builder().build())
            .build(),
        AwsWafAction::Block => RuleAction::builder()
            .block(BlockAction::builder().build())
            .build(),
        AwsWafAction::Count => RuleAction::builder()
            .count(CountAction::builder().build())
            .build(),
        AwsWafAction::Challenge => RuleAction::builder()
            .challenge(ChallengeAction::builder().build())
            .build(),
        AwsWafAction::Captcha => RuleAction::builder()
            .captcha(CaptchaAction::builder().build())
            .build(),
    }
}
fn to_rule(value: &AwsWafRule) -> AwsWafApiResult<Rule> {
    let mut builder = Rule::builder()
        .name(&value.name)
        .priority(
            i32::try_from(value.priority)
                .map_err(|_| validation("aws_waf_rule_priority_overflow"))?,
        )
        .statement(to_statement(&value.statement)?)
        .visibility_config(to_visibility(&value.visibility)?);
    if matches!(value.statement, AwsWafStatement::ManagedRuleGroup(_)) {
        builder = builder.override_action(to_override_action(
            value
                .managed_override_action
                .ok_or_else(|| validation("aws_waf_managed_rule_override_missing"))?,
        ));
    } else {
        builder = builder.action(to_action(value.action));
    }
    builder
        .build()
        .map_err(|_| validation("invalid_aws_waf_rule"))
}

fn to_override_action(value: AwsWafManagedRuleOverrideAction) -> OverrideAction {
    match value {
        AwsWafManagedRuleOverrideAction::None => OverrideAction::builder()
            .none(aws_sdk_wafv2::types::NoneAction::builder().build())
            .build(),
        AwsWafManagedRuleOverrideAction::Count => OverrideAction::builder()
            .count(CountAction::builder().build())
            .build(),
    }
}
fn to_statement(value: &AwsWafStatement) -> AwsWafApiResult<Statement> {
    match value {
        AwsWafStatement::ManagedRuleGroup(group) => {
            let mut builder = ManagedRuleGroupStatement::builder()
                .vendor_name(&group.vendor_name)
                .name(&group.name);
            if let Some(version) = &group.version {
                builder = builder.version(version);
            }
            for name in &group.excluded_rules {
                builder = builder.excluded_rules(
                    ExcludedRule::builder()
                        .name(name)
                        .build()
                        .map_err(|_| validation("invalid_aws_waf_excluded_rule"))?,
                );
            }
            for override_value in &group.rule_action_overrides {
                builder = builder.rule_action_overrides(to_rule_action_override(override_value)?);
            }
            let statement = builder
                .build()
                .map_err(|_| validation("invalid_aws_waf_managed_rule_group"))?;
            Ok(Statement::builder()
                .managed_rule_group_statement(statement)
                .build())
        }
        AwsWafStatement::IpSetReference(ip) => Ok(Statement::builder()
            .ip_set_reference_statement(
                IpSetReferenceStatement::builder()
                    .arn(&ip.arn)
                    .build()
                    .map_err(|_| validation("invalid_aws_waf_ip_set"))?,
            )
            .build()),
        AwsWafStatement::RateBased {
            limit,
            aggregate_key: AwsWafRateAggregateKey::Ip,
            scope_down_ip_set,
        } => {
            let mut builder = RateBasedStatement::builder()
                .limit(i64::from(*limit))
                .aggregate_key_type(RateBasedStatementAggregateKeyType::Ip);
            if let Some(ip) = scope_down_ip_set {
                let inner = Statement::builder()
                    .ip_set_reference_statement(
                        IpSetReferenceStatement::builder()
                            .arn(&ip.arn)
                            .build()
                            .map_err(|_| validation("invalid_aws_waf_ip_set"))?,
                    )
                    .build();
                builder = builder.scope_down_statement(inner);
            }
            let statement = builder
                .build()
                .map_err(|_| validation("invalid_aws_waf_rate_rule"))?;
            Ok(Statement::builder().rate_based_statement(statement).build())
        }
    }
}
fn from_sdk_acl(
    value: &aws_sdk_wafv2::types::WebAcl,
    token: &str,
    scope: &AwsWafScope,
    account: &str,
) -> AwsWafApiResult<AwsWafWebAcl> {
    // UpdateWebACL replaces the ACL's configuration rather than patching it.
    // This bounded transport deliberately emits only the fields represented in
    // `AwsWafWebAcl`; accepting any of these values would erase them on a
    // subsequent Center write.
    if value
        .description()
        .is_some_and(|description| !description.is_empty())
        || value.data_protection_config().is_some()
        || !value.pre_process_firewall_manager_rule_groups().is_empty()
        || !value.post_process_firewall_manager_rule_groups().is_empty()
        || value.managed_by_firewall_manager()
        || value
            .custom_response_bodies()
            .is_some_and(|bodies| !bodies.is_empty())
        || value.captcha_config().is_some()
        || value.challenge_config().is_some()
        || !value.token_domains().is_empty()
        || value.association_config().is_some()
        || value.retrofitted_by_firewall_manager()
        || value.on_source_d_do_s_protection_config().is_some()
        || value.application_config().is_some()
    {
        return Err(validation("unsupported_aws_waf_web_acl_variant"));
    }
    let rules = value
        .rules()
        .iter()
        .map(from_sdk_rule)
        .collect::<AwsWafApiResult<Vec<_>>>()?;
    let visibility = value
        .visibility_config()
        .ok_or_else(|| validation("aws_waf_acl_visibility_missing"))?;
    let default = value
        .default_action()
        .ok_or_else(|| validation("aws_waf_default_action_missing"))?;
    let default_action = match (default.allow(), default.block()) {
        (Some(allow), None) if allow.custom_request_handling().is_none() => {
            AwsWafDefaultAction::Allow
        }
        (None, Some(block)) if block.custom_response().is_none() => AwsWafDefaultAction::Block,
        _ => return Err(validation("invalid_aws_waf_default_action")),
    };
    let capacity =
        u32::try_from(value.capacity()).map_err(|_| validation("invalid_aws_waf_capacity"))?;
    let acl = AwsWafWebAcl {
        id: AwsWafWebAclId::new(value.id())
            .map_err(|_| validation("invalid_aws_waf_web_acl_id"))?,
        name: value.name().to_string(),
        arn: value.arn().to_string(),
        scope: scope.clone(),
        default_action,
        visibility: AwsWafVisibilityConfig {
            cloudwatch_metrics_enabled: visibility.cloud_watch_metrics_enabled(),
            sampled_requests_enabled: visibility.sampled_requests_enabled(),
            metric_name: visibility.metric_name().to_string(),
        },
        rules,
        capacity,
        lock_token: AwsWafLockToken::new(token)
            .map_err(|_| validation("invalid_aws_waf_lock_token"))?,
    };
    acl.validate(
        account,
        &AwsWafCapabilityEvidence {
            managed_rule_groups: acl
                .rules
                .iter()
                .filter_map(|rule| match &rule.statement {
                    AwsWafStatement::ManagedRuleGroup(group) => Some(group.key()),
                    _ => None,
                })
                .collect(),
            challenge_allowed: true,
            captcha_allowed: true,
            maximum_wcu: acl.capacity.max(1),
        },
    )?;
    Ok(acl)
}
fn from_sdk_rule(value: &Rule) -> AwsWafApiResult<AwsWafRule> {
    if !value.rule_labels().is_empty()
        || value.captcha_config().is_some()
        || value.challenge_config().is_some()
    {
        return Err(validation("unsupported_aws_waf_rule_variant"));
    }
    let statement = value
        .statement()
        .ok_or_else(|| validation("aws_waf_rule_statement_missing"))?;
    let (statement, action, managed_override_action) =
        if let Some(group) = statement.managed_rule_group_statement() {
            if value.action().is_some() {
                return Err(validation("invalid_aws_waf_managed_rule_action"));
            }
            (
                AwsWafStatement::ManagedRuleGroup(from_sdk_managed_rule_group(group)?),
                // `action` remains the legacy rule DTO field. The exact managed
                // action is retained separately in `managed_override_action`.
                AwsWafAction::Count,
                Some(from_sdk_override_action(
                    value
                        .override_action()
                        .ok_or_else(|| validation("aws_waf_managed_rule_override_missing"))?,
                )?),
            )
        } else if let Some(ip) = statement.ip_set_reference_statement() {
            if ip.ip_set_forwarded_ip_config().is_some() {
                return Err(validation("unsupported_aws_waf_ip_set_reference_variant"));
            }
            (
                AwsWafStatement::IpSetReference(AwsWafIpSetReference {
                    arn: ip.arn().to_string(),
                }),
                from_sdk_normal_rule_action(value)?,
                None,
            )
        } else if let Some(rate) = statement.rate_based_statement() {
            if rate.aggregate_key_type() != &RateBasedStatementAggregateKeyType::Ip
                || rate.evaluation_window_sec() != 300
                || rate.forwarded_ip_config().is_some()
                || !rate.custom_keys().is_empty()
            {
                return Err(validation("unsupported_aws_waf_rate_rule_variant"));
            }
            let scope_down_ip_set = match rate.scope_down_statement() {
                None => None,
                Some(statement) => {
                    let ip = statement
                        .ip_set_reference_statement()
                        .filter(|ip| ip.ip_set_forwarded_ip_config().is_none())
                        .ok_or_else(|| validation("unsupported_aws_waf_rate_scope_down"))?;
                    Some(AwsWafIpSetReference {
                        arn: ip.arn().to_string(),
                    })
                }
            };
            (
                AwsWafStatement::RateBased {
                    limit: u32::try_from(rate.limit())
                        .map_err(|_| validation("invalid_aws_waf_rate_limit"))?,
                    aggregate_key: AwsWafRateAggregateKey::Ip,
                    scope_down_ip_set,
                },
                from_sdk_normal_rule_action(value)?,
                None,
            )
        } else {
            return Err(validation("unsupported_aws_waf_statement"));
        };
    if !matches!(statement, AwsWafStatement::ManagedRuleGroup(_))
        && value.override_action().is_some()
    {
        return Err(validation("invalid_aws_waf_rule_override_action"));
    }
    let visibility = value
        .visibility_config()
        .ok_or_else(|| validation("aws_waf_rule_visibility_missing"))?;
    Ok(AwsWafRule {
        name: value.name().to_string(),
        priority: u32::try_from(value.priority())
            .map_err(|_| validation("invalid_aws_waf_rule_priority"))?,
        action,
        statement,
        visibility: AwsWafVisibilityConfig {
            cloudwatch_metrics_enabled: visibility.cloud_watch_metrics_enabled(),
            sampled_requests_enabled: visibility.sampled_requests_enabled(),
            metric_name: visibility.metric_name().to_string(),
        },
        owner: AwsWafRuleOwner::External,
        managed_override_action,
    })
}

fn from_sdk_normal_rule_action(value: &Rule) -> AwsWafApiResult<AwsWafAction> {
    value
        .action()
        .map(from_sdk_action)
        .transpose()?
        .ok_or_else(|| validation("aws_waf_rule_action_missing"))
}

fn from_sdk_override_action(
    value: &OverrideAction,
) -> AwsWafApiResult<AwsWafManagedRuleOverrideAction> {
    match (value.none(), value.count()) {
        (Some(_), None) => Ok(AwsWafManagedRuleOverrideAction::None),
        (None, Some(count)) if count.custom_request_handling().is_none() => {
            Ok(AwsWafManagedRuleOverrideAction::Count)
        }
        _ => Err(validation("invalid_aws_waf_managed_rule_override")),
    }
}

fn to_rule_action_override(
    value: &AwsWafManagedRuleOverride,
) -> AwsWafApiResult<RuleActionOverride> {
    RuleActionOverride::builder()
        .name(&value.name)
        .action_to_use(to_action(value.action))
        .build()
        .map_err(|_| validation("invalid_aws_waf_rule_action_override"))
}

/// Only map managed-rule-group variants that this bounded model can replay
/// byte-for-byte in provider meaning. A scope-down statement or managed-group
/// config would be lost by a later full WebACL update, so inventory fails
/// closed instead of silently removing it.
fn from_sdk_managed_rule_group(
    group: &ManagedRuleGroupStatement,
) -> AwsWafApiResult<AwsWafManagedRuleGroup> {
    if group.scope_down_statement().is_some() || !group.managed_rule_group_configs().is_empty() {
        return Err(validation("unsupported_aws_waf_managed_rule_group_variant"));
    }
    let excluded_rules = group
        .excluded_rules()
        .iter()
        .map(|value| value.name().to_string())
        .collect();
    let rule_action_overrides = group
        .rule_action_overrides()
        .iter()
        .map(|value| {
            Ok(AwsWafManagedRuleOverride {
                name: value.name().to_string(),
                action: from_sdk_action(
                    value
                        .action_to_use()
                        .ok_or_else(|| validation("aws_waf_rule_action_override_missing"))?,
                )?,
            })
        })
        .collect::<AwsWafApiResult<Vec<_>>>()?;
    Ok(AwsWafManagedRuleGroup {
        vendor_name: group.vendor_name().to_string(),
        name: group.name().to_string(),
        version: group.version().map(str::to_owned),
        // AWS exposes authoritative Web ACL capacity, not a per-managed-group
        // split. `AwsWafWebAcl::validate` consequently treats any ACL with a
        // managed group as provider-capacity authoritative.
        capacity: 1,
        excluded_rules,
        rule_action_overrides,
    })
}
fn from_sdk_action(value: &RuleAction) -> AwsWafApiResult<AwsWafAction> {
    match (
        value.allow(),
        value.block(),
        value.count(),
        value.challenge(),
        value.captcha(),
    ) {
        (Some(allow), None, None, None, None) if allow.custom_request_handling().is_none() => {
            Ok(AwsWafAction::Allow)
        }
        (None, Some(block), None, None, None) if block.custom_response().is_none() => {
            Ok(AwsWafAction::Block)
        }
        (None, None, Some(count), None, None) if count.custom_request_handling().is_none() => {
            Ok(AwsWafAction::Count)
        }
        (None, None, None, Some(challenge), None)
            if challenge.custom_request_handling().is_none() =>
        {
            Ok(AwsWafAction::Challenge)
        }
        (None, None, None, None, Some(captcha)) if captcha.custom_request_handling().is_none() => {
            Ok(AwsWafAction::Captcha)
        }
        _ => Err(validation("invalid_aws_waf_rule_action")),
    }
}
fn association_from_arn(
    arn: &str,
    web_acl_id: AwsWafWebAclId,
    scope: &AwsWafScope,
    account: &str,
    partition: &str,
) -> AwsWafApiResult<AwsWafAssociation> {
    require_arn_partition(arn, partition)?;
    let service = arn
        .split(':')
        .nth(2)
        .ok_or_else(|| validation("invalid_aws_waf_association_arn"))?;
    let resource_kind = match service {
        "elasticloadbalancing" => AwsWafRegionalResourceKind::ApplicationLoadBalancer,
        "apigateway" => AwsWafRegionalResourceKind::ApiGatewayStage,
        "appsync" => AwsWafRegionalResourceKind::AppSyncApi,
        "cognito-idp" => AwsWafRegionalResourceKind::CognitoUserPool,
        _ => return Err(validation("unsupported_aws_waf_association_target")),
    };
    let association = AwsWafAssociation {
        target: AwsWafAssociationTarget {
            resource_arn: arn.to_string(),
            resource_kind,
        },
        web_acl_id,
    };
    association.target.validate(scope, account)?;
    Ok(association)
}
fn is_aws_account_id(value: &str) -> bool {
    value.len() == 12 && value.bytes().all(|byte| byte.is_ascii_digit())
}
fn aws_arn_partition(arn: &str) -> Option<&str> {
    let mut parts = arn.split(':');
    match (parts.next(), parts.next()) {
        (Some("arn"), Some(partition @ ("aws" | "aws-cn" | "aws-us-gov"))) => Some(partition),
        _ => None,
    }
}

fn require_arn_partition(arn: &str, expected: &str) -> AwsWafApiResult<()> {
    if aws_arn_partition(arn) == Some(expected) {
        Ok(())
    } else {
        Err(validation("aws_waf_arn_partition_mismatch"))
    }
}
fn validate_test_endpoint(value: &str) -> AwsWafApiResult<()> {
    if value.starts_with("http://127.0.0.1")
        || value.starts_with("http://[::1]")
        || value.starts_with("https://")
    {
        Ok(())
    } else {
        Err(validation("invalid_aws_waf_test_endpoint"))
    }
}

fn validated_catalog_marker(
    marker: Option<&str>,
    seen: &mut BTreeSet<String>,
) -> AwsWafApiResult<Option<String>> {
    match marker {
        None => Ok(None),
        Some(value)
            if value.is_empty() || value.len() > 1_024 || !seen.insert(value.to_string()) =>
        {
            Err(validation("invalid_aws_waf_managed_catalog_marker"))
        }
        Some(value) => Ok(Some(value.to_string())),
    }
}
fn map_read_error(code: Option<&str>) -> NormalizedProviderError {
    map_error(code, false)
}
fn map_mutation_error(code: Option<&str>) -> NormalizedProviderError {
    map_error(code, true)
}
fn map_error(code: Option<&str>, mutation: bool) -> NormalizedProviderError {
    let code = code.unwrap_or("aws_waf_sdk_error");
    let category = match code {
        "WAFOptimisticLockException" | "WAFUnavailableEntityException" => {
            ProviderErrorCategory::Conflict
        }
        "WAFInvalidParameterException" => ProviderErrorCategory::Validation,
        "WAFNonexistentItemException" => ProviderErrorCategory::NotFound,
        "WAFServiceLimitExceededException" | "WAFLimitsExceededException" => {
            ProviderErrorCategory::Quota
        }
        "Throttling" | "ThrottlingException" => ProviderErrorCategory::Throttled,
        "WAFInvalidOperationException" | "WAFInvalidPermissionPolicyException" => {
            ProviderErrorCategory::Authorization
        }
        "WAFInternalErrorException" => {
            if mutation {
                ProviderErrorCategory::UnknownOutcome
            } else {
                ProviderErrorCategory::Transient
            }
        }
        _ => {
            if mutation {
                ProviderErrorCategory::UnknownOutcome
            } else {
                ProviderErrorCategory::Transient
            }
        }
    };
    NormalizedProviderError::new(
        category,
        code,
        "Sanitized AWS WAF SDK error",
        (category == ProviderErrorCategory::Throttled).then_some(DEFAULT_THROTTLE_RETRY_AFTER_MS),
        None,
    )
    .expect("fixed normalized AWS error")
}
fn validation(code: &str) -> NormalizedProviderError {
    NormalizedProviderError::new(
        ProviderErrorCategory::Validation,
        code,
        "Sanitized AWS WAF SDK error",
        None,
        None,
    )
    .expect("fixed validation error")
}
fn unknown(code: &str) -> NormalizedProviderError {
    NormalizedProviderError::new(
        ProviderErrorCategory::UnknownOutcome,
        code,
        "Sanitized AWS WAF SDK error",
        None,
        None,
    )
    .expect("fixed unknown outcome")
}
fn not_found(code: &str) -> NormalizedProviderError {
    NormalizedProviderError::new(
        ProviderErrorCategory::NotFound,
        code,
        "Sanitized AWS WAF SDK error",
        None,
        None,
    )
    .expect("fixed not found")
}

#[cfg(test)]
mod sdk_mapping_tests {
    use super::*;

    #[test]
    fn throttling_has_bounded_retry_evidence_and_mutations_are_single_attempt() {
        let read = map_read_error(Some("ThrottlingException"));
        assert_eq!(read.category(), ProviderErrorCategory::Throttled);
        assert_eq!(read.retry_after_ms(), Some(DEFAULT_THROTTLE_RETRY_AFTER_MS));
        let mutation = map_mutation_error(Some("Throttling"));
        assert_eq!(mutation.category(), ProviderErrorCategory::Throttled);
        assert_eq!(MUTATION_MAX_ATTEMPTS, 1);
    }

    fn visibility() -> VisibilityConfig {
        VisibilityConfig::builder()
            .cloud_watch_metrics_enabled(true)
            .sampled_requests_enabled(false)
            .metric_name("metric")
            .build()
            .unwrap()
    }

    fn managed_rule(override_action: OverrideAction) -> Rule {
        let group = ManagedRuleGroupStatement::builder()
            .vendor_name("AWS")
            .name("AWSManagedRulesCommonRuleSet")
            .excluded_rules(
                ExcludedRule::builder()
                    .name("NoUserAgent_HEADER")
                    .build()
                    .unwrap(),
            )
            .rule_action_overrides(
                RuleActionOverride::builder()
                    .name("SizeRestrictions_BODY")
                    .action_to_use(to_action(AwsWafAction::Count))
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        Rule::builder()
            .name("managed")
            .priority(1)
            .statement(
                Statement::builder()
                    .managed_rule_group_statement(group)
                    .build(),
            )
            .override_action(override_action)
            .visibility_config(visibility())
            .build()
            .unwrap()
    }

    #[test]
    fn managed_group_exceptions_and_top_level_override_round_trip() {
        let source = managed_rule(to_override_action(AwsWafManagedRuleOverrideAction::Count));
        let mapped = from_sdk_rule(&source).unwrap();
        assert_eq!(
            mapped.managed_override_action,
            Some(AwsWafManagedRuleOverrideAction::Count)
        );
        let AwsWafStatement::ManagedRuleGroup(group) = &mapped.statement else {
            panic!("managed")
        };
        assert!(group.excluded_rules.contains("NoUserAgent_HEADER"));
        assert_eq!(group.rule_action_overrides[0].action, AwsWafAction::Count);
        let replayed = to_rule(&mapped).unwrap();
        assert!(replayed.override_action().unwrap().count().is_some());
        let replayed_group = replayed
            .statement()
            .unwrap()
            .managed_rule_group_statement()
            .unwrap();
        assert_eq!(
            replayed_group.excluded_rules()[0].name(),
            "NoUserAgent_HEADER"
        );
        assert_eq!(
            replayed_group.rule_action_overrides()[0].name(),
            "SizeRestrictions_BODY"
        );
    }

    #[test]
    fn unsupported_optional_rule_state_and_custom_action_fail_closed() {
        let custom_handling = aws_sdk_wafv2::types::CustomRequestHandling::builder()
            .insert_headers(
                aws_sdk_wafv2::types::CustomHttpHeader::builder()
                    .name("x-test")
                    .value("1")
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        let rule = Rule::builder()
            .name("custom")
            .priority(1)
            .statement(
                Statement::builder()
                    .rate_based_statement(
                        RateBasedStatement::builder()
                            .limit(100)
                            .evaluation_window_sec(300)
                            .aggregate_key_type(RateBasedStatementAggregateKeyType::Ip)
                            .build()
                            .unwrap(),
                    )
                    .build(),
            )
            .action(
                RuleAction::builder()
                    .allow(
                        AllowAction::builder()
                            .custom_request_handling(custom_handling)
                            .build(),
                    )
                    .build(),
            )
            .visibility_config(visibility())
            .build()
            .unwrap();
        assert_eq!(
            from_sdk_rule(&rule).unwrap_err().code(),
            "invalid_aws_waf_rule_action"
        );
    }

    #[test]
    fn full_replacement_acl_with_unrepresented_state_is_rejected() {
        let acl = aws_sdk_wafv2::types::WebAcl::builder()
            .name("acl")
            .id("acl-id")
            .arn("arn:aws:wafv2:us-west-2:123456789012:regional/webacl/acl/acl-id")
            .default_action(to_default_action(AwsWafDefaultAction::Block))
            .visibility_config(visibility())
            .capacity(1)
            .description("must not be erased")
            .build()
            .unwrap();
        let error = from_sdk_acl(
            &acl,
            "token",
            &AwsWafScope::Regional {
                region: "us-west-2".to_string(),
            },
            "123456789012",
        )
        .unwrap_err();
        assert_eq!(error.code(), "unsupported_aws_waf_web_acl_variant");
    }

    #[test]
    fn empty_provider_description_is_equivalent_to_unset() {
        let acl = aws_sdk_wafv2::types::WebAcl::builder()
            .name("acl")
            .id("acl-id")
            .arn("arn:aws:wafv2:us-west-2:123456789012:regional/webacl/acl/acl-id")
            .default_action(to_default_action(AwsWafDefaultAction::Block))
            .visibility_config(visibility())
            .capacity(0)
            .description("")
            .build()
            .unwrap();
        assert!(from_sdk_acl(
            &acl,
            "token",
            &AwsWafScope::Regional {
                region: "us-west-2".to_string()
            },
            "123456789012",
        )
        .is_ok());
    }

    #[test]
    fn association_projection_rejects_foreign_account_and_region() {
        let scope = AwsWafScope::Regional {
            region: "us-west-2".to_string(),
        };
        let id = AwsWafWebAclId::new("acl-id").unwrap();
        for arn in [
            "arn:aws:elasticloadbalancing:us-west-2:999999999999:loadbalancer/app/test/123",
            "arn:aws:elasticloadbalancing:us-east-1:123456789012:loadbalancer/app/test/123",
        ] {
            assert_eq!(
                association_from_arn(arn, id.clone(), &scope, "123456789012", "aws")
                    .unwrap_err()
                    .code(),
                "invalid_aws_waf_regional_association_target"
            );
        }
    }

    #[test]
    fn partition_drift_is_rejected_before_provider_dispatch() {
        assert_eq!(
            require_arn_partition(
                "arn:aws-cn:elasticloadbalancing:cn-north-1:123456789012:loadbalancer/app/test/123",
                "aws",
            )
            .unwrap_err()
            .code(),
            "aws_waf_arn_partition_mismatch"
        );
    }
}
