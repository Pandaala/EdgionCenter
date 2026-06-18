//! Background poller: fetches the controllers' /effective views and feeds the
//! aggregator store. Replaces the dead fed_sync RegionRoute/GIR feed.

use std::collections::HashMap;

use crate::metadata_store::{CenterMetaDataStore, EffectiveGirView, EffectiveRegionRouteView};
use crate::proxy::ProxyForwarder;

/// Tolerant element-wise parse: a single bad element is dropped, not the whole batch.
/// A non-JSON body yields an empty Vec.
pub fn parse_region_effective(body: &[u8]) -> Vec<EffectiveRegionRouteView> {
    let v: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let arr = v.get("data").and_then(|d| d.as_array()).cloned().unwrap_or_default();
    arr.into_iter()
        .filter_map(|el| serde_json::from_value::<EffectiveRegionRouteView>(el).ok())
        .collect()
}

/// Tolerant element-wise parse for GIR effective views.
/// A single bad element is dropped, not the whole batch.
pub fn parse_gir_effective(body: &[u8]) -> Vec<EffectiveGirView> {
    let v: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let arr = v.get("data").and_then(|d| d.as_array()).cloned().unwrap_or_default();
    arr.into_iter()
        .filter_map(|el| serde_json::from_value::<EffectiveGirView>(el).ok())
        .collect()
}

/// Poll one controller's two effective endpoints and update the store.
/// On a non-200 response or forwarding error the store is left unchanged for that
/// endpoint (fail-open: keep the previous snapshot rather than clearing it).
pub async fn poll_controller_once(
    proxy: &ProxyForwarder,
    store: &CenterMetaDataStore,
    controller_id: &str,
) {
    if let Ok(resp) = proxy
        .forward(
            controller_id,
            "GET".to_string(),
            "/api/v1/region-routes/effective".to_string(),
            HashMap::new(),
            Vec::new(),
        )
        .await
    {
        if resp.status_code == 200 {
            store.replace_region_routes(controller_id, parse_region_effective(&resp.body));
        }
    }
    if let Ok(resp) = proxy
        .forward(
            controller_id,
            "GET".to_string(),
            "/api/v1/global-ip-restrictions/effective".to_string(),
            HashMap::new(),
            Vec::new(),
        )
        .await
    {
        if resp.status_code == 200 {
            store.replace_gir(controller_id, parse_gir_effective(&resp.body));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tolerant_parse_drops_bad_keeps_good() {
        let body = br#"{"success":true,"data":[
            {"namespace":"default","pluginName":"ep1","myRegion":"east","regions":[]},
            {"garbage":true}
        ]}"#;
        let parsed = parse_region_effective(body);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].plugin_name, "ep1");
    }

    #[test]
    fn tolerant_parse_non_json_body_yields_empty() {
        let parsed = parse_region_effective(b"not json at all");
        assert!(parsed.is_empty());
    }

    #[test]
    fn tolerant_parse_missing_data_field_yields_empty() {
        let parsed = parse_region_effective(br#"{"success":true}"#);
        assert!(parsed.is_empty());
    }

    #[test]
    fn tolerant_parse_gir_drops_bad_keeps_good() {
        let body = br#"{"success":true,"data":[
            {"namespace":"default","pluginName":"gir1","enable":true,"activeProfile":"strict","profiles":{}},
            {"garbage":true}
        ]}"#;
        let parsed = parse_gir_effective(body);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].plugin_name, "gir1");
    }
}
