//! Shared consistency-report DTO for Center.
//!
//! The RegionRoute consistency endpoint that used to live here was removed
//! together with the `region_routes` MetaDataStore map: that map had no
//! production writer, so the endpoint always reported on an empty set.
//! RegionRoute state is served from the federation watch model through the
//! `region-route-overrides` endpoints instead.
//!
//! [`ConsistencyResult`] stays because the GlobalConnectionIpRestriction
//! consistency handler still returns it.

use serde::Serialize;

/// One row in a consistency response.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsistencyResult {
    pub namespace: String,
    /// Resource identifier within the namespace.
    pub name: String,
    pub consistent: bool,
    /// Number of online controllers that reported this key.
    pub controller_count: usize,
    /// Field names that differ across online controllers (empty when consistent).
    pub conflicts: Vec<String>,
}
