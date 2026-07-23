//! AWS WAFv2 Admin boundary.
//!
//! Provider credentials and SDK objects remain behind composition. This module
//! deliberately has no arbitrary WAF statement/action JSON request field.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    extract::{rejection::JsonRejection, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use edgion_center_core::CloudResourceId;
use serde::{Deserialize, Serialize};

use super::ApiState;
use crate::common::api::ApiResponse;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AwsWafScopeDto {
    Cloudfront,
    Regional { region: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsWafWebAclDto {
    pub id: String,
    pub name: String,
    pub arn: String,
    pub scope: AwsWafScopeDto,
    pub capacity: u32,
    pub lock_token_present: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsWafIpSetDto {
    pub id: String,
    pub name: String,
    pub arn: String,
    pub scope: AwsWafScopeDto,
    pub address_version: String,
    pub addresses: Vec<String>,
    pub lock_token: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsWafCatalogDto {
    pub vendor_name: String,
    pub name: String,
    pub versions: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsWafCapacityDto {
    pub required_wcu: u32,
    pub allowed: bool,
    pub reason: String,
}

/// Sanitized WAF visibility configuration. Metric names are bounded identifiers;
/// raw CloudWatch configuration and provider objects never cross this boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsWafVisibilityDto {
    pub cloudwatch_metrics_enabled: bool,
    pub sampled_requests_enabled: bool,
    pub metric_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AwsWafActionDto {
    Allow,
    Block,
    Count,
    Challenge,
    Captcha,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AwsWafDefaultActionDto {
    Allow,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AwsWafAddressVersionDto {
    Ipv4,
    Ipv6,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsWafManagedRuleOverrideDto {
    pub name: String,
    pub action: AwsWafActionDto,
}

/// Closed, provider-safe WAF statement family. There is intentionally no
/// `raw`, expression, JSON, or arbitrary provider action variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AwsWafStatementDto {
    ManagedRuleGroup {
        vendor_name: String,
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        version: Option<String>,
        #[serde(default)]
        excluded_rules: Vec<String>,
        #[serde(default)]
        rule_action_overrides: Vec<AwsWafManagedRuleOverrideDto>,
    },
    IpSetReference {
        arn: String,
    },
    RateBased {
        limit: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        scope_down_ip_set: Option<AwsWafIpSetReferenceDto>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsWafIpSetReferenceDto {
    pub arn: String,
}

/// The response records observed ownership; mutation requests deliberately do
/// not contain this field. The service derives Center ownership from its
/// trusted request reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AwsWafRuleOwnershipDto {
    CenterOwned,
    External,
}

/// The top-level action for a managed rule group. It is intentionally distinct
/// from `AwsWafActionDto`: AWS accepts only `None` or `Count` here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AwsWafManagedRuleOverrideActionDto {
    None,
    Count,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsWafRuleDto {
    pub name: String,
    pub priority: u32,
    /// Managed rule groups use `managedOverrideAction` instead of a normal
    /// rule action, so this is absent for those rules.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<AwsWafActionDto>,
    pub statement: AwsWafStatementDto,
    pub visibility: AwsWafVisibilityDto,
    pub ownership: AwsWafRuleOwnershipDto,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub managed_override_action: Option<AwsWafManagedRuleOverrideActionDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsWafWebAclDetailDto {
    pub id: String,
    pub name: String,
    pub arn: String,
    pub scope: AwsWafScopeDto,
    pub default_action: AwsWafDefaultActionDto,
    pub visibility: AwsWafVisibilityDto,
    pub capacity: u32,
    /// Opaque concurrency authority accepted only by guarded WAF mutations.
    /// Request bodies are excluded from audit records.
    pub lock_token: String,
    pub rules: Vec<AwsWafRuleDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsWafAssociationDto {
    pub resource_arn: String,
    pub resource_kind: AwsWafRegionalResourceKindDto,
    pub web_acl_id: String,
    /// CloudFront is controlled through the separate CLD-29A Distribution
    /// endpoint; this value makes that authority explicit in inventory.
    pub target_deployment_authority: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AwsWafRegionalResourceKindDto {
    ApplicationLoadBalancer,
    ApiGatewayStage,
    AppSyncApi,
    CognitoUserPool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsWafWebAclCreateRequest {
    pub name: String,
    pub default_action: AwsWafDefaultActionDto,
    pub visibility: AwsWafVisibilityDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsWafRuleWriteRequest {
    /// Stable Center-owned identity. It is not an owner discriminator and is
    /// never accepted for external provider rules.
    pub reference: String,
    pub lock_token: String,
    pub name: String,
    pub priority: u32,
    #[serde(default)]
    pub action: Option<AwsWafActionDto>,
    /// Required for a managed rule group and restricted to `none` on the
    /// normal create/update route. Moving to `count` uses the security-weaken
    /// route so it cannot be accidental.
    #[serde(default)]
    pub managed_override_action: Option<AwsWafManagedRuleOverrideActionDto>,
    pub statement: AwsWafStatementDto,
    pub visibility: AwsWafVisibilityDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsWafDeleteRequest {
    pub lock_token: String,
    pub confirmation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsWafWebAclUpdateRequest {
    pub lock_token: String,
    pub visibility: AwsWafVisibilityDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsWafWebAclSecurityWeakenRequest {
    pub lock_token: String,
    pub default_action: AwsWafDefaultActionDto,
    pub confirmation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsWafRuleSecurityWeakenRequest {
    pub lock_token: String,
    #[serde(default)]
    pub action: Option<AwsWafActionDto>,
    #[serde(default)]
    pub managed_override_action: Option<AwsWafManagedRuleOverrideActionDto>,
    pub confirmation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsWafIpSetCreateRequest {
    pub name: String,
    pub address_version: AwsWafAddressVersionDto,
    pub addresses: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsWafIpSetUpdateRequest {
    pub lock_token: String,
    pub addresses: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsWafCapacityRequest {
    pub rules: Vec<AwsWafRuleWriteRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsWafRegionalAssociationRequest {
    pub resource_arn: String,
    pub resource_kind: AwsWafRegionalResourceKindDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsWafRegionalDetachRequest {
    pub resource_arn: String,
    pub resource_kind: AwsWafRegionalResourceKindDto,
    pub confirmation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsWafManagedExceptionRequest {
    pub lock_token: String,
    #[serde(default)]
    pub excluded_rules: Option<Vec<String>>,
    #[serde(default)]
    pub rule_action_overrides: Option<Vec<AwsWafManagedRuleOverrideDto>>,
    pub confirmation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AwsWafAdminError {
    Invalid,
    NotFound,
    Conflict,
    UnknownOutcome,
    Unavailable,
}

#[async_trait]
pub trait AwsWafAdminService: Send + Sync {
    async fn list_web_acls(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
    ) -> Result<Vec<AwsWafWebAclDto>, AwsWafAdminError>;
    async fn list_ip_sets(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
    ) -> Result<Vec<AwsWafIpSetDto>, AwsWafAdminError>;
    async fn managed_catalog(
        &self,
        account: CloudResourceId,
        scope: AwsWafScopeDto,
    ) -> Result<Vec<AwsWafCatalogDto>, AwsWafAdminError>;

    async fn get_web_acl(
        &self,
        _: CloudResourceId,
        _: AwsWafScopeDto,
        _: &str,
    ) -> Result<AwsWafWebAclDetailDto, AwsWafAdminError> {
        Err(AwsWafAdminError::Unavailable)
    }
    async fn create_web_acl(
        &self,
        _: CloudResourceId,
        _: AwsWafScopeDto,
        _: AwsWafWebAclCreateRequest,
    ) -> Result<AwsWafWebAclDetailDto, AwsWafAdminError> {
        Err(AwsWafAdminError::Unavailable)
    }
    /// Updates only non-protective metadata after a fresh provider read and
    /// exact lock comparison. It must reject default-action weakening.
    async fn update_web_acl(
        &self,
        _: CloudResourceId,
        _: AwsWafScopeDto,
        _: &str,
        _: AwsWafWebAclUpdateRequest,
    ) -> Result<AwsWafWebAclDetailDto, AwsWafAdminError> {
        Err(AwsWafAdminError::Unavailable)
    }
    /// Deletes one Web ACL after a fresh lock comparison and proving that it
    /// has no associations. The caller uses the dedicated weaken route.
    async fn delete_web_acl(
        &self,
        _: CloudResourceId,
        _: AwsWafScopeDto,
        _: &str,
        _: AwsWafDeleteRequest,
    ) -> Result<(), AwsWafAdminError> {
        Err(AwsWafAdminError::Unavailable)
    }
    async fn security_weaken_web_acl(
        &self,
        _: CloudResourceId,
        _: AwsWafScopeDto,
        _: &str,
        _: AwsWafWebAclSecurityWeakenRequest,
    ) -> Result<AwsWafWebAclDetailDto, AwsWafAdminError> {
        Err(AwsWafAdminError::Unavailable)
    }
    async fn list_rules(
        &self,
        _: CloudResourceId,
        _: AwsWafScopeDto,
        _: &str,
    ) -> Result<Vec<AwsWafRuleDto>, AwsWafAdminError> {
        Err(AwsWafAdminError::Unavailable)
    }
    async fn create_rule(
        &self,
        _: CloudResourceId,
        _: AwsWafScopeDto,
        _: &str,
        _: AwsWafRuleWriteRequest,
    ) -> Result<AwsWafWebAclDetailDto, AwsWafAdminError> {
        Err(AwsWafAdminError::Unavailable)
    }
    async fn update_rule(
        &self,
        _: CloudResourceId,
        _: AwsWafScopeDto,
        _: &str,
        _: &str,
        _: AwsWafRuleWriteRequest,
    ) -> Result<AwsWafWebAclDetailDto, AwsWafAdminError> {
        Err(AwsWafAdminError::Unavailable)
    }
    async fn delete_rule(
        &self,
        _: CloudResourceId,
        _: AwsWafScopeDto,
        _: &str,
        _: &str,
        _: AwsWafDeleteRequest,
    ) -> Result<AwsWafWebAclDetailDto, AwsWafAdminError> {
        Err(AwsWafAdminError::Unavailable)
    }
    /// Applies a deliberately weaker rule action after a fresh read and exact
    /// lock comparison. Normal `update_rule` must reject the same transition.
    async fn security_weaken_rule(
        &self,
        _: CloudResourceId,
        _: AwsWafScopeDto,
        _: &str,
        _: &str,
        _: AwsWafRuleSecurityWeakenRequest,
    ) -> Result<AwsWafWebAclDetailDto, AwsWafAdminError> {
        Err(AwsWafAdminError::Unavailable)
    }
    async fn apply_managed_exception(
        &self,
        _: CloudResourceId,
        _: AwsWafScopeDto,
        _: &str,
        _: &str,
        _: AwsWafManagedExceptionRequest,
    ) -> Result<AwsWafWebAclDetailDto, AwsWafAdminError> {
        Err(AwsWafAdminError::Unavailable)
    }
    async fn check_capacity(
        &self,
        _: CloudResourceId,
        _: AwsWafScopeDto,
        _: AwsWafCapacityRequest,
    ) -> Result<AwsWafCapacityDto, AwsWafAdminError> {
        Err(AwsWafAdminError::Unavailable)
    }
    async fn create_ip_set(
        &self,
        _: CloudResourceId,
        _: AwsWafScopeDto,
        _: AwsWafIpSetCreateRequest,
    ) -> Result<AwsWafIpSetDto, AwsWafAdminError> {
        Err(AwsWafAdminError::Unavailable)
    }
    async fn update_ip_set(
        &self,
        _: CloudResourceId,
        _: AwsWafScopeDto,
        _: &str,
        _: AwsWafIpSetUpdateRequest,
    ) -> Result<AwsWafIpSetDto, AwsWafAdminError> {
        Err(AwsWafAdminError::Unavailable)
    }
    async fn delete_ip_set(
        &self,
        _: CloudResourceId,
        _: AwsWafScopeDto,
        _: &str,
        _: AwsWafDeleteRequest,
    ) -> Result<(), AwsWafAdminError> {
        Err(AwsWafAdminError::Unavailable)
    }
    async fn list_associations(
        &self,
        _: CloudResourceId,
        _: AwsWafScopeDto,
        _: &str,
    ) -> Result<Vec<AwsWafAssociationDto>, AwsWafAdminError> {
        Err(AwsWafAdminError::Unavailable)
    }
    async fn associate_regional_resource(
        &self,
        _: CloudResourceId,
        _: AwsWafScopeDto,
        _: &str,
        _: AwsWafRegionalAssociationRequest,
    ) -> Result<Vec<AwsWafAssociationDto>, AwsWafAdminError> {
        Err(AwsWafAdminError::Unavailable)
    }
    async fn disassociate_regional_resource(
        &self,
        _: CloudResourceId,
        _: AwsWafScopeDto,
        _: AwsWafRegionalDetachRequest,
    ) -> Result<(), AwsWafAdminError> {
        Err(AwsWafAdminError::Unavailable)
    }
}
pub type SharedAwsWafAdminService = Arc<dyn AwsWafAdminService>;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AwsWafScopeQuery {
    pub region: Option<String>,
}

pub async fn list_web_acls(
    State(state): State<ApiState>,
    Path((account_id, scope)): Path<(String, String)>,
    Query(query): Query<AwsWafScopeQuery>,
) -> Response {
    let Some(service) = state.aws_waf_admin.as_deref() else {
        return error(AwsWafAdminError::Unavailable);
    };
    let Ok(account) = CloudResourceId::new(account_id) else {
        return error(AwsWafAdminError::Invalid);
    };
    let Ok(scope) = parse_scope(&scope, query.region) else {
        return error(AwsWafAdminError::Invalid);
    };
    match service.list_web_acls(account, scope).await {
        Ok(value) => (StatusCode::OK, Json(ApiResponse::ok_body(value))).into_response(),
        Err(value) => error(value),
    }
}
pub async fn list_ip_sets(
    State(state): State<ApiState>,
    Path((account_id, scope)): Path<(String, String)>,
    Query(query): Query<AwsWafScopeQuery>,
) -> Response {
    let Some(service) = state.aws_waf_admin.as_deref() else {
        return error(AwsWafAdminError::Unavailable);
    };
    let Ok(account) = CloudResourceId::new(account_id) else {
        return error(AwsWafAdminError::Invalid);
    };
    let Ok(scope) = parse_scope(&scope, query.region) else {
        return error(AwsWafAdminError::Invalid);
    };
    match service.list_ip_sets(account, scope).await {
        Ok(value) => (StatusCode::OK, Json(ApiResponse::ok_body(value))).into_response(),
        Err(value) => error(value),
    }
}
pub async fn list_managed_catalog(
    State(state): State<ApiState>,
    Path((account_id, scope)): Path<(String, String)>,
    Query(query): Query<AwsWafScopeQuery>,
) -> Response {
    let Some(service) = state.aws_waf_admin.as_deref() else {
        return error(AwsWafAdminError::Unavailable);
    };
    let Ok(account) = CloudResourceId::new(account_id) else {
        return error(AwsWafAdminError::Invalid);
    };
    let Ok(scope) = parse_scope(&scope, query.region) else {
        return error(AwsWafAdminError::Invalid);
    };
    match service.managed_catalog(account, scope).await {
        Ok(value) => (StatusCode::OK, Json(ApiResponse::ok_body(value))).into_response(),
        Err(value) => error(value),
    }
}

pub async fn get_web_acl(
    State(state): State<ApiState>,
    Path((account_id, scope, web_acl_id)): Path<(String, String, String)>,
    Query(query): Query<AwsWafScopeQuery>,
) -> Response {
    let Ok((account, scope)) = account_scope(account_id, scope, query) else {
        return error(AwsWafAdminError::Invalid);
    };
    if !valid_identifier(&web_acl_id) {
        return error(AwsWafAdminError::Invalid);
    }
    let Some(service) = state.aws_waf_admin.as_deref() else {
        return error(AwsWafAdminError::Unavailable);
    };
    match service.get_web_acl(account, scope, &web_acl_id).await {
        Ok(value) => ok(StatusCode::OK, value),
        Err(value) => error(value),
    }
}

pub async fn create_web_acl(
    State(state): State<ApiState>,
    Path((account_id, scope)): Path<(String, String)>,
    Query(query): Query<AwsWafScopeQuery>,
    body: Result<Json<AwsWafWebAclCreateRequest>, JsonRejection>,
) -> Response {
    let Ok((account, scope)) = account_scope(account_id, scope, query) else {
        return error(AwsWafAdminError::Invalid);
    };
    let Ok(Json(request)) = body else {
        return error(AwsWafAdminError::Invalid);
    };
    if !valid_web_acl_create(&request) {
        return error(AwsWafAdminError::Invalid);
    }
    let Some(service) = state.aws_waf_admin.as_deref() else {
        return error(AwsWafAdminError::Unavailable);
    };
    match service.create_web_acl(account, scope, request).await {
        Ok(value) => ok(StatusCode::ACCEPTED, value),
        Err(value) => error(value),
    }
}

pub async fn update_web_acl(
    State(state): State<ApiState>,
    Path((account_id, scope, web_acl_id)): Path<(String, String, String)>,
    Query(query): Query<AwsWafScopeQuery>,
    body: Result<Json<AwsWafWebAclUpdateRequest>, JsonRejection>,
) -> Response {
    let Ok((account, scope)) = account_scope(account_id, scope, query) else {
        return error(AwsWafAdminError::Invalid);
    };
    let Ok(Json(request)) = body else {
        return error(AwsWafAdminError::Invalid);
    };
    if !valid_identifier(&web_acl_id)
        || !valid_lock_token(&request.lock_token)
        || !valid_visibility(&request.visibility)
    {
        return error(AwsWafAdminError::Invalid);
    }
    let Some(service) = state.aws_waf_admin.as_deref() else {
        return error(AwsWafAdminError::Unavailable);
    };
    match service
        .update_web_acl(account, scope, &web_acl_id, request)
        .await
    {
        Ok(value) => ok(StatusCode::ACCEPTED, value),
        Err(value) => error(value),
    }
}

pub async fn security_weaken_web_acl(
    State(state): State<ApiState>,
    Path((account_id, scope, web_acl_id)): Path<(String, String, String)>,
    Query(query): Query<AwsWafScopeQuery>,
    body: Result<Json<AwsWafWebAclSecurityWeakenRequest>, JsonRejection>,
) -> Response {
    let Ok((account, scope)) = account_scope(account_id, scope, query) else {
        return error(AwsWafAdminError::Invalid);
    };
    let Ok(Json(request)) = body else {
        return error(AwsWafAdminError::Invalid);
    };
    if !valid_identifier(&web_acl_id)
        || !valid_lock_token(&request.lock_token)
        || request.confirmation != web_acl_id
    {
        return error(AwsWafAdminError::Invalid);
    }
    let Some(service) = state.aws_waf_admin.as_deref() else {
        return error(AwsWafAdminError::Unavailable);
    };
    match service
        .security_weaken_web_acl(account, scope, &web_acl_id, request)
        .await
    {
        Ok(value) => ok(StatusCode::ACCEPTED, value),
        Err(value) => error(value),
    }
}

pub async fn delete_web_acl(
    State(state): State<ApiState>,
    Path((account_id, scope, web_acl_id)): Path<(String, String, String)>,
    Query(query): Query<AwsWafScopeQuery>,
    body: Result<Json<AwsWafDeleteRequest>, JsonRejection>,
) -> Response {
    let Ok((account, scope)) = account_scope(account_id, scope, query) else {
        return error(AwsWafAdminError::Invalid);
    };
    let Ok(Json(request)) = body else {
        return error(AwsWafAdminError::Invalid);
    };
    if !valid_identifier(&web_acl_id)
        || !valid_lock_token(&request.lock_token)
        || request.confirmation != web_acl_id
    {
        return error(AwsWafAdminError::Invalid);
    }
    let Some(service) = state.aws_waf_admin.as_deref() else {
        return error(AwsWafAdminError::Unavailable);
    };
    match service
        .delete_web_acl(account, scope, &web_acl_id, request)
        .await
    {
        Ok(()) => ok(StatusCode::NO_CONTENT, ()),
        Err(value) => error(value),
    }
}

pub async fn list_rules(
    State(state): State<ApiState>,
    Path((account_id, scope, web_acl_id)): Path<(String, String, String)>,
    Query(query): Query<AwsWafScopeQuery>,
) -> Response {
    let Ok((account, scope)) = account_scope(account_id, scope, query) else {
        return error(AwsWafAdminError::Invalid);
    };
    if !valid_identifier(&web_acl_id) {
        return error(AwsWafAdminError::Invalid);
    }
    let Some(service) = state.aws_waf_admin.as_deref() else {
        return error(AwsWafAdminError::Unavailable);
    };
    match service.list_rules(account, scope, &web_acl_id).await {
        Ok(value) => ok(StatusCode::OK, value),
        Err(value) => error(value),
    }
}

pub async fn create_rule(
    State(state): State<ApiState>,
    Path((account_id, scope, web_acl_id)): Path<(String, String, String)>,
    Query(query): Query<AwsWafScopeQuery>,
    body: Result<Json<AwsWafRuleWriteRequest>, JsonRejection>,
) -> Response {
    let Ok((account, scope)) = account_scope(account_id, scope, query) else {
        return error(AwsWafAdminError::Invalid);
    };
    let Ok(Json(request)) = body else {
        return error(AwsWafAdminError::Invalid);
    };
    if !valid_identifier(&web_acl_id) || !valid_rule_write(&request) {
        return error(AwsWafAdminError::Invalid);
    }
    let Some(service) = state.aws_waf_admin.as_deref() else {
        return error(AwsWafAdminError::Unavailable);
    };
    match service
        .create_rule(account, scope, &web_acl_id, request)
        .await
    {
        Ok(value) => ok(StatusCode::ACCEPTED, value),
        Err(value) => error(value),
    }
}

pub async fn update_rule(
    State(state): State<ApiState>,
    Path((account_id, scope, web_acl_id, reference)): Path<(String, String, String, String)>,
    Query(query): Query<AwsWafScopeQuery>,
    body: Result<Json<AwsWafRuleWriteRequest>, JsonRejection>,
) -> Response {
    let Ok((account, scope)) = account_scope(account_id, scope, query) else {
        return error(AwsWafAdminError::Invalid);
    };
    let Ok(Json(request)) = body else {
        return error(AwsWafAdminError::Invalid);
    };
    if !valid_identifier(&web_acl_id)
        || !valid_identifier(&reference)
        || request.reference != reference
        || !valid_rule_write(&request)
    {
        return error(AwsWafAdminError::Invalid);
    }
    let Some(service) = state.aws_waf_admin.as_deref() else {
        return error(AwsWafAdminError::Unavailable);
    };
    match service
        .update_rule(account, scope, &web_acl_id, &reference, request)
        .await
    {
        Ok(value) => ok(StatusCode::ACCEPTED, value),
        Err(value) => error(value),
    }
}

pub async fn delete_rule(
    State(state): State<ApiState>,
    Path((account_id, scope, web_acl_id, reference)): Path<(String, String, String, String)>,
    Query(query): Query<AwsWafScopeQuery>,
    body: Result<Json<AwsWafDeleteRequest>, JsonRejection>,
) -> Response {
    let Ok((account, scope)) = account_scope(account_id, scope, query) else {
        return error(AwsWafAdminError::Invalid);
    };
    let Ok(Json(request)) = body else {
        return error(AwsWafAdminError::Invalid);
    };
    if !valid_identifier(&web_acl_id)
        || !valid_identifier(&reference)
        || !valid_lock_token(&request.lock_token)
        || request.confirmation != format!("{web_acl_id}/{reference}")
    {
        return error(AwsWafAdminError::Invalid);
    }
    let Some(service) = state.aws_waf_admin.as_deref() else {
        return error(AwsWafAdminError::Unavailable);
    };
    match service
        .delete_rule(account, scope, &web_acl_id, &reference, request)
        .await
    {
        Ok(value) => ok(StatusCode::ACCEPTED, value),
        Err(value) => error(value),
    }
}

pub async fn security_weaken_rule(
    State(state): State<ApiState>,
    Path((account_id, scope, web_acl_id, reference)): Path<(String, String, String, String)>,
    Query(query): Query<AwsWafScopeQuery>,
    body: Result<Json<AwsWafRuleSecurityWeakenRequest>, JsonRejection>,
) -> Response {
    let Ok((account, scope)) = account_scope(account_id, scope, query) else {
        return error(AwsWafAdminError::Invalid);
    };
    let Ok(Json(request)) = body else {
        return error(AwsWafAdminError::Invalid);
    };
    if !valid_identifier(&web_acl_id)
        || !valid_identifier(&reference)
        || !valid_lock_token(&request.lock_token)
        || !matches!(
            (request.action, request.managed_override_action),
            (Some(AwsWafActionDto::Allow | AwsWafActionDto::Count), None)
                | (None, Some(AwsWafManagedRuleOverrideActionDto::Count))
        )
        || request.confirmation != format!("{web_acl_id}/{reference}")
    {
        return error(AwsWafAdminError::Invalid);
    }
    let Some(service) = state.aws_waf_admin.as_deref() else {
        return error(AwsWafAdminError::Unavailable);
    };
    match service
        .security_weaken_rule(account, scope, &web_acl_id, &reference, request)
        .await
    {
        Ok(value) => ok(StatusCode::ACCEPTED, value),
        Err(value) => error(value),
    }
}

pub async fn managed_exception(
    State(state): State<ApiState>,
    Path((account_id, scope, web_acl_id, reference)): Path<(String, String, String, String)>,
    Query(query): Query<AwsWafScopeQuery>,
    body: Result<Json<AwsWafManagedExceptionRequest>, JsonRejection>,
) -> Response {
    let Ok((account, scope)) = account_scope(account_id, scope, query) else {
        return error(AwsWafAdminError::Invalid);
    };
    let Ok(Json(request)) = body else {
        return error(AwsWafAdminError::Invalid);
    };
    if !valid_identifier(&web_acl_id)
        || !valid_identifier(&reference)
        || !valid_lock_token(&request.lock_token)
        || request.confirmation != format!("{web_acl_id}/{reference}")
        || !valid_exception(&request)
    {
        return error(AwsWafAdminError::Invalid);
    }
    let Some(service) = state.aws_waf_admin.as_deref() else {
        return error(AwsWafAdminError::Unavailable);
    };
    match service
        .apply_managed_exception(account, scope, &web_acl_id, &reference, request)
        .await
    {
        Ok(value) => ok(StatusCode::ACCEPTED, value),
        Err(value) => error(value),
    }
}

pub async fn check_capacity(
    State(state): State<ApiState>,
    Path((account_id, scope)): Path<(String, String)>,
    Query(query): Query<AwsWafScopeQuery>,
    body: Result<Json<AwsWafCapacityRequest>, JsonRejection>,
) -> Response {
    let Ok((account, scope)) = account_scope(account_id, scope, query) else {
        return error(AwsWafAdminError::Invalid);
    };
    let Ok(Json(request)) = body else {
        return error(AwsWafAdminError::Invalid);
    };
    if request.rules.len() > 1_500 || request.rules.iter().any(|rule| !valid_rule_write(rule)) {
        return error(AwsWafAdminError::Invalid);
    }
    let Some(service) = state.aws_waf_admin.as_deref() else {
        return error(AwsWafAdminError::Unavailable);
    };
    match service.check_capacity(account, scope, request).await {
        Ok(value) => ok(StatusCode::OK, value),
        Err(value) => error(value),
    }
}

pub async fn create_ip_set(
    State(state): State<ApiState>,
    Path((account_id, scope)): Path<(String, String)>,
    Query(query): Query<AwsWafScopeQuery>,
    body: Result<Json<AwsWafIpSetCreateRequest>, JsonRejection>,
) -> Response {
    let Ok((account, scope)) = account_scope(account_id, scope, query) else {
        return error(AwsWafAdminError::Invalid);
    };
    let Ok(Json(request)) = body else {
        return error(AwsWafAdminError::Invalid);
    };
    if !valid_ip_set_create(&request) {
        return error(AwsWafAdminError::Invalid);
    }
    let Some(service) = state.aws_waf_admin.as_deref() else {
        return error(AwsWafAdminError::Unavailable);
    };
    match service.create_ip_set(account, scope, request).await {
        Ok(value) => ok(StatusCode::ACCEPTED, value),
        Err(value) => error(value),
    }
}

pub async fn update_ip_set(
    State(state): State<ApiState>,
    Path((account_id, scope, ip_set_id)): Path<(String, String, String)>,
    Query(query): Query<AwsWafScopeQuery>,
    body: Result<Json<AwsWafIpSetUpdateRequest>, JsonRejection>,
) -> Response {
    let Ok((account, scope)) = account_scope(account_id, scope, query) else {
        return error(AwsWafAdminError::Invalid);
    };
    let Ok(Json(request)) = body else {
        return error(AwsWafAdminError::Invalid);
    };
    if !valid_identifier(&ip_set_id)
        || !valid_lock_token(&request.lock_token)
        || !valid_addresses(&request.addresses)
    {
        return error(AwsWafAdminError::Invalid);
    }
    let Some(service) = state.aws_waf_admin.as_deref() else {
        return error(AwsWafAdminError::Unavailable);
    };
    match service
        .update_ip_set(account, scope, &ip_set_id, request)
        .await
    {
        Ok(value) => ok(StatusCode::ACCEPTED, value),
        Err(value) => error(value),
    }
}

pub async fn delete_ip_set(
    State(state): State<ApiState>,
    Path((account_id, scope, ip_set_id)): Path<(String, String, String)>,
    Query(query): Query<AwsWafScopeQuery>,
    body: Result<Json<AwsWafDeleteRequest>, JsonRejection>,
) -> Response {
    let Ok((account, scope)) = account_scope(account_id, scope, query) else {
        return error(AwsWafAdminError::Invalid);
    };
    let Ok(Json(request)) = body else {
        return error(AwsWafAdminError::Invalid);
    };
    if !valid_identifier(&ip_set_id)
        || !valid_lock_token(&request.lock_token)
        || request.confirmation != ip_set_id
    {
        return error(AwsWafAdminError::Invalid);
    }
    let Some(service) = state.aws_waf_admin.as_deref() else {
        return error(AwsWafAdminError::Unavailable);
    };
    match service
        .delete_ip_set(account, scope, &ip_set_id, request)
        .await
    {
        Ok(()) => ok(StatusCode::NO_CONTENT, ()),
        Err(value) => error(value),
    }
}

pub async fn list_associations(
    State(state): State<ApiState>,
    Path((account_id, scope, web_acl_id)): Path<(String, String, String)>,
    Query(query): Query<AwsWafScopeQuery>,
) -> Response {
    let Ok((account, scope)) = account_scope(account_id, scope, query) else {
        return error(AwsWafAdminError::Invalid);
    };
    if !valid_identifier(&web_acl_id) {
        return error(AwsWafAdminError::Invalid);
    }
    let Some(service) = state.aws_waf_admin.as_deref() else {
        return error(AwsWafAdminError::Unavailable);
    };
    match service.list_associations(account, scope, &web_acl_id).await {
        Ok(value) => ok(StatusCode::OK, value),
        Err(value) => error(value),
    }
}

pub async fn associate_regional_resource(
    State(state): State<ApiState>,
    Path((account_id, scope, web_acl_id)): Path<(String, String, String)>,
    Query(query): Query<AwsWafScopeQuery>,
    body: Result<Json<AwsWafRegionalAssociationRequest>, JsonRejection>,
) -> Response {
    let Ok((account, scope)) = account_scope(account_id, scope, query) else {
        return error(AwsWafAdminError::Invalid);
    };
    let Ok(Json(request)) = body else {
        return error(AwsWafAdminError::Invalid);
    };
    if !matches!(scope, AwsWafScopeDto::Regional { .. })
        || !valid_identifier(&web_acl_id)
        || !valid_association(&request.resource_arn)
    {
        return error(AwsWafAdminError::Invalid);
    }
    let Some(service) = state.aws_waf_admin.as_deref() else {
        return error(AwsWafAdminError::Unavailable);
    };
    match service
        .associate_regional_resource(account, scope, &web_acl_id, request)
        .await
    {
        Ok(value) => ok(StatusCode::ACCEPTED, value),
        Err(value) => error(value),
    }
}

pub async fn disassociate_regional_resource(
    State(state): State<ApiState>,
    Path((account_id, scope)): Path<(String, String)>,
    Query(query): Query<AwsWafScopeQuery>,
    body: Result<Json<AwsWafRegionalDetachRequest>, JsonRejection>,
) -> Response {
    let Ok((account, scope)) = account_scope(account_id, scope, query) else {
        return error(AwsWafAdminError::Invalid);
    };
    let Ok(Json(request)) = body else {
        return error(AwsWafAdminError::Invalid);
    };
    if !matches!(scope, AwsWafScopeDto::Regional { .. })
        || request.confirmation != request.resource_arn
        || !valid_association(&request.resource_arn)
    {
        return error(AwsWafAdminError::Invalid);
    }
    let Some(service) = state.aws_waf_admin.as_deref() else {
        return error(AwsWafAdminError::Unavailable);
    };
    match service
        .disassociate_regional_resource(account, scope, request)
        .await
    {
        Ok(()) => ok(StatusCode::NO_CONTENT, ()),
        Err(value) => error(value),
    }
}

fn account_scope(
    account_id: String,
    scope: String,
    query: AwsWafScopeQuery,
) -> Result<(CloudResourceId, AwsWafScopeDto), AwsWafAdminError> {
    let account = CloudResourceId::new(account_id).map_err(|_| AwsWafAdminError::Invalid)?;
    let scope = parse_scope(&scope, query.region)?;
    Ok((account, scope))
}

fn ok<T: Serialize>(status: StatusCode, value: T) -> Response {
    (status, Json(ApiResponse::ok_body(value))).into_response()
}

fn valid_region(value: &str) -> bool {
    value.len() >= 3
        && value.len() <= 32
        && value.contains('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}
fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.trim() == value
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}
fn valid_lock_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1_024
        && value.trim() == value
        && !value.chars().any(char::is_control)
}
fn valid_visibility(value: &AwsWafVisibilityDto) -> bool {
    valid_identifier(&value.metric_name)
}
fn valid_statement(value: &AwsWafStatementDto) -> bool {
    match value {
        AwsWafStatementDto::ManagedRuleGroup {
            vendor_name,
            name,
            version,
            excluded_rules,
            rule_action_overrides,
        } => {
            valid_identifier(vendor_name)
                && valid_identifier(name)
                && version.as_deref().is_none_or(valid_identifier)
                && excluded_rules.len() <= 100
                && excluded_rules.iter().all(|value| valid_identifier(value))
                && rule_action_overrides.len() <= 100
                && rule_action_overrides
                    .iter()
                    .all(|value| valid_identifier(&value.name))
        }
        AwsWafStatementDto::IpSetReference { arn, .. } => valid_association(arn),
        AwsWafStatementDto::RateBased {
            limit,
            scope_down_ip_set,
        } => {
            (100..=20_000_000).contains(limit)
                && scope_down_ip_set
                    .as_ref()
                    .is_none_or(|value| valid_association(&value.arn))
        }
    }
}
fn valid_rule_write(value: &AwsWafRuleWriteRequest) -> bool {
    valid_identifier(&value.reference)
        && valid_lock_token(&value.lock_token)
        && valid_identifier(&value.name)
        && valid_visibility(&value.visibility)
        && valid_statement(&value.statement)
}
fn valid_web_acl_create(value: &AwsWafWebAclCreateRequest) -> bool {
    valid_identifier(&value.name) && valid_visibility(&value.visibility)
}
fn valid_addresses(addresses: &[String]) -> bool {
    !addresses.is_empty()
        && addresses.len() <= 10_000
        && addresses
            .iter()
            .all(|value| value.len() <= 64 && value.parse::<ipnet::IpNet>().is_ok())
}
fn valid_ip_set_create(value: &AwsWafIpSetCreateRequest) -> bool {
    valid_identifier(&value.name) && valid_addresses(&value.addresses)
}
fn valid_association(value: &str) -> bool {
    value.len() <= 2_048 && value.starts_with("arn:") && !value.chars().any(char::is_control)
}
fn valid_exception(value: &AwsWafManagedExceptionRequest) -> bool {
    valid_lock_token(&value.lock_token)
        && value.excluded_rules.as_ref().is_none_or(|values| {
            values.len() <= 100 && values.iter().all(|value| valid_identifier(value))
        })
        && value.rule_action_overrides.as_ref().is_none_or(|values| {
            values.len() <= 100 && values.iter().all(|value| valid_identifier(&value.name))
        })
}
fn parse_scope(value: &str, region: Option<String>) -> Result<AwsWafScopeDto, AwsWafAdminError> {
    match (value, region) {
        ("cloudfront", None) => Ok(AwsWafScopeDto::Cloudfront),
        ("regional", Some(region)) if valid_region(&region) => {
            Ok(AwsWafScopeDto::Regional { region })
        }
        _ => Err(AwsWafAdminError::Invalid),
    }
}

fn error(value: AwsWafAdminError) -> axum::response::Response {
    let status = match value {
        AwsWafAdminError::Invalid => StatusCode::BAD_REQUEST,
        AwsWafAdminError::NotFound => StatusCode::NOT_FOUND,
        AwsWafAdminError::Conflict => StatusCode::CONFLICT,
        AwsWafAdminError::UnknownOutcome => StatusCode::CONFLICT,
        AwsWafAdminError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
    };
    (
        status,
        Json(ApiResponse::<()>::err_body(error_code(value).to_string())),
    )
        .into_response()
}

fn error_code(value: AwsWafAdminError) -> &'static str {
    if matches!(value, AwsWafAdminError::UnknownOutcome) {
        "unknown_outcome"
    } else {
        "aws_waf_request_failed"
    }
}

#[cfg(test)]
mod wire_tests {
    use super::*;

    #[test]
    fn scope_uses_the_tagged_web_wire_contract() {
        let cloudfront = serde_json::to_value(AwsWafScopeDto::Cloudfront).unwrap();
        let regional = serde_json::to_value(AwsWafScopeDto::Regional {
            region: "us-east-1".to_string(),
        })
        .unwrap();
        assert_eq!(cloudfront, serde_json::json!({ "type": "cloudfront" }));
        assert_eq!(
            regional,
            serde_json::json!({ "type": "regional", "region": "us-east-1" })
        );
        assert_eq!(
            serde_json::from_value::<AwsWafScopeDto>(regional).unwrap(),
            AwsWafScopeDto::Regional {
                region: "us-east-1".to_string(),
            }
        );
    }

    #[test]
    fn unknown_outcome_has_a_stable_client_visible_code() {
        assert_eq!(
            error_code(AwsWafAdminError::UnknownOutcome),
            "unknown_outcome"
        );
    }
}
