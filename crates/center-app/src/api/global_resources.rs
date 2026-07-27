use super::ApiState;
use crate::watch_cache::CacheStatus;
use axum::{
    extract::{rejection::QueryRejection, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use edgion_center_core::{
    EdgionConfigDataType, GlobalResourceInventoryKind, GLOBAL_RESOURCE_KINDS,
};
use edgion_center_runtime::global_resources::{ClusterResolution, ClusterResolutionState};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ApiKind {
    kind: GlobalResourceInventoryKind,
    slug: &'static str,
}

const API_KINDS: [ApiKind; 1] = [ApiKind {
    kind: GlobalResourceInventoryKind::EdgionConfigData,
    slug: "edgion-config-data",
}];

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

    // Clusters come from the federation watch registry, exactly as the
    // watch-backed list path derives `ClusterResult` — see
    // `list_config_data_from_watch`. This keeps the catalog free of any
    // dependency on the fan-out `global_resources` service.
    let mut clusters: Vec<ClusterResolution> = state
        .sync_client
        .plugin_metadata
        .statuses()
        .into_iter()
        .map(|(controller_id, status)| ClusterResolution {
            cluster: watch_cluster_of(&controller_id).to_string(),
            state: if status.stale {
                ClusterResolutionState::Offline
            } else {
                ClusterResolutionState::Available
            },
            controller_id: Some(controller_id),
            candidates: Vec::new(),
        })
        .collect();
    clusters.sort_by(|a, b| (&a.cluster, &a.controller_id).cmp(&(&b.cluster, &b.controller_id)));

    Json(CatalogResponse {
        catalog_revision: catalog_revision(),
        config_revision: WATCH_CONFIG_REVISION.to_string(),
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
    /// Watch-backed member enrichment (CCI-03). `None` when the entry's
    /// controller has no matching entry in the status snapshot taken
    /// moments earlier (the two registry reads are not atomic) —
    /// `skip_serializing_if` keeps the wire shape byte-identical to before
    /// this field existed.
    #[serde(skip_serializing_if = "Option::is_none")]
    sync_state: Option<WatchSyncState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    freshness_unix_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    revision: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ComparisonGroup {
    key: GroupKey,
    members: Vec<ComparisonMember>,
}

/// Wire-only inventory error code. Nothing in the surviving watch-backed
/// path constructs one (`ClusterResult`/`DetailResponse` always emit an
/// empty `errors` vec), but the field stays on the wire shape because the
/// dashboard drawer still renders it and has tests against the shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
enum InventoryErrorCode {
    ClusterNotFound,
    ControllerOffline,
    ControllerAmbiguous,
    OwnershipIndeterminate,
    UpstreamUnavailable,
    UpstreamRejected,
    PayloadTooLarge,
    InvalidUpstreamResponse,
    BudgetExceeded,
    PaginationInvalid,
    PaginationLimit,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicInventoryError {
    namespace: String,
    code: InventoryErrorCode,
    status: Option<u16>,
    retryable: bool,
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
    /// Watch-backed cluster enrichment (CCI-03). Always populated on this
    /// path — see `ComparisonMember`'s matching fields for the one case
    /// where the analogous per-member enrichment can still be `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    sync_state: Option<WatchSyncState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    freshness_unix_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    revision: Option<u64>,
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

// ---- watch-backed EdgionConfigData path (CCI-03/CCI-04) ----
//
// GlobalResources reads `state.sync_client.plugin_metadata`, the federation
// watch cache read model, which is kept current by the ongoing watch stream
// and already redacts non-passthrough config types (see
// `watch_cache::read_model` doc comments). There is no Controller HTTP call
// anywhere on this read path, and no separate fan-out service to compose
// against — this module is the entire GlobalResources surface.

/// `config_revision` sentinel for watch-backed responses/tokens. There is no
/// separate desired-state config object to report a revision for, so the
/// only requirement is that the value stays stable so continuation tokens
/// round-trip correctly across pages of the same query.
const WATCH_CONFIG_REVISION: &str = "watch-read-model";

#[derive(Debug, Serialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "camelCase")]
enum WatchSyncState {
    Ok,
    Stale,
    Overflowed,
}

/// Classifies a controller's watch cache freshness for API consumers.
/// Overflow (the controller's last batch was rejected for exceeding the
/// per-controller entry cap) takes precedence over mere staleness
/// (controller offline / never synced).
fn watch_sync_state(status: &CacheStatus) -> WatchSyncState {
    if status.overflowed {
        WatchSyncState::Overflowed
    } else if status.stale {
        WatchSyncState::Stale
    } else {
        WatchSyncState::Ok
    }
}

/// The cluster name encoded in a watch controller id (`"<cluster>/<suffix>"`).
/// Falls back to the whole id when there is no `/` — defensive only;
/// production controller ids always carry a cluster prefix.
fn watch_cluster_of(controller_id: &str) -> &str {
    controller_id
        .split_once('/')
        .map_or(controller_id, |(cluster, _)| cluster)
}

/// Maps the app-level ConfigData filter to the exact `/spec/data/type` wire
/// string `CenterWatchCacheRegistry::list_all` matches against.
fn watch_type_filter(filter: ConfigDataFilter) -> &'static str {
    match filter {
        ConfigDataFilter::KeyList => "KeyList",
        ConfigDataFilter::IpList => "IpList",
        ConfigDataFilter::Selector => "Selector",
        ConfigDataFilter::RegionRouteOverride => "RegionRouteOverride",
        ConfigDataFilter::Misc => "Misc",
        ConfigDataFilter::Unknown => "Unknown",
    }
}

/// sha256 over the sorted `(controller_id, revision, stale, overflowed)`
/// tuples for every controller in `statuses` — a compact membership
/// fingerprint used for continuation-token staleness checks.
fn watch_membership_revision(statuses: &[(String, CacheStatus)]) -> String {
    let mut rows: Vec<(&str, u64, bool, bool)> = statuses
        .iter()
        .map(|(id, status)| {
            (
                id.as_str(),
                status.revision,
                status.stale,
                status.overflowed,
            )
        })
        .collect();
    rows.sort();
    let canonical = serde_json::to_vec(&rows).expect("membership rows are serializable");
    let digest = Sha256::digest(canonical);
    let hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!("sha256:{hex}")
}

/// sha256 over the canonical JSON of the response's `clusters`/`groups`
/// payload, delegating straight to `inventory_revision` — kept as a
/// separate named entry point so the watch-backed list handler's intent
/// reads clearly at the call site.
fn watch_inventory_revision(clusters: &[ClusterResult], groups: &[ComparisonGroup]) -> String {
    inventory_revision(clusters, groups)
}

fn cluster_not_found() -> Response {
    error_response(
        StatusCode::NOT_FOUND,
        "cluster_not_found",
        "The requested cluster was not found.",
    )
}

/// The `edgion-config-data` `ApiKind::slug`, looked up from `API_KINDS`
/// instead of duplicated as a literal so the continuation-token `kind` field
/// can never drift out of sync with the catalog.
fn config_data_kind_slug() -> &'static str {
    API_KINDS
        .iter()
        .find(|entry| entry.kind == GlobalResourceInventoryKind::EdgionConfigData)
        .expect("EdgionConfigData is a registered API kind")
        .slug
}

async fn list_config_data_from_watch(
    state: &ApiState,
    filter: Option<ConfigDataFilter>,
    clusters_param: &[String],
    limit: usize,
    continue_token: Option<&str>,
) -> Response {
    let decoded_token = match continue_token.map(decode_token).transpose() {
        Ok(token) => token,
        Err(()) => return invalid_request("invalid_continue_token"),
    };

    let kind_slug = config_data_kind_slug();
    let current_catalog_revision = catalog_revision();
    let current_config_revision = WATCH_CONFIG_REVISION.to_string();
    let current_query_revision = cluster_query_revision(clusters_param);
    if decoded_token.as_ref().is_some_and(|token| {
        token.catalog_revision != current_catalog_revision
            || token.config_revision != current_config_revision
            || token.kind != kind_slug
            || token.query_revision != current_query_revision
            || token.limit != limit
            || token.config_data_type != filter
    }) {
        return invalid_request("stale_continue_token");
    }

    let statuses = state.sync_client.plugin_metadata.statuses();
    let known_clusters: BTreeSet<&str> = statuses
        .iter()
        .map(|(id, _)| watch_cluster_of(id))
        .collect();
    for requested in clusters_param {
        if !known_clusters.contains(requested.as_str()) {
            return cluster_not_found();
        }
    }
    let allowed: Option<BTreeSet<&str>> =
        (!clusters_param.is_empty()).then(|| clusters_param.iter().map(String::as_str).collect());
    let in_scope = |cluster: &str| allowed.as_ref().is_none_or(|set| set.contains(cluster));

    let mut scoped_statuses: Vec<(String, CacheStatus)> = statuses
        .into_iter()
        .filter(|(id, _)| in_scope(watch_cluster_of(id)))
        .collect();
    scoped_statuses.sort_by(|a, b| a.0.cmp(&b.0));

    let status_by_controller: BTreeMap<&str, &CacheStatus> = scoped_statuses
        .iter()
        .map(|(id, status)| (id.as_str(), status))
        .collect();

    let mut clusters: Vec<ClusterResult> = scoped_statuses
        .iter()
        .map(|(controller_id, status)| ClusterResult {
            cluster: watch_cluster_of(controller_id).to_string(),
            state: if status.stale {
                ClusterResolutionState::Offline
            } else {
                ClusterResolutionState::Available
            },
            controller_id: Some(controller_id.clone()),
            candidates: Vec::new(),
            complete: !status.stale && !status.overflowed,
            errors: Vec::new(),
            sync_state: Some(watch_sync_state(status)),
            freshness_unix_ms: status.last_applied_unix_ms,
            revision: Some(status.revision),
        })
        .collect();
    clusters.sort_by(|a, b| (&a.cluster, &a.controller_id).cmp(&(&b.cluster, &b.controller_id)));

    // `statuses()` above and `list_all()` below are two separate snapshots,
    // each taken under its own lock, so a controller that connects between
    // them yields a member with no matching `CacheStatus`. That is accepted,
    // not a defect: `sync_state` / `freshness_unix_ms` / `revision` are all
    // `Option` and render as unknown, and the next request picks the
    // controller up. A single combined accessor would not fix it either —
    // `list_all` locks each per-controller cache individually after the
    // registry snapshot, so no cross-controller atomic point exists short of
    // a global lock over every cache.
    let type_filter = filter.map(watch_type_filter);
    let mut grouped = BTreeMap::<GroupKey, Vec<ComparisonMember>>::new();
    for entry in state.sync_client.plugin_metadata.list_all(type_filter) {
        let cluster = watch_cluster_of(&entry.controller_id).to_string();
        if !in_scope(&cluster) {
            continue;
        }
        let status = status_by_controller
            .get(entry.controller_id.as_str())
            .copied();
        let key = GroupKey {
            kind: GlobalResourceInventoryKind::EdgionConfigData
                .as_str()
                .to_string(),
            namespace: entry.namespace,
            name: entry.name,
        };
        grouped.entry(key).or_default().push(ComparisonMember {
            cluster,
            controller_id: Some(entry.controller_id),
            object: entry.doc,
            sync_state: status.map(watch_sync_state),
            freshness_unix_ms: status.and_then(|status| status.last_applied_unix_ms),
            revision: status.map(|status| status.revision),
        });
    }
    let all_groups: Vec<ComparisonGroup> = grouped
        .into_iter()
        .map(|(key, mut members)| {
            // Same ordering as `clusters` above. `list_all` orders by
            // `controller_id` alone, which diverges from `(cluster,
            // controller_id)` whenever one cluster name is a strict prefix of
            // another ("a" vs "a-b": '-' sorts before '/').
            members.sort_by(|a, b| {
                (&a.cluster, &a.controller_id).cmp(&(&b.cluster, &b.controller_id))
            });
            ComparisonGroup { key, members }
        })
        .collect();

    let current_inventory_revision = watch_inventory_revision(&clusters, &all_groups);
    let current_membership_revision = watch_membership_revision(&scoped_statuses);
    if decoded_token.as_ref().is_some_and(|token| {
        token.membership_revision != current_membership_revision
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
            kind: kind_slug.to_string(),
            query_revision: current_query_revision,
            membership_revision: current_membership_revision.clone(),
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
        membership_revision: current_membership_revision,
        inventory_revision: current_inventory_revision,
        kind: GlobalResourceInventoryKind::EdgionConfigData,
        config_data_type: filter,
        clusters,
        groups,
        continue_token: next,
    })
    .into_response()
}

async fn detail_config_data_from_watch(
    state: &ApiState,
    namespace: &str,
    name: &str,
    cluster: &str,
) -> Response {
    let statuses = state.sync_client.plugin_metadata.statuses();
    let known_clusters: BTreeSet<&str> = statuses
        .iter()
        .map(|(id, _)| watch_cluster_of(id))
        .collect();
    if !known_clusters.contains(cluster) {
        return cluster_not_found();
    }

    let entry = state
        .sync_client
        .plugin_metadata
        .list_all(None)
        .into_iter()
        .find(|entry| {
            entry.namespace == namespace
                && entry.name == name
                && watch_cluster_of(&entry.controller_id) == cluster
        });
    let Some(entry) = entry else {
        return error_response(
            StatusCode::NOT_FOUND,
            "global_resource_not_found",
            "The requested global resource was not found.",
        );
    };
    let stale = statuses
        .iter()
        .find(|(id, _)| id == &entry.controller_id)
        .is_some_and(|(_, status)| status.stale);

    Json(DetailResponse {
        kind: GlobalResourceInventoryKind::EdgionConfigData,
        namespace: namespace.to_string(),
        name: name.to_string(),
        cluster: cluster.to_string(),
        state: if stale {
            ClusterResolutionState::Offline
        } else {
            ClusterResolutionState::Available
        },
        controller_id: Some(entry.controller_id),
        candidates: Vec::new(),
        complete: !stale,
        errors: Vec::new(),
        object: Some(entry.doc),
    })
    .into_response()
}

// ---- end watch-backed EdgionConfigData path (CCI-03) ----

pub async fn list(
    State(state): State<ApiState>,
    Path(kind_slug): Path<String>,
    query: Result<Query<Vec<(String, String)>>, QueryRejection>,
) -> Response {
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

    // `API_KINDS` has exactly one entry (`EdgionConfigData`), so `api_kind`
    // is always that kind here; `list` is served straight from the
    // federation watch read model — zero Controller HTTP.
    debug_assert_eq!(api_kind.kind, GlobalResourceInventoryKind::EdgionConfigData);
    list_config_data_from_watch(
        &state,
        filter,
        &requested_clusters,
        limit,
        query.continue_token.as_deref(),
    )
    .await
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
    let Some(api_kind) = parse_kind(&kind_slug) else {
        return invalid_request("invalid_global_resource_kind");
    };
    let cluster = match required_cluster_query(query) {
        Ok(cluster) => cluster,
        Err(code) => return invalid_request(code),
    };

    // `API_KINDS` has exactly one entry (`EdgionConfigData`), so `api_kind`
    // is always that kind here; `detail` is served straight from the
    // federation watch read model — zero Controller HTTP.
    debug_assert_eq!(api_kind.kind, GlobalResourceInventoryKind::EdgionConfigData);
    detail_config_data_from_watch(&state, &namespace, &name, &cluster).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use edgion_center_runtime::global_resources::ClusterResolutionState;
    use parking_lot::Mutex;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::Arc;

    use crate::{
        aggregator::ResourceAggregator,
        fed_sync::registry::ControllerRegistry,
        metadata_store::CenterMetaDataStore,
        proxy::ProxyForwarder,
        watch_cache::{CenterSyncClient, CenterWatchCacheRegistry},
    };

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
    fn inventory_revision_covers_membership_errors_keys_and_raw_objects() {
        let mut clusters = vec![ClusterResult {
            cluster: "cluster-a".into(),
            state: ClusterResolutionState::Available,
            controller_id: Some("controller-a".into()),
            candidates: vec!["controller-a".into()],
            complete: true,
            errors: Vec::new(),
            sync_state: None,
            freshness_unix_ms: None,
            revision: None,
        }];
        let object = item("HTTPRoute", "edgion-system", "route", None);
        let mut groups = vec![ComparisonGroup {
            key: GroupKey {
                kind: "HTTPRoute".into(),
                namespace: "edgion-system".into(),
                name: "route".into(),
            },
            members: vec![ComparisonMember {
                cluster: "cluster-a".into(),
                controller_id: Some("cluster-a-controller".into()),
                object,
                sync_state: None,
                freshness_unix_ms: None,
                revision: None,
            }],
        }];
        let original = inventory_revision(&clusters, &groups);
        clusters[0].complete = false;
        assert_ne!(inventory_revision(&clusters, &groups), original);

        clusters[0].complete = true;
        groups[0].members[0].object["spec"]["hostnames"] = json!(["changed.example"]);
        assert_ne!(inventory_revision(&clusters, &groups), original);
    }

    // ---- watch-backed EdgionConfigData path (CCI-03) ----

    /// Minimal `ApiState` with a real `CenterWatchCacheRegistry`. GlobalResources
    /// is served entirely from the federation watch read model (CCI-04), so
    /// tests seed data straight into `state.sync_client.plugin_metadata` and
    /// call the `list`/`detail` handler functions directly.
    fn watch_test_state() -> ApiState {
        let registry = ControllerRegistry::new();
        let metadata_store = Arc::new(CenterMetaDataStore::new());
        let sync_client = Arc::new(CenterSyncClient {
            plugin_metadata: CenterWatchCacheRegistry::new(metadata_store.clone()),
        });
        let proxy = Arc::new(ProxyForwarder::new(
            registry.clone(),
            Arc::new(Mutex::new(HashMap::new())),
            5,
        ));
        ApiState {
            aggregator: Arc::new(ResourceAggregator::new()),
            proxy,
            controller_directory: None,
            controller_evictor: Arc::new(edgion_center_runtime::eviction::NoopControllerEvictor),
            user_admin: None,
            role_admin: None,
            audit_reader: None,
            cloudflare_dns_admin: None,
            cloudflare_dns_write_admin: None,
            cloudflare_waf_admin: None,
            route53_dns_admin: None,
            route53_dns_write_admin: None,
            route53_zone_lifecycle_admin: None,
            cloudfront_admin: None,
            aws_waf_admin: None,
            provider_account_store: None,
            capability_snapshot_store: None,
            credential_inspection_service: None,
            metadata_store,
            sync_client,
            registry,
            platform_ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            authz_mode: edgion_center_core::AuthzMode::AllowAll,
            platform_mode: edgion_center_core::CenterMode::Standalone,
            capabilities: edgion_center_core::CenterCapabilities::for_mode(
                edgion_center_core::CenterMode::Standalone,
            ),
        }
    }

    /// A `pm_json`-shaped (see federation/server.rs) EdgionConfigData
    /// document for seeding the watch cache directly.
    fn watch_doc(namespace: &str, name: &str, config_type: &str, config: Value) -> Value {
        json!({
            "apiVersion": "edgion.io/v1",
            "kind": "EdgionConfigData",
            "metadata": {"namespace": namespace, "name": name},
            "spec": {"enable": true, "data": {"type": config_type, "config": config}}
        })
    }

    async fn call_list(
        state: &ApiState,
        kind: &str,
        pairs: Vec<(String, String)>,
    ) -> (StatusCode, Value) {
        let response = list(
            State(state.clone()),
            Path(kind.to_string()),
            Ok(Query(pairs)),
        )
        .await;
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn call_detail(
        state: &ApiState,
        kind: &str,
        namespace: &str,
        name: &str,
        pairs: Vec<(String, String)>,
    ) -> (StatusCode, Value) {
        let response = detail(
            State(state.clone()),
            Path((kind.to_string(), namespace.to_string(), name.to_string())),
            Ok(Query(pairs)),
        )
        .await;
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    async fn watch_list_serves_config_data_without_controller_http() {
        let state = watch_test_state();
        state
            .sync_client
            .plugin_metadata
            .get_or_create("cluster-a/c0")
            .replace_all(
                vec![(
                    "ns/shared".to_string(),
                    watch_doc("ns", "shared", "KeyList", json!({"items": []})),
                )],
                1,
                "server-1".to_string(),
            );
        state
            .sync_client
            .plugin_metadata
            .get_or_create("cluster-b/c0")
            .replace_all(
                vec![(
                    "ns/shared".to_string(),
                    watch_doc("ns", "shared", "KeyList", json!({"items": []})),
                )],
                1,
                "server-1".to_string(),
            );

        let (status, body) = call_list(&state, "edgion-config-data", vec![]).await;
        assert_eq!(status, StatusCode::OK);
        let groups = body["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["key"]["namespace"], "ns");
        assert_eq!(groups[0]["key"]["name"], "shared");
        let members = groups[0]["members"].as_array().unwrap();
        assert_eq!(members.len(), 2);
        for member in members {
            assert_eq!(member["syncState"], "ok");
            assert!(member["freshnessUnixMs"].is_u64());
            assert_eq!(member["revision"], 1);
        }
        let clusters = body["clusters"].as_array().unwrap();
        assert_eq!(clusters.len(), 2);
        assert!(clusters.iter().all(|c| c["state"] == "available"));
    }

    #[tokio::test]
    async fn watch_list_type_filter_and_misc_redaction() {
        const SECRET: &str = "swordfish-super-secret-token-9f3a";
        let state = watch_test_state();
        state
            .sync_client
            .plugin_metadata
            .get_or_create("cluster-a/c0")
            .replace_all(
                vec![
                    (
                        "ns/ips".to_string(),
                        watch_doc("ns", "ips", "IpList", json!({"cidrs": ["10.0.0.0/8"]})),
                    ),
                    (
                        "ns/secret".to_string(),
                        watch_doc("ns", "secret", "Misc", json!({"token": SECRET})),
                    ),
                ],
                1,
                "server-1".to_string(),
            );

        let (status, body) = call_list(
            &state,
            "edgion-config-data",
            vec![("configDataType".to_string(), "Misc".to_string())],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert!(!body.to_string().contains(SECRET));
        let groups = body["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["key"]["name"], "secret");
        assert!(groups[0]["members"][0]["object"]
            .pointer("/spec/data/config")
            .is_none());

        let (status, body) = call_list(
            &state,
            "edgion-config-data",
            vec![("configDataType".to_string(), "IpList".to_string())],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let groups = body["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0]["key"]["name"], "ips");
        assert_eq!(
            groups[0]["members"][0]["object"]["spec"]["data"]["config"]["cidrs"][0],
            "10.0.0.0/8"
        );
    }

    #[tokio::test]
    async fn watch_list_pagination_and_stale_token() {
        let state = watch_test_state();
        let cache = state
            .sync_client
            .plugin_metadata
            .get_or_create("cluster-a/c0");
        cache.replace_all(
            vec![
                (
                    "ns/a".to_string(),
                    watch_doc("ns", "a", "KeyList", json!({})),
                ),
                (
                    "ns/b".to_string(),
                    watch_doc("ns", "b", "KeyList", json!({})),
                ),
            ],
            1,
            "server-1".to_string(),
        );

        let (status, first) = call_list(
            &state,
            "edgion-config-data",
            vec![("limit".to_string(), "1".to_string())],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(first["groups"].as_array().unwrap().len(), 1);
        assert_eq!(first["groups"][0]["key"]["name"], "a");
        let token = first["continueToken"]
            .as_str()
            .expect("first page has a continuation token")
            .to_string();

        let (status, second) = call_list(
            &state,
            "edgion-config-data",
            vec![
                ("limit".to_string(), "1".to_string()),
                ("continue".to_string(), token.clone()),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(second["groups"].as_array().unwrap().len(), 1);
        assert_eq!(second["groups"][0]["key"]["name"], "b");
        assert!(second["continueToken"].is_null());

        // Mutate the cache: the old token's membership/inventory fingerprint
        // no longer matches, so re-using it must fail closed.
        cache.replace_all(
            vec![
                (
                    "ns/a".to_string(),
                    watch_doc("ns", "a", "KeyList", json!({})),
                ),
                (
                    "ns/b".to_string(),
                    watch_doc("ns", "b", "KeyList", json!({})),
                ),
                (
                    "ns/c".to_string(),
                    watch_doc("ns", "c", "KeyList", json!({})),
                ),
            ],
            2,
            "server-1".to_string(),
        );
        let (status, body) = call_list(
            &state,
            "edgion-config-data",
            vec![
                ("limit".to_string(), "1".to_string()),
                ("continue".to_string(), token),
            ],
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["error"]["code"], "stale_continue_token");
    }

    #[tokio::test]
    async fn watch_list_cluster_filter_and_stale_state() {
        let state = watch_test_state();
        state
            .sync_client
            .plugin_metadata
            .get_or_create("cluster-a/c0")
            .replace_all(
                vec![(
                    "ns/a".to_string(),
                    watch_doc("ns", "a", "KeyList", json!({})),
                )],
                1,
                "server-1".to_string(),
            );
        state
            .sync_client
            .plugin_metadata
            .get_or_create("cluster-b/c0")
            .replace_all(
                vec![(
                    "ns/a".to_string(),
                    watch_doc("ns", "a", "KeyList", json!({})),
                )],
                1,
                "server-1".to_string(),
            );
        state
            .sync_client
            .plugin_metadata
            .mark_offline("cluster-b/c0");

        let (status, body) = call_list(&state, "edgion-config-data", vec![]).await;
        assert_eq!(status, StatusCode::OK);
        let clusters = body["clusters"].as_array().unwrap();
        let offline = clusters
            .iter()
            .find(|c| c["cluster"] == "cluster-b")
            .expect("cluster-b is reported");
        assert_eq!(offline["state"], "offline");
        let members = body["groups"][0]["members"].as_array().unwrap();
        let stale_member = members
            .iter()
            .find(|m| m["cluster"] == "cluster-b")
            .expect("cluster-b's member is present");
        assert_eq!(stale_member["syncState"], "stale");

        let (status, narrowed) = call_list(
            &state,
            "edgion-config-data",
            vec![("cluster".to_string(), "cluster-a".to_string())],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(narrowed["clusters"].as_array().unwrap().len(), 1);
        assert_eq!(
            narrowed["groups"][0]["members"].as_array().unwrap().len(),
            1
        );
        assert_eq!(narrowed["groups"][0]["members"][0]["cluster"], "cluster-a");

        let (status, body) = call_list(
            &state,
            "edgion-config-data",
            vec![("cluster".to_string(), "cluster-zzz".to_string())],
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "cluster_not_found");
    }

    async fn call_catalog(state: &ApiState) -> (StatusCode, Value) {
        let response = catalog(State(state.clone())).await;
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    async fn catalog_is_served_from_the_watch_registry() {
        let state = watch_test_state();
        state
            .sync_client
            .plugin_metadata
            .get_or_create("cluster-a/c0")
            .replace_all(
                vec![(
                    "ns/a".to_string(),
                    watch_doc("ns", "a", "KeyList", json!({})),
                )],
                1,
                "server-1".to_string(),
            );
        state
            .sync_client
            .plugin_metadata
            .get_or_create("cluster-b/c0")
            .replace_all(
                vec![(
                    "ns/a".to_string(),
                    watch_doc("ns", "a", "KeyList", json!({})),
                )],
                1,
                "server-1".to_string(),
            );
        state
            .sync_client
            .plugin_metadata
            .mark_offline("cluster-b/c0");

        let (status, body) = call_catalog(&state).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["configRevision"], WATCH_CONFIG_REVISION);
        assert!(body.get("platformNamespaces").is_none());
        let kinds = body["kinds"].as_array().unwrap();
        assert_eq!(kinds.len(), API_KINDS.len());

        let clusters = body["clusters"].as_array().unwrap();
        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0]["cluster"], "cluster-a");
        assert_eq!(clusters[0]["state"], "available");
        assert_eq!(clusters[0]["controllerId"], "cluster-a/c0");
        assert_eq!(clusters[0]["candidates"].as_array().unwrap().len(), 0);
        assert_eq!(clusters[1]["cluster"], "cluster-b");
        assert_eq!(clusters[1]["state"], "offline");
        assert_eq!(clusters[1]["controllerId"], "cluster-b/c0");
        assert_eq!(clusters[1]["candidates"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn watch_detail_finds_member_without_http() {
        let state = watch_test_state();
        state
            .sync_client
            .plugin_metadata
            .get_or_create("cluster-a/c0")
            .replace_all(
                vec![(
                    "ns/a".to_string(),
                    watch_doc("ns", "a", "KeyList", json!({"items": []})),
                )],
                1,
                "server-1".to_string(),
            );

        let (status, body) = call_detail(
            &state,
            "edgion-config-data",
            "ns",
            "a",
            vec![("cluster".to_string(), "cluster-a".to_string())],
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["object"]["metadata"]["name"], "a");
        assert_eq!(body["state"], "available");
        assert_eq!(body["complete"], true);

        let (status, body) = call_detail(
            &state,
            "edgion-config-data",
            "ns",
            "missing",
            vec![("cluster".to_string(), "cluster-a".to_string())],
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "global_resource_not_found");

        let (status, body) = call_detail(
            &state,
            "edgion-config-data",
            "ns",
            "a",
            vec![("cluster".to_string(), "cluster-zzz".to_string())],
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"]["code"], "cluster_not_found");
    }
}
