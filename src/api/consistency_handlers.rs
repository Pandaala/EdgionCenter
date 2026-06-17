//! Consistency detection endpoints for Center.
//!
//! GET /api/v1/center/cluster-region-routes/consistency
//!   Compares ClusterRegionRoute data across all controllers and reports conflicts.
//!
//! GET /api/v1/center/service-region-routes/consistency
//!   Compares ServiceRegionRoute data across all controllers and reports conflicts.
//!
//! NOTE(migration): Both handlers are stubbed to return an empty report list.
//! ClusterRegionRouteEntry and ServiceRegionRouteEntry were deleted upstream
//! (PluginMetaData → EdgionConfigData migration). Restore from git history when
//! RegionRoute consistency detection is re-implemented on EdgionConfigData.

use std::collections::HashMap;

use axum::extract::State;
use axum::Json;
use serde::Serialize;

use super::ApiState;

// ---------------------------------------------------------------------------
// Report DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsistencyReport {
    pub namespace: String,
    pub name: String,
    pub consistent: bool,
    pub controller_count: usize,
    pub conflicts: Vec<ConflictDetail>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictDetail {
    pub field: String,
    pub values: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /api/v1/center/cluster-region-routes/consistency`
///
/// NOTE(migration): Stubbed to empty — ClusterRegionRouteEntry was deleted upstream.
/// Restore comparison logic from git history when RegionRoute is re-implemented.
pub async fn cluster_region_routes_consistency(_state: State<ApiState>) -> Json<serde_json::Value> {
    // NOTE(migration): ClusterRegionRouteEntry deleted upstream; returns empty report.
    let reports: Vec<ConsistencyReport> = Vec::new();
    Json(serde_json::json!({ "success": true, "data": reports }))
}

/// `GET /api/v1/center/service-region-routes/consistency`
///
/// NOTE(migration): Stubbed to empty — ServiceRegionRouteEntry was deleted upstream.
/// Restore comparison logic from git history when RegionRoute is re-implemented.
pub async fn service_region_routes_consistency(_state: State<ApiState>) -> Json<serde_json::Value> {
    // NOTE(migration): ServiceRegionRouteEntry deleted upstream; returns empty report.
    let reports: Vec<ConsistencyReport> = Vec::new();
    Json(serde_json::json!({ "success": true, "data": reports }))
}
