use k8s_openapi::apimachinery::pkg::apis::meta::v1::Time;
use kube::{CustomResource, KubeSchema};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Durable, namespaced projection of a federation Controller identity.
///
/// Registration metadata is repeated in status because it belongs to the
/// observed connection, while `spec.controller_id` is the immutable collision
/// guard for the deterministic Kubernetes object name.
#[derive(CustomResource, KubeSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[kube(
    group = "center.edgion.io",
    version = "v1alpha1",
    kind = "EdgionController",
    plural = "edgioncontrollers",
    namespaced,
    status = "EdgionControllerStatus",
    shortname = "ec"
)]
#[serde(rename_all = "camelCase")]
pub struct EdgionControllerSpec {
    /// Canonical federation controller id. This remains authoritative even
    /// when the id cannot be represented as a Kubernetes DNS label.
    #[x_kube(validation = "self != ''", validation = "self == oldSelf")]
    #[schemars(length(max = 253))]
    pub controller_id: String,
    /// Initial cluster identity, retained for operator readability.
    #[serde(default)]
    pub cluster: String,
    /// Initial environment labels. Current observed values live in status.
    #[serde(default)]
    pub environments: Vec<String>,
    /// Initial free-form tags. Current observed values live in status.
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "PascalCase")]
pub enum EdgionControllerPhase {
    Online,
    Offline,
    Stale,
}

/// Revision-fenced observed state written through the status subresource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EdgionControllerStatus {
    pub phase: EdgionControllerPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cluster: String,
    #[serde(default)]
    pub environments: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connected_replica: Option<String>,
    /// Opaque token of the Lease acquisition that owns this projection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ownership_token: Option<String>,
    /// Strictly increasing acquire epoch used for cross-replica fencing.
    #[serde(default)]
    pub ownership_epoch: u64,
    /// Last successfully applied watch revision and source server for the
    /// current session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_version: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watch_server_id: Option<String>,
    /// Total resource count from the latest StatsReport.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats_updated_unix_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watch_updated_unix_ms: Option<i64>,
    /// Kubernetes date-time value for kubectl and generic clients.
    pub last_seen_time: Time,
    /// Observation time for display and legacy standalone-compatible ordering;
    /// Kubernetes ownership correctness uses `ownershipEpoch` instead.
    pub observed_at_unix_ms: i64,
    /// Hidden durable eviction fence. Fenced objects remain in Kubernetes so a
    /// delayed offline write cannot recreate them.
    #[serde(default)]
    pub evicted: bool,
    /// Object generation observed while writing this projection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,
}
