//! Per-Controller resolution state reported by the GlobalResources read model.
//!
//! This module used to also serve the live cross-cluster inventory fan-out
//! (list/detail HTTP requests broadcast to every Controller) and the
//! authoritative-target resolution core behind durable desired-state sync.
//! Both have been retired; GlobalResources is served entirely from the
//! federation watch cache. Only the vocabulary the read model reports to
//! callers survives here.
//!
//! Note that the read model never constructs `Ambiguous` or `Indeterminate`
//! and always leaves `candidates` empty — it emits one row per Controller and
//! does not resolve an authoritative target. The variants stay because the
//! dashboard renders against this wire shape.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClusterResolutionState {
    Available,
    Offline,
    Ambiguous,
    Indeterminate,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClusterResolution {
    pub cluster: String,
    pub state: ClusterResolutionState,
    pub controller_id: Option<String>,
    pub candidates: Vec<String>,
}
