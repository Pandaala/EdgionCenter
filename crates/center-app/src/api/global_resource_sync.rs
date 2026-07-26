//! Durable GlobalResource desired-state, planning, and manual apply Admin API.
//!
//! Apply is manual and fenced; adoption, prune, and delete remain unavailable.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{rejection::JsonRejection, rejection::QueryRejection, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use edgion_center_core::{
    GlobalResource, GlobalResourceCreateResult, GlobalResourceDesired, GlobalResourceId,
    GlobalResourcePageRequest, GlobalResourcePlan, GlobalResourcePlanState,
    GlobalResourceReplaceResult, GlobalResourceStore, MAX_GLOBAL_RESOURCE_PAGE_SIZE,
};
use edgion_center_runtime::global_resource_planner::GlobalResourcePlanService;
use serde::{Deserialize, Serialize};

use super::ApiState;
use crate::common::{
    api::{ApiResponse, ListResponse},
    unified_auth::UnifiedAuthClaims,
};

const DEFAULT_PAGE_SIZE: u16 = 50;
const CURSOR_PREFIX: &str = "grd1.";
const MAX_CURSOR_BYTES: usize = 384;
const MAX_PLAN_TARGETS: usize = 100;

#[derive(Clone)]
pub struct GlobalResourceSyncApi {
    pub store: Arc<dyn GlobalResourceStore>,
    pub planner: Arc<GlobalResourcePlanService>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateGlobalResourceRequest {
    pub id: String,
    pub desired: GlobalResourceDesired,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplaceGlobalResourceRequest {
    pub desired: GlobalResourceDesired,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListGlobalResourcesQuery {
    #[serde(default = "default_page_size")]
    pub limit: u16,
    pub cursor: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanGlobalResourceRequest {
    pub target_clusters: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApplyGlobalResourceRequest {
    pub plan_token: String,
    pub generation: u64,
    pub target_clusters: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GlobalResourcePlanResponse {
    global_resource_id: GlobalResourceId,
    generation: u64,
    desired_revision: edgion_center_core::GlobalResourceRevision,
    target_clusters: Vec<String>,
    plan_token: String,
    applicable: bool,
    targets: Vec<edgion_center_core::GlobalResourceTargetPlan>,
}

fn default_page_size() -> u16 {
    DEFAULT_PAGE_SIZE
}

fn error(status: StatusCode, code: &'static str) -> Response {
    (status, Json(ApiResponse::<()>::err_body(code.to_string()))).into_response()
}

fn invalid_request() -> Response {
    error(StatusCode::BAD_REQUEST, "invalid_global_resource_request")
}

fn store_failure() -> Response {
    error(
        StatusCode::SERVICE_UNAVAILABLE,
        "global_resource_store_unavailable",
    )
}

fn planner_failure() -> Response {
    error(
        StatusCode::SERVICE_UNAVAILABLE,
        "global_resource_planner_unavailable",
    )
}

fn json_rejection(rejection: JsonRejection) -> Response {
    if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
        error(StatusCode::PAYLOAD_TOO_LARGE, "request_too_large")
    } else {
        invalid_request()
    }
}

#[allow(clippy::result_large_err)]
fn actor(claims: Option<Extension<UnifiedAuthClaims>>) -> Result<String, Response> {
    claims
        .and_then(|Extension(claims)| claims.sub)
        .filter(|subject| !subject.trim().is_empty())
        .ok_or_else(|| error(StatusCode::UNAUTHORIZED, "authenticated_subject_required"))
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[allow(clippy::result_large_err)]
fn parse_id(value: String) -> Result<GlobalResourceId, Response> {
    GlobalResourceId::new(value).map_err(|_| invalid_request())
}

fn encode_cursor(id: &GlobalResourceId) -> String {
    format!("{CURSOR_PREFIX}{}", URL_SAFE_NO_PAD.encode(id.as_str()))
}

#[allow(clippy::result_large_err)]
fn decode_cursor(value: &str) -> Result<GlobalResourceId, Response> {
    if value.len() > MAX_CURSOR_BYTES {
        return Err(invalid_request());
    }
    let encoded = value
        .strip_prefix(CURSOR_PREFIX)
        .ok_or_else(invalid_request)?;
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| invalid_request())?;
    let decoded = String::from_utf8(bytes).map_err(|_| invalid_request())?;
    let id = GlobalResourceId::new(decoded).map_err(|_| invalid_request())?;
    if encode_cursor(&id) != value {
        return Err(invalid_request());
    }
    Ok(id)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IfMatchError {
    Missing,
    Invalid,
}

fn parse_if_match(headers: &HeaderMap) -> Result<u64, IfMatchError> {
    let values = headers.get_all(header::IF_MATCH);
    let mut values = values.iter();
    let Some(value) = values.next() else {
        return Err(IfMatchError::Missing);
    };
    if values.next().is_some() {
        return Err(IfMatchError::Invalid);
    }
    let raw = value.to_str().map_err(|_| IfMatchError::Invalid)?;
    let generation = raw
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .ok_or(IfMatchError::Invalid)?;
    if raw != format!("\"{generation}\"") {
        return Err(IfMatchError::Invalid);
    }
    Ok(generation)
}

fn resource_response(status: StatusCode, resource: GlobalResource) -> Response {
    let generation = resource.generation;
    let mut response = (status, Json(ApiResponse::ok_body(resource))).into_response();
    if let Ok(value) = HeaderValue::from_str(&format!("\"{generation}\"")) {
        response.headers_mut().insert(header::ETAG, value);
    }
    response
}

fn created_resource_response(resource: GlobalResource) -> Response {
    let location = format!(
        "/api/v1/center/global-resource-sync/resources/{}",
        resource.id
    );
    let mut response = resource_response(StatusCode::CREATED, resource);
    if let Ok(value) = HeaderValue::from_str(&location) {
        response.headers_mut().insert(header::LOCATION, value);
    }
    response
}

#[allow(clippy::result_large_err)]
fn sync_api(state: &ApiState) -> Result<&GlobalResourceSyncApi, Response> {
    state.global_resource_sync.as_deref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "global_resource_sync_unavailable",
        )
    })
}

pub async fn create(
    State(state): State<ApiState>,
    claims: Option<Extension<UnifiedAuthClaims>>,
    request: Result<Json<CreateGlobalResourceRequest>, JsonRejection>,
) -> Response {
    let api = match sync_api(&state) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let actor = match actor(claims) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let request = match request {
        Ok(Json(value)) => value,
        Err(rejection) => return json_rejection(rejection),
    };
    let id = match parse_id(request.id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    if request.desired.validate(api.planner.config()).is_err() {
        return invalid_request();
    }
    match api
        .store
        .create(&id, &request.desired, &actor, now_unix_ms())
        .await
    {
        Ok(GlobalResourceCreateResult::Created(resource)) => created_resource_response(*resource),
        Ok(GlobalResourceCreateResult::AlreadyExists) => {
            error(StatusCode::CONFLICT, "global_resource_already_exists")
        }
        Err(_) => store_failure(),
    }
}

pub async fn list(
    State(state): State<ApiState>,
    query: Result<Query<ListGlobalResourcesQuery>, QueryRejection>,
) -> Response {
    let api = match sync_api(&state) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let query = match query {
        Ok(Query(value)) => value,
        Err(_) => return invalid_request(),
    };
    let after = match query.cursor.as_deref().map(decode_cursor).transpose() {
        Ok(value) => value,
        Err(response) => return response,
    };
    let request = GlobalResourcePageRequest {
        limit: query.limit,
        after,
    };
    if request.validate().is_err() || request.limit > MAX_GLOBAL_RESOURCE_PAGE_SIZE {
        return invalid_request();
    }
    match api.store.list(&request).await {
        Ok(page) => {
            if page.validate(&request, api.planner.config()).is_err() {
                return store_failure();
            }
            let cursor = page.next.as_ref().map(encode_cursor);
            Json(ListResponse::success_with_token(page.items, cursor)).into_response()
        }
        Err(_) => store_failure(),
    }
}

pub async fn get(State(state): State<ApiState>, Path(id): Path<String>) -> Response {
    let api = match sync_api(&state) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let id = match parse_id(id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    match api.store.get(&id).await {
        Ok(Some(resource)) => resource_response(StatusCode::OK, resource),
        Ok(None) => error(StatusCode::NOT_FOUND, "global_resource_not_found"),
        Err(_) => store_failure(),
    }
}

pub async fn replace(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    claims: Option<Extension<UnifiedAuthClaims>>,
    request: Result<Json<ReplaceGlobalResourceRequest>, JsonRejection>,
) -> Response {
    let api = match sync_api(&state) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let actor = match actor(claims) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let id = match parse_id(id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let expected_generation = match parse_if_match(&headers) {
        Ok(value) => value,
        Err(IfMatchError::Missing) => {
            return error(StatusCode::PRECONDITION_REQUIRED, "if_match_required")
        }
        Err(IfMatchError::Invalid) => return invalid_request(),
    };
    let request = match request {
        Ok(Json(value)) => value,
        Err(rejection) => return json_rejection(rejection),
    };
    if request.desired.validate(api.planner.config()).is_err() {
        return invalid_request();
    }
    match api
        .store
        .replace_if_generation(
            &id,
            expected_generation,
            &request.desired,
            &actor,
            now_unix_ms(),
        )
        .await
    {
        Ok(GlobalResourceReplaceResult::Stored(resource)) => {
            resource_response(StatusCode::OK, *resource)
        }
        Ok(GlobalResourceReplaceResult::NotFound) => {
            error(StatusCode::NOT_FOUND, "global_resource_not_found")
        }
        Ok(GlobalResourceReplaceResult::GenerationMismatch { .. }) => error(
            StatusCode::PRECONDITION_FAILED,
            "global_resource_generation_mismatch",
        ),
        Err(edgion_center_core::CoreError::Conflict(_)) => invalid_request(),
        Err(_) => store_failure(),
    }
}

pub async fn plan(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    request: Result<Json<PlanGlobalResourceRequest>, JsonRejection>,
) -> Response {
    let api = match sync_api(&state) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let id = match parse_id(id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let request = match request {
        Ok(Json(value)) => value,
        Err(rejection) => return json_rejection(rejection),
    };
    let target_clusters = match validate_plan_targets(request.target_clusters) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let resource = match api.store.get(&id).await {
        Ok(Some(value)) => value,
        Ok(None) => return error(StatusCode::NOT_FOUND, "global_resource_not_found"),
        Err(_) => return store_failure(),
    };
    let plan = match api
        .planner
        .plan(&resource, target_clusters.as_deref())
        .await
    {
        Ok(value) => value,
        Err(edgion_center_core::CoreError::Conflict(_)) => {
            return error(StatusCode::BAD_REQUEST, "invalid_plan_target")
        }
        Err(_) => return planner_failure(),
    };
    Json(ApiResponse::ok_body(plan_response(plan))).into_response()
}

pub async fn apply(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    claims: Option<Extension<UnifiedAuthClaims>>,
    request: Result<Json<ApplyGlobalResourceRequest>, JsonRejection>,
) -> Response {
    let api = match sync_api(&state) {
        Ok(value) => value,
        Err(response) => return response,
    };
    if actor(claims).is_err() {
        return error(StatusCode::UNAUTHORIZED, "authenticated_subject_required");
    }
    let id = match parse_id(id) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let request = match request {
        Ok(Json(value)) => value,
        Err(rejection) => return json_rejection(rejection),
    };
    let targets = match validate_plan_targets(Some(request.target_clusters.clone())) {
        Ok(Some(value)) => value,
        _ => return error(StatusCode::BAD_REQUEST, "invalid_plan_target"),
    };
    let resource = match api.store.get(&id).await {
        Ok(Some(value)) => value,
        Ok(None) => return error(StatusCode::NOT_FOUND, "global_resource_not_found"),
        Err(_) => return store_failure(),
    };
    if resource.generation != request.generation {
        return error(
            StatusCode::PRECONDITION_FAILED,
            "global_resource_generation_mismatch",
        );
    }
    let plan = match api.planner.plan(&resource, Some(&targets)).await {
        Ok(value) => value,
        Err(_) => return planner_failure(),
    };
    let token = format!(
        "grp1.{}",
        plan.binding
            .plan_revision
            .strip_prefix("sha256:")
            .unwrap_or(&plan.binding.plan_revision)
    );
    if token != request.plan_token || plan.binding.ordered_targets != targets {
        return error(
            StatusCode::PRECONDITION_FAILED,
            "global_resource_plan_mismatch",
        );
    }
    if plan.targets.iter().any(|target| {
        !matches!(
            target.state,
            GlobalResourcePlanState::Create
                | GlobalResourcePlanState::Update
                | GlobalResourcePlanState::Noop
        )
    }) {
        return error(StatusCode::CONFLICT, "global_resource_plan_not_applicable");
    }
    match api.planner.apply_plan(&resource, &plan).await {
        Ok(result) => Json(ApiResponse::ok_body(result)).into_response(),
        Err(_) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "global_resource_apply_unavailable",
        ),
    }
}

#[allow(clippy::result_large_err)]
fn validate_plan_targets(targets: Option<Vec<String>>) -> Result<Option<Vec<String>>, Response> {
    let Some(targets) = targets else {
        return Ok(None);
    };
    if targets.len() > MAX_PLAN_TARGETS
        || targets
            .iter()
            .any(|value| value.is_empty() || value.trim() != value || value.len() > 253)
    {
        return Err(error(StatusCode::BAD_REQUEST, "invalid_plan_target"));
    }
    let ordered = targets.into_iter().collect::<BTreeSet<_>>();
    if ordered.len() > MAX_PLAN_TARGETS {
        return Err(error(StatusCode::BAD_REQUEST, "invalid_plan_target"));
    }
    Ok(Some(ordered.into_iter().collect()))
}

fn plan_response(plan: GlobalResourcePlan) -> GlobalResourcePlanResponse {
    let applicable = !plan.targets.is_empty()
        && plan.targets.iter().all(|target| {
            matches!(
                target.state,
                GlobalResourcePlanState::Create
                    | GlobalResourcePlanState::Update
                    | GlobalResourcePlanState::Noop
            )
        });
    let plan_token = format!(
        "grp1.{}",
        plan.binding
            .plan_revision
            .strip_prefix("sha256:")
            .unwrap_or(&plan.binding.plan_revision)
    );
    GlobalResourcePlanResponse {
        global_resource_id: plan.binding.global_resource_id,
        generation: plan.binding.generation,
        desired_revision: plan.binding.desired_revision,
        target_clusters: plan.binding.ordered_targets,
        plan_token,
        applicable,
        targets: plan.targets,
    }
}
