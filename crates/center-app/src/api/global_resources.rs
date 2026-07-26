use super::ApiState;
use axum::{
    extract::{rejection::QueryRejection, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use edgion_center_core::{
    CoreError, EdgionConfigDataType, GlobalResourceInventoryKind, GLOBAL_RESOURCE_KINDS,
};
use edgion_center_runtime::global_resources::{
    ClusterInventory, ClusterResolution, ClusterResolutionState, InventoryErrorCode,
    InventoryNamespaceError,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const TOKEN_PREFIX: &str = "gr1.";
const MAX_TOKEN_BYTES: usize = 4096;
const DEFAULT_LIMIT: usize = 50;
const MAX_LIMIT: usize = 200;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorBody {
    code: &'static str,
    message: &'static str,
}

fn error_response(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    (
        status,
        Json(ErrorEnvelope {
            error: ErrorBody { code, message },
        }),
    )
        .into_response()
}

fn invalid_request(code: &'static str) -> Response {
    error_response(StatusCode::BAD_REQUEST, code, "The request is invalid.")
}

fn service_unavailable() -> Response {
    error_response(
        StatusCode::SERVICE_UNAVAILABLE,
        "global_resources_unavailable",
        "Global resource inventory is temporarily unavailable.",
    )
}

fn map_core_error(error: CoreError) -> Response {
    match error {
        CoreError::InvalidIdentifier { .. } | CoreError::Conflict(_) => {
            invalid_request("invalid_request")
        }
        CoreError::NotFound(_) => error_response(
            StatusCode::NOT_FOUND,
            "global_resource_not_found",
            "The requested global resource was not found.",
        ),
        CoreError::Unsupported(_) | CoreError::Adapter(_) => service_unavailable(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ApiKind {
    kind: GlobalResourceInventoryKind,
    slug: &'static str,
}

const API_KINDS: [ApiKind; 5] = [
    ApiKind {
        kind: GlobalResourceInventoryKind::HTTPRoute,
        slug: "http-route",
    },
    ApiKind {
        kind: GlobalResourceInventoryKind::GRPCRoute,
        slug: "grpc-route",
    },
    ApiKind {
        kind: GlobalResourceInventoryKind::EdgionPlugins,
        slug: "edgion-plugins",
    },
    ApiKind {
        kind: GlobalResourceInventoryKind::EdgionConfigData,
        slug: "edgion-config-data",
    },
    ApiKind {
        kind: GlobalResourceInventoryKind::ReferenceGrant,
        slug: "reference-grant",
    },
];

fn parse_kind(slug: &str) -> Option<ApiKind> {
    API_KINDS.iter().copied().find(|entry| {
        entry.slug == slug
            || GLOBAL_RESOURCE_KINDS
                .iter()
                .find(|catalog| catalog.kind == entry.kind)
                .is_some_and(|catalog| catalog.path_kind == slug)
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogResponse {
    catalog_revision: String,
    config_revision: String,
    platform_namespaces: Vec<String>,
    kinds: Vec<CatalogKind>,
    clusters: Vec<ClusterResolution>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogKind {
    kind: GlobalResourceInventoryKind,
    slug: &'static str,
    config_data_types: &'static [EdgionConfigDataType],
}

fn catalog_revision() -> String {
    let api_kinds = API_KINDS
        .iter()
        .map(|entry| (entry.kind, entry.slug))
        .collect::<Vec<_>>();
    let canonical = serde_json::to_vec(&(&GLOBAL_RESOURCE_KINDS, api_kinds))
        .expect("static global resource catalog is valid");
    let digest = Sha256::digest(canonical);
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("sha256:{hex}")
}

pub async fn catalog(State(state): State<ApiState>) -> Response {
    let Some(service) = state.global_resources.as_ref() else {
        return service_unavailable();
    };
    let clusters = match service.resolve_targets().await {
        Ok(clusters) => clusters,
        Err(error) => return map_core_error(error),
    };
    let kinds = API_KINDS
        .iter()
        .map(|api| {
            let core = GLOBAL_RESOURCE_KINDS
                .iter()
                .find(|entry| entry.kind == api.kind)
                .expect("API kind exists in the core catalog");
            CatalogKind {
                kind: api.kind,
                slug: api.slug,
                config_data_types: core.config_data_types,
            }
        })
        .collect();
    Json(CatalogResponse {
        catalog_revision: catalog_revision(),
        config_revision: service.config().revision(),
        platform_namespaces: service.config().platform_namespaces.clone(),
        kinds,
        clusters,
    })
    .into_response()
}

#[derive(Debug, Default)]
struct ListQuery {
    cluster: Vec<String>,
    config_data_type: Option<String>,
    limit: Option<usize>,
    continue_token: Option<String>,
}

fn parse_list_query(pairs: Vec<(String, String)>) -> Result<ListQuery, ()> {
    let mut query = ListQuery::default();
    for (key, value) in pairs {
        match key.as_str() {
            "cluster" => query.cluster.push(value),
            "configDataType" if query.config_data_type.is_none() => {
                query.config_data_type = Some(value);
            }
            "limit" if query.limit.is_none() => {
                query.limit = Some(value.parse().map_err(|_| ())?);
            }
            "continue" if query.continue_token.is_none() => {
                query.continue_token = Some(value);
            }
            _ => return Err(()),
        }
    }
    Ok(query)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum ConfigDataFilter {
    KeyList,
    IpList,
    Selector,
    RegionRouteOverride,
    Misc,
    Unknown,
}

fn parse_config_data_filter(value: Option<&str>) -> Result<Option<ConfigDataFilter>, ()> {
    value
        .map(|value| match value {
            "KeyList" => Ok(ConfigDataFilter::KeyList),
            "IpList" => Ok(ConfigDataFilter::IpList),
            "Selector" => Ok(ConfigDataFilter::Selector),
            "RegionRouteOverride" => Ok(ConfigDataFilter::RegionRouteOverride),
            "Misc" => Ok(ConfigDataFilter::Misc),
            "Unknown" => Ok(ConfigDataFilter::Unknown),
            _ => Err(()),
        })
        .transpose()
}

fn item_config_data_type(item: &Value) -> ConfigDataFilter {
    match item.pointer("/spec/data/type").and_then(Value::as_str) {
        Some("KeyList") => ConfigDataFilter::KeyList,
        Some("IpList") => ConfigDataFilter::IpList,
        Some("Selector") => ConfigDataFilter::Selector,
        Some("RegionRouteOverride") => ConfigDataFilter::RegionRouteOverride,
        Some("Misc") => ConfigDataFilter::Misc,
        _ => ConfigDataFilter::Unknown,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GroupKey {
    kind: String,
    namespace: String,
    name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ComparisonMember {
    cluster: String,
    controller_id: Option<String>,
    object: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ComparisonGroup {
    key: GroupKey,
    members: Vec<ComparisonMember>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicInventoryError {
    namespace: String,
    code: InventoryErrorCode,
    status: Option<u16>,
    retryable: bool,
}

impl From<&InventoryNamespaceError> for PublicInventoryError {
    fn from(error: &InventoryNamespaceError) -> Self {
        Self {
            namespace: error.namespace.clone(),
            code: error.code,
            status: error.status,
            retryable: error.retryable,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClusterResult {
    cluster: String,
    state: ClusterResolutionState,
    controller_id: Option<String>,
    candidates: Vec<String>,
    complete: bool,
    errors: Vec<PublicInventoryError>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListResponse {
    catalog_revision: String,
    config_revision: String,
    membership_revision: String,
    inventory_revision: String,
    kind: GlobalResourceInventoryKind,
    config_data_type: Option<ConfigDataFilter>,
    clusters: Vec<ClusterResult>,
    groups: Vec<ComparisonGroup>,
    continue_token: Option<String>,
}

fn group_inventory(
    kind: GlobalResourceInventoryKind,
    clusters: &[ClusterInventory],
    filter: Option<ConfigDataFilter>,
) -> Vec<ComparisonGroup> {
    let mut groups = BTreeMap::<GroupKey, Vec<ComparisonMember>>::new();
    for cluster in clusters {
        for item in &cluster.items {
            let Some(namespace) = item.pointer("/metadata/namespace").and_then(Value::as_str)
            else {
                continue;
            };
            let Some(name) = item.pointer("/metadata/name").and_then(Value::as_str) else {
                continue;
            };
            let key = GroupKey {
                kind: kind.as_str().to_string(),
                namespace: namespace.to_string(),
                name: name.to_string(),
            };
            groups.entry(key).or_default().push(ComparisonMember {
                cluster: cluster.cluster.clone(),
                controller_id: cluster.controller_id.clone(),
                object: item.clone(),
            });
        }
    }
    groups
        .into_iter()
        .filter(|(_, members)| {
            kind != GlobalResourceInventoryKind::EdgionConfigData
                || filter.is_none_or(|expected| {
                    members
                        .iter()
                        .any(|member| item_config_data_type(&member.object) == expected)
                })
        })
        .map(|(key, mut members)| {
            members.sort_by(|left, right| {
                (&left.cluster, &left.controller_id).cmp(&(&right.cluster, &right.controller_id))
            });
            ComparisonGroup { key, members }
        })
        .collect()
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ContinueToken {
    catalog_revision: String,
    config_revision: String,
    kind: String,
    query_revision: String,
    membership_revision: String,
    limit: usize,
    config_data_type: Option<ConfigDataFilter>,
    inventory_revision: String,
    after: GroupKey,
}

fn encode_token(token: &ContinueToken) -> String {
    let canonical = serde_json::to_vec(token).expect("continuation token is serializable");
    format!("{TOKEN_PREFIX}{}", URL_SAFE_NO_PAD.encode(canonical))
}

fn decode_token(value: &str) -> Result<ContinueToken, ()> {
    if value.len() > MAX_TOKEN_BYTES {
        return Err(());
    }
    let encoded = value.strip_prefix(TOKEN_PREFIX).ok_or(())?;
    let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|_| ())?;
    let token = serde_json::from_slice::<ContinueToken>(&bytes).map_err(|_| ())?;
    if encode_token(&token) != value {
        return Err(());
    }
    Ok(token)
}

fn normalized_clusters(clusters: &[String]) -> Result<Vec<String>, ()> {
    if clusters
        .iter()
        .any(|cluster| cluster.is_empty() || cluster.trim() != cluster)
    {
        return Err(());
    }
    Ok(clusters
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct InventorySnapshot<'a> {
    clusters: &'a [ClusterResult],
    groups: &'a [ComparisonGroup],
}

fn canonicalize_json(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(canonicalize_json),
        Value::Object(object) => {
            object.values_mut().for_each(canonicalize_json);
            object.sort_keys();
        }
        _ => {}
    }
}

fn inventory_revision(clusters: &[ClusterResult], groups: &[ComparisonGroup]) -> String {
    let mut snapshot = serde_json::to_value(InventorySnapshot { clusters, groups })
        .expect("global resource inventory snapshot is serializable");
    canonicalize_json(&mut snapshot);
    let canonical =
        serde_json::to_vec(&snapshot).expect("canonical inventory snapshot is serializable");
    let digest = Sha256::digest(canonical);
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("sha256:{hex}")
}

fn cluster_query_revision(clusters: &[String]) -> String {
    let canonical = serde_json::to_vec(clusters).expect("normalized cluster query is serializable");
    let digest = Sha256::digest(canonical);
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("sha256:{hex}")
}

pub async fn list(
    State(state): State<ApiState>,
    Path(kind_slug): Path<String>,
    query: Result<Query<Vec<(String, String)>>, QueryRejection>,
) -> Response {
    let Some(service) = state.global_resources.as_ref() else {
        return service_unavailable();
    };
    let Some(api_kind) = parse_kind(&kind_slug) else {
        return invalid_request("invalid_global_resource_kind");
    };
    let pairs = match query {
        Ok(Query(pairs)) => pairs,
        Err(_) => return invalid_request("invalid_query"),
    };
    let query = match parse_list_query(pairs) {
        Ok(query) => query,
        Err(()) => return invalid_request("invalid_query"),
    };
    let requested_clusters = match normalized_clusters(&query.cluster) {
        Ok(clusters) => clusters,
        Err(()) => return invalid_request("invalid_cluster"),
    };
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return invalid_request("invalid_limit");
    }
    let filter = match parse_config_data_filter(query.config_data_type.as_deref()) {
        Ok(filter) => filter,
        Err(()) => return invalid_request("invalid_config_data_type"),
    };
    if api_kind.kind != GlobalResourceInventoryKind::EdgionConfigData && filter.is_some() {
        return invalid_request("config_data_type_not_applicable");
    }
    let decoded_token = match query
        .continue_token
        .as_deref()
        .map(decode_token)
        .transpose()
    {
        Ok(token) => token,
        Err(()) => return invalid_request("invalid_continue_token"),
    };
    let current_catalog_revision = catalog_revision();
    let current_config_revision = service.config().revision();
    let current_query_revision = cluster_query_revision(&requested_clusters);
    if decoded_token.as_ref().is_some_and(|token| {
        token.catalog_revision != current_catalog_revision
            || token.config_revision != current_config_revision
            || token.kind != api_kind.slug
            || token.query_revision != current_query_revision
            || token.limit != limit
            || token.config_data_type != filter
    }) {
        return invalid_request("stale_continue_token");
    }
    let inventory = match service.inventory(api_kind.kind, &requested_clusters).await {
        Ok(inventory) => inventory,
        Err(error) => return map_core_error(error),
    };
    let all_groups = group_inventory(api_kind.kind, &inventory.clusters, filter);
    let clusters = inventory
        .clusters
        .iter()
        .map(|cluster| ClusterResult {
            cluster: cluster.cluster.clone(),
            state: cluster.state.clone(),
            controller_id: cluster.controller_id.clone(),
            candidates: cluster.candidates.clone(),
            complete: cluster.complete,
            errors: cluster
                .errors
                .iter()
                .map(PublicInventoryError::from)
                .collect(),
        })
        .collect::<Vec<_>>();
    let current_inventory_revision = inventory_revision(&clusters, &all_groups);
    if decoded_token.as_ref().is_some_and(|token| {
        token.membership_revision != inventory.membership_revision
            || token.inventory_revision != current_inventory_revision
    }) {
        return invalid_request("stale_continue_token");
    }
    let start = decoded_token
        .as_ref()
        .map(|token| all_groups.partition_point(|group| group.key <= token.after))
        .unwrap_or(0);
    let end = start.saturating_add(limit).min(all_groups.len());
    let next = (end < all_groups.len()).then(|| {
        encode_token(&ContinueToken {
            catalog_revision: current_catalog_revision.clone(),
            config_revision: current_config_revision.clone(),
            kind: api_kind.slug.to_string(),
            query_revision: current_query_revision,
            membership_revision: inventory.membership_revision.clone(),
            limit,
            config_data_type: filter,
            inventory_revision: current_inventory_revision.clone(),
            after: all_groups[end - 1].key.clone(),
        })
    });
    let groups = all_groups.into_iter().skip(start).take(limit).collect();
    Json(ListResponse {
        catalog_revision: current_catalog_revision,
        config_revision: current_config_revision,
        membership_revision: inventory.membership_revision.clone(),
        inventory_revision: current_inventory_revision,
        kind: api_kind.kind,
        config_data_type: filter,
        clusters,
        groups,
        continue_token: next,
    })
    .into_response()
}

fn required_cluster_query(
    query: Result<Query<Vec<(String, String)>>, QueryRejection>,
) -> Result<String, &'static str> {
    let pairs = match query {
        Ok(Query(pairs)) => pairs,
        Err(_) => return Err("invalid_query"),
    };
    if pairs.len() != 1
        || pairs[0].0 != "cluster"
        || pairs[0].1.is_empty()
        || pairs[0].1.trim() != pairs[0].1
    {
        return Err("cluster_required");
    }
    Ok(pairs[0].1.clone())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DetailResponse {
    kind: GlobalResourceInventoryKind,
    namespace: String,
    name: String,
    cluster: String,
    state: ClusterResolutionState,
    controller_id: Option<String>,
    candidates: Vec<String>,
    complete: bool,
    errors: Vec<PublicInventoryError>,
    object: Option<Value>,
}

pub async fn detail(
    State(state): State<ApiState>,
    Path((kind_slug, namespace, name)): Path<(String, String, String)>,
    query: Result<Query<Vec<(String, String)>>, QueryRejection>,
) -> Response {
    let Some(service) = state.global_resources.as_ref() else {
        return service_unavailable();
    };
    let Some(api_kind) = parse_kind(&kind_slug) else {
        return invalid_request("invalid_global_resource_kind");
    };
    let cluster = match required_cluster_query(query) {
        Ok(cluster) => cluster,
        Err(code) => return invalid_request(code),
    };
    let result = match service
        .get_resource(api_kind.kind, &cluster, &namespace, &name)
        .await
    {
        Ok(result) => result,
        Err(error) => return map_core_error(error),
    };
    let object = result.items.first().cloned();
    if result.errors.iter().any(|error| {
        error.code == InventoryErrorCode::ClusterNotFound
            || (error.code == InventoryErrorCode::UpstreamRejected && error.status == Some(404))
    }) {
        return error_response(
            StatusCode::NOT_FOUND,
            "global_resource_not_found",
            "The requested global resource was not found.",
        );
    }
    Json(DetailResponse {
        kind: api_kind.kind,
        namespace,
        name,
        cluster: result.cluster,
        state: result.state,
        controller_id: result.controller_id,
        candidates: result.candidates,
        complete: result.complete,
        errors: result
            .errors
            .iter()
            .map(PublicInventoryError::from)
            .collect(),
        object,
    })
    .into_response()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreflightResponse<T> {
    cluster: String,
    preflight: T,
}

pub async fn preflight(
    State(state): State<ApiState>,
    query: Result<Query<Vec<(String, String)>>, QueryRejection>,
) -> Response {
    let Some(service) = state.global_resources.as_ref() else {
        return service_unavailable();
    };
    let cluster = match required_cluster_query(query) {
        Ok(cluster) => cluster,
        Err(code) => return invalid_request(code),
    };
    match service.preflight_cluster(&cluster).await {
        Ok(preflight) => Json(PreflightResponse { cluster, preflight }).into_response(),
        Err(error) if error.status == 404 => error_response(
            StatusCode::NOT_FOUND,
            "cluster_not_found",
            "The requested cluster was not found.",
        ),
        Err(_) => service_unavailable(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgion_center_runtime::global_resources::ClusterResolutionState;
    use serde_json::json;

    fn cluster(cluster: &str, items: Vec<Value>) -> ClusterInventory {
        ClusterInventory {
            cluster: cluster.to_string(),
            state: ClusterResolutionState::Available,
            controller_id: Some(format!("{cluster}-controller")),
            candidates: vec![format!("{cluster}-controller")],
            items,
            errors: Vec::new(),
            complete: true,
        }
    }

    fn item(kind: &str, namespace: &str, name: &str, data_type: Option<&str>) -> Value {
        let mut value = json!({
            "kind": kind,
            "metadata": {"namespace": namespace, "name": name},
            "spec": {"data": {}}
        });
        if let Some(data_type) = data_type {
            value["spec"]["data"]["type"] = Value::String(data_type.to_string());
        }
        value
    }

    #[test]
    fn kind_slugs_are_stable_and_core_paths_are_compatible() {
        assert_eq!(
            parse_kind("http-route").map(|entry| entry.kind),
            Some(GlobalResourceInventoryKind::HTTPRoute)
        );
        assert_eq!(
            parse_kind("httproute").map(|entry| entry.kind),
            Some(GlobalResourceInventoryKind::HTTPRoute)
        );
        assert!(parse_kind("HTTPRoute").is_none());
        assert!(parse_kind("http-route-v2").is_none());
    }

    #[test]
    fn config_data_filter_is_exact_and_unknown_is_explicit() {
        assert_eq!(
            parse_config_data_filter(Some("IpList")),
            Ok(Some(ConfigDataFilter::IpList))
        );
        assert_eq!(
            parse_config_data_filter(Some("Unknown")),
            Ok(Some(ConfigDataFilter::Unknown))
        );
        assert!(parse_config_data_filter(Some("iplist")).is_err());
        assert_eq!(
            item_config_data_type(&item("EdgionConfigData", "edgion-data", "x", None)),
            ConfigDataFilter::Unknown
        );
        assert_eq!(
            item_config_data_type(&item(
                "EdgionConfigData",
                "edgion-data",
                "x",
                Some("FutureType")
            )),
            ConfigDataFilter::Unknown
        );
    }

    #[test]
    fn list_query_accepts_repeated_clusters_and_rejects_duplicate_scalars() {
        let query = parse_list_query(vec![
            ("cluster".into(), "cluster-a".into()),
            ("cluster".into(), "cluster-b".into()),
            ("configDataType".into(), "IpList".into()),
            ("limit".into(), "25".into()),
        ])
        .unwrap();
        assert_eq!(query.cluster, ["cluster-a", "cluster-b"]);
        assert_eq!(query.config_data_type.as_deref(), Some("IpList"));
        assert_eq!(query.limit, Some(25));
        assert!(parse_list_query(vec![
            ("limit".into(), "25".into()),
            ("limit".into(), "50".into())
        ])
        .is_err());
        assert!(parse_list_query(vec![("unknown".into(), "value".into())]).is_err());
    }

    #[tokio::test]
    async fn axum_query_extractor_preserves_repeated_cluster_pairs() {
        use axum::{routing::get, Router};
        use tower::ServiceExt;

        async fn probe(query: Result<Query<Vec<(String, String)>>, QueryRejection>) -> StatusCode {
            match query {
                Ok(Query(pairs))
                    if pairs
                        == [
                            ("cluster".to_string(), "cluster-a".to_string()),
                            ("cluster".to_string(), "cluster-b".to_string()),
                        ] =>
                {
                    StatusCode::OK
                }
                _ => StatusCode::BAD_REQUEST,
            }
        }

        let response = Router::new()
            .route("/", get(probe))
            .oneshot(
                axum::http::Request::builder()
                    .uri("/?cluster=cluster-a&cluster=cluster-b")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn continuation_token_is_canonical_bounded_and_query_bound() {
        let token = ContinueToken {
            catalog_revision: "catalog-1".into(),
            config_revision: "config-1".into(),
            kind: "edgion-config-data".into(),
            query_revision: cluster_query_revision(&["cluster-b".into()]),
            membership_revision: "membership-1".into(),
            limit: 50,
            config_data_type: Some(ConfigDataFilter::IpList),
            inventory_revision: "inventory-1".into(),
            after: GroupKey {
                kind: "EdgionConfigData".into(),
                namespace: "edgion-data".into(),
                name: "allowlist".into(),
            },
        };
        let encoded = encode_token(&token);
        assert!(encoded.starts_with(TOKEN_PREFIX));
        let decoded = decode_token(&encoded).unwrap();
        assert_eq!(decoded.kind, token.kind);
        assert_eq!(decoded.query_revision, token.query_revision);
        assert_eq!(decoded.membership_revision, token.membership_revision);
        assert_eq!(decoded.limit, token.limit);
        assert_eq!(decoded.config_data_type, token.config_data_type);
        assert_eq!(decoded.inventory_revision, token.inventory_revision);
        assert_eq!(decoded.after, token.after);
        assert!(decode_token("gr1.=").is_err());
        assert!(decode_token(&format!("gr1.{}", "a".repeat(MAX_TOKEN_BYTES))).is_err());
    }

    #[test]
    fn grouping_uses_kind_namespace_name_and_preserves_each_raw_object() {
        let shared_a = item("HTTPRoute", "edgion-system", "shared", None);
        let mut shared_b = shared_a.clone();
        shared_b["spec"]["hostnames"] = json!(["b.example"]);
        let groups = group_inventory(
            GlobalResourceInventoryKind::HTTPRoute,
            &[
                cluster("cluster-b", vec![shared_b.clone()]),
                cluster("cluster-a", vec![shared_a.clone()]),
            ],
            None,
        );
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key.kind, "HTTPRoute");
        assert_eq!(groups[0].key.namespace, "edgion-system");
        assert_eq!(groups[0].key.name, "shared");
        assert_eq!(groups[0].members[0].cluster, "cluster-a");
        assert_eq!(groups[0].members[0].object, shared_a);
        assert_eq!(groups[0].members[1].cluster, "cluster-b");
        assert_eq!(groups[0].members[1].object, shared_b);
    }

    #[test]
    fn grouping_filters_config_data_by_exact_nested_type() {
        let groups = group_inventory(
            GlobalResourceInventoryKind::EdgionConfigData,
            &[cluster(
                "cluster-a",
                vec![
                    item("EdgionConfigData", "edgion-data", "ips", Some("IpList")),
                    item(
                        "EdgionConfigData",
                        "edgion-data",
                        "future",
                        Some("FutureType"),
                    ),
                ],
            )],
            Some(ConfigDataFilter::Unknown),
        );
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].key.name, "future");
    }

    #[test]
    fn config_data_filter_keeps_every_member_of_a_matching_group() {
        let groups = group_inventory(
            GlobalResourceInventoryKind::EdgionConfigData,
            &[
                cluster(
                    "cluster-a",
                    vec![item(
                        "EdgionConfigData",
                        "edgion-data",
                        "shared",
                        Some("IpList"),
                    )],
                ),
                cluster(
                    "cluster-b",
                    vec![item(
                        "EdgionConfigData",
                        "edgion-data",
                        "shared",
                        Some("FutureType"),
                    )],
                ),
            ],
            Some(ConfigDataFilter::IpList),
        );
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].members.len(), 2);
    }

    #[test]
    fn inventory_revision_covers_membership_errors_keys_and_raw_objects() {
        let mut clusters = vec![ClusterResult {
            cluster: "cluster-a".into(),
            state: ClusterResolutionState::Available,
            controller_id: Some("controller-a".into()),
            candidates: vec!["controller-a".into()],
            complete: true,
            errors: Vec::new(),
        }];
        let groups = group_inventory(
            GlobalResourceInventoryKind::HTTPRoute,
            &[cluster(
                "cluster-a",
                vec![item("HTTPRoute", "edgion-system", "route", None)],
            )],
            None,
        );
        let original = inventory_revision(&clusters, &groups);
        clusters[0].complete = false;
        assert_ne!(inventory_revision(&clusters, &groups), original);

        let mut changed_groups = groups;
        changed_groups[0].members[0].object["spec"]["hostnames"] = json!(["changed.example"]);
        assert_ne!(inventory_revision(&clusters, &changed_groups), original);
    }
}
