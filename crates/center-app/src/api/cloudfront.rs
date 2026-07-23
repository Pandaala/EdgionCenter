//! Minimal CloudFront Distribution Admin API contract.
//!
//! This module is deliberately SDK-free and never accepts or returns a full CloudFront
//! configuration. Provider clients, raw XML, ETags beyond their opaque projection, and origin
//! secrets stay behind the composition boundary.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    extract::{rejection::JsonRejection, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use edgion_center_core::CloudResourceId;
use serde::{Deserialize, Serialize};

use super::ApiState;
use crate::common::api::ApiResponse;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudFrontAdminError {
    InvalidRequest,
    NotFound,
    Conflict,
    UnknownOutcome,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontDistributionDto {
    pub id: String,
    pub arn: String,
    pub domain_name: String,
    pub status: String,
    pub enabled: bool,
    pub etag: String,
    pub deployed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_acl_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_origin: Option<CloudFrontHttpsOriginDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontHttpsOriginDto {
    pub domain_name: String,
    pub https_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudFrontCreateRequest {
    pub caller_reference: String,
    pub origin_domain_name: String,
    pub origin_https_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudFrontOriginUpdateRequest {
    pub origin_domain_name: String,
    pub origin_https_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudFrontDeleteRequest {
    pub confirmation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudFrontWebAclRequest {
    /// Opaque AWS WAFv2 Web ACL ID. The composition resolves and validates its full ACL scope.
    pub web_acl_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloudFrontWebAclDetachRequest {
    pub confirmation: String,
}

#[async_trait]
pub trait CloudFrontAdminService: Send + Sync {
    async fn list(
        &self,
        account_id: &CloudResourceId,
    ) -> Result<Vec<CloudFrontDistributionDto>, CloudFrontAdminError>;
    async fn get(
        &self,
        account_id: &CloudResourceId,
        distribution_id: &str,
    ) -> Result<CloudFrontDistributionDto, CloudFrontAdminError>;
    async fn create(
        &self,
        account_id: &CloudResourceId,
        request: CloudFrontCreateRequest,
    ) -> Result<CloudFrontDistributionDto, CloudFrontAdminError>;
    async fn update_origin(
        &self,
        account_id: &CloudResourceId,
        distribution_id: &str,
        request: CloudFrontOriginUpdateRequest,
    ) -> Result<CloudFrontDistributionDto, CloudFrontAdminError>;
    async fn set_enabled(
        &self,
        account_id: &CloudResourceId,
        distribution_id: &str,
        enabled: bool,
    ) -> Result<CloudFrontDistributionDto, CloudFrontAdminError>;
    async fn delete(
        &self,
        account_id: &CloudResourceId,
        distribution_id: &str,
        request: CloudFrontDeleteRequest,
    ) -> Result<(), CloudFrontAdminError>;
    async fn set_web_acl(
        &self,
        account_id: &CloudResourceId,
        distribution_id: &str,
        request: CloudFrontWebAclRequest,
    ) -> Result<CloudFrontDistributionDto, CloudFrontAdminError>;
    async fn detach_web_acl(
        &self,
        account_id: &CloudResourceId,
        distribution_id: &str,
        request: CloudFrontWebAclDetachRequest,
    ) -> Result<CloudFrontDistributionDto, CloudFrontAdminError>;
}

pub type SharedCloudFrontAdminService = Arc<dyn CloudFrontAdminService>;

pub async fn list(State(state): State<ApiState>, Path(account_id): Path<String>) -> Response {
    let Ok(account_id) = CloudResourceId::new(account_id) else {
        return error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    let Some(service) = state.cloudfront_admin.as_deref() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    match service.list(&account_id).await {
        Ok(items) => (StatusCode::OK, Json(ApiResponse::ok_body(items))).into_response(),
        Err(value) => map_error(value),
    }
}

pub async fn get(
    State(state): State<ApiState>,
    Path((account_id, distribution_id)): Path<(String, String)>,
) -> Response {
    call_get(&state, account_id, distribution_id).await
}

pub async fn observe(
    State(state): State<ApiState>,
    Path((account_id, distribution_id)): Path<(String, String)>,
) -> Response {
    call_get(&state, account_id, distribution_id).await
}

async fn call_get(state: &ApiState, account_id: String, distribution_id: String) -> Response {
    let Ok(account_id) = CloudResourceId::new(account_id) else {
        return error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    if !valid_distribution_id(&distribution_id) {
        return error(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let Some(service) = state.cloudfront_admin.as_deref() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    match service.get(&account_id, &distribution_id).await {
        Ok(item) => (StatusCode::OK, Json(ApiResponse::ok_body(item))).into_response(),
        Err(value) => map_error(value),
    }
}

pub async fn create(
    State(state): State<ApiState>,
    Path(account_id): Path<String>,
    body: Result<Json<CloudFrontCreateRequest>, JsonRejection>,
) -> Response {
    let Ok(account_id) = CloudResourceId::new(account_id) else {
        return error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    let Ok(Json(request)) = body else {
        return error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    let Some(service) = state.cloudfront_admin.as_deref() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    match service.create(&account_id, request).await {
        Ok(item) => (StatusCode::ACCEPTED, Json(ApiResponse::ok_body(item))).into_response(),
        Err(value) => map_error(value),
    }
}

pub async fn update_origin(
    State(state): State<ApiState>,
    Path((account_id, distribution_id)): Path<(String, String)>,
    body: Result<Json<CloudFrontOriginUpdateRequest>, JsonRejection>,
) -> Response {
    let Ok(account_id) = CloudResourceId::new(account_id) else {
        return error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    let Ok(Json(request)) = body else {
        return error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    if !valid_distribution_id(&distribution_id) {
        return error(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let Some(service) = state.cloudfront_admin.as_deref() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    match service
        .update_origin(&account_id, &distribution_id, request)
        .await
    {
        Ok(item) => (StatusCode::ACCEPTED, Json(ApiResponse::ok_body(item))).into_response(),
        Err(value) => map_error(value),
    }
}

pub async fn enable(
    State(state): State<ApiState>,
    Path((account_id, distribution_id)): Path<(String, String)>,
) -> Response {
    set_enabled(state, account_id, distribution_id, true).await
}
pub async fn disable(
    State(state): State<ApiState>,
    Path((account_id, distribution_id)): Path<(String, String)>,
) -> Response {
    set_enabled(state, account_id, distribution_id, false).await
}
async fn set_enabled(
    state: ApiState,
    account_id: String,
    distribution_id: String,
    enabled: bool,
) -> Response {
    let Ok(account_id) = CloudResourceId::new(account_id) else {
        return error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    if !valid_distribution_id(&distribution_id) {
        return error(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let Some(service) = state.cloudfront_admin.as_deref() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    match service
        .set_enabled(&account_id, &distribution_id, enabled)
        .await
    {
        Ok(item) => (StatusCode::ACCEPTED, Json(ApiResponse::ok_body(item))).into_response(),
        Err(value) => map_error(value),
    }
}

pub async fn delete(
    State(state): State<ApiState>,
    Path((account_id, distribution_id)): Path<(String, String)>,
    body: Result<Json<CloudFrontDeleteRequest>, JsonRejection>,
) -> Response {
    let Ok(account_id) = CloudResourceId::new(account_id) else {
        return error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    let Ok(Json(request)) = body else {
        return error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    if !valid_distribution_id(&distribution_id) || request.confirmation != distribution_id {
        return error(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let Some(service) = state.cloudfront_admin.as_deref() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    match service.delete(&account_id, &distribution_id, request).await {
        Ok(()) => (
            StatusCode::NO_CONTENT,
            Json(ApiResponse::ok_body(())).into_response(),
        )
            .into_response(),
        Err(value) => map_error(value),
    }
}

pub async fn set_web_acl(
    State(state): State<ApiState>,
    Path((account_id, distribution_id)): Path<(String, String)>,
    body: Result<Json<CloudFrontWebAclRequest>, JsonRejection>,
) -> Response {
    let Ok(account_id) = CloudResourceId::new(account_id) else {
        return error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    let Ok(Json(request)) = body else {
        return error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    if !valid_distribution_id(&distribution_id) {
        return error(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let Some(service) = state.cloudfront_admin.as_deref() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    match service
        .set_web_acl(&account_id, &distribution_id, request)
        .await
    {
        Ok(item) => (StatusCode::ACCEPTED, Json(ApiResponse::ok_body(item))).into_response(),
        Err(value) => map_error(value),
    }
}

pub async fn detach_web_acl(
    State(state): State<ApiState>,
    Path((account_id, distribution_id)): Path<(String, String)>,
    body: Result<Json<CloudFrontWebAclDetachRequest>, JsonRejection>,
) -> Response {
    let Ok(account_id) = CloudResourceId::new(account_id) else {
        return error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    let Ok(Json(request)) = body else {
        return error(StatusCode::BAD_REQUEST, "invalid_request");
    };
    if !valid_distribution_id(&distribution_id) || request.confirmation != distribution_id {
        return error(StatusCode::BAD_REQUEST, "invalid_request");
    }
    let Some(service) = state.cloudfront_admin.as_deref() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable");
    };
    match service
        .detach_web_acl(&account_id, &distribution_id, request)
        .await
    {
        Ok(item) => (StatusCode::ACCEPTED, Json(ApiResponse::ok_body(item))).into_response(),
        Err(value) => map_error(value),
    }
}

fn valid_distribution_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|value| value.is_ascii_uppercase() || value.is_ascii_digit())
}
fn error(status: StatusCode, code: &'static str) -> Response {
    (status, Json(ApiResponse::<()>::err_body(code.to_string()))).into_response()
}
fn map_error(value: CloudFrontAdminError) -> Response {
    match value {
        CloudFrontAdminError::InvalidRequest => error(StatusCode::BAD_REQUEST, "invalid_request"),
        CloudFrontAdminError::NotFound => error(StatusCode::NOT_FOUND, "not_found"),
        CloudFrontAdminError::Conflict => error(StatusCode::CONFLICT, "conflict"),
        CloudFrontAdminError::UnknownOutcome => error(StatusCode::CONFLICT, "unknown_outcome"),
        CloudFrontAdminError::Unavailable => {
            error(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_create_shape_rejects_arbitrary_distribution_config() {
        let request: CloudFrontCreateRequest = serde_json::from_str(
            r#"{"callerReference":"r1","originDomainName":"api.example.com","originHttpsPort":443}"#,
        )
        .expect("fixed request");
        assert_eq!(request.origin_https_port, 443);
        assert!(serde_json::from_str::<CloudFrontCreateRequest>(
            r#"{"callerReference":"r1","originDomainName":"api.example.com","originHttpsPort":443,"distributionConfig":{}}"#
        )
        .is_err());
    }

    #[test]
    fn distribution_identity_is_bounded() {
        assert!(valid_distribution_id("E123ABC"));
        assert!(!valid_distribution_id("e123abc"));
        assert!(!valid_distribution_id("E123/ABC"));
    }

    #[test]
    fn web_acl_contract_rejects_raw_provider_configuration() {
        let request: CloudFrontWebAclRequest =
            serde_json::from_str(r#"{"webAclId":"ACL123"}"#).expect("fixed WAF request");
        assert_eq!(request.web_acl_id, "ACL123");
        assert!(serde_json::from_str::<CloudFrontWebAclRequest>(
            r#"{"webAclId":"ACL123","distributionConfig":{}}"#
        )
        .is_err());
    }
}
