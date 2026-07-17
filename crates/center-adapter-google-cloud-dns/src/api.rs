//! SDK-free Google Cloud DNS v1 transport seam and provider DTOs.

use async_trait::async_trait;
use edgion_center_core::NormalizedProviderError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub type GoogleCloudDnsApiResult<T> = Result<T, NormalizedProviderError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GoogleZoneVisibility {
    Public,
    Private,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoogleZoneKind {
    Authoritative,
    Forwarding,
    Peering,
    ReverseLookup,
    ServiceDirectory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GoogleDnsSecState {
    Off,
    On,
    Transfer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleManagedZone {
    pub id: String,
    pub name: String,
    pub dns_name: String,
    pub visibility: GoogleZoneVisibility,
    pub kind: GoogleZoneKind,
    pub dnssec_state: GoogleDnsSecState,
    #[serde(default)]
    pub name_servers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleManagedZoneCreate {
    pub name: String,
    pub dns_name: String,
    pub visibility: GoogleZoneVisibility,
    pub dnssec_state: GoogleDnsSecState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleDnsKey {
    pub key_tag: u16,
    pub algorithm: u8,
    pub key_type: String,
    pub digests: Vec<GoogleDnsKeyDigest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoogleDnsKeyDigest {
    #[serde(rename = "type")]
    pub digest_type: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoogleManagedZonePage {
    pub items: Vec<GoogleManagedZone>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleLoadBalancerTarget {
    pub load_balancer_type: String,
    pub ip_address: String,
    pub port: String,
    pub ip_protocol: String,
    pub network_url: String,
    pub project: String,
    pub region: String,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleHealthCheckTargets {
    #[serde(default)]
    pub internal_load_balancers: Vec<GoogleLoadBalancerTarget>,
    #[serde(default)]
    pub external_endpoints: Vec<String>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleGeoPolicyItem {
    pub location: String,
    #[serde(default)]
    pub rrdatas: Vec<String>,
    #[serde(default)]
    pub signature_rrdatas: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_checked_targets: Option<GoogleHealthCheckTargets>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleGeoPolicy {
    pub items: Vec<GoogleGeoPolicyItem>,
    pub enable_fencing: bool,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleWrrPolicyItem {
    pub weight: serde_json::Number,
    #[serde(default)]
    pub rrdatas: Vec<String>,
    #[serde(default)]
    pub signature_rrdatas: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_checked_targets: Option<GoogleHealthCheckTargets>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleWrrPolicy {
    pub items: Vec<GoogleWrrPolicyItem>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GooglePrimaryBackupPolicy {
    pub primary_targets: GoogleHealthCheckTargets,
    pub backup_geo_targets: GoogleGeoPolicy,
    pub trickle_traffic: serde_json::Number,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum GoogleRoutingData {
    Geo {
        geo: GoogleGeoPolicy,
    },
    Wrr {
        wrr: GoogleWrrPolicy,
    },
    PrimaryBackup {
        #[serde(rename = "primaryBackup")]
        primary_backup: GooglePrimaryBackupPolicy,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleRoutingPolicy {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check: Option<String>,
    #[serde(flatten)]
    pub routing_data: GoogleRoutingData,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleResourceRecordSet {
    pub name: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub ttl: u32,
    #[serde(default)]
    pub rrdatas: Vec<String>,
    #[serde(default)]
    pub signature_rrdatas: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing_policy: Option<GoogleRoutingPolicy>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GoogleRecordSetPage {
    pub items: Vec<GoogleResourceRecordSet>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleChangeRequest {
    pub additions: Vec<GoogleResourceRecordSet>,
    pub deletions: Vec<GoogleResourceRecordSet>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleChange {
    pub id: String,
    pub status: String,
    pub start_time: String,
    pub is_serving: bool,
    #[serde(default)]
    pub additions: Vec<GoogleResourceRecordSet>,
    #[serde(default)]
    pub deletions: Vec<GoogleResourceRecordSet>,
}

#[async_trait]
pub trait GoogleCloudDnsApi: Send + Sync {
    fn verified_project_id(&self) -> &str;
    async fn get_managed_zone(
        &self,
        zone: &str,
    ) -> GoogleCloudDnsApiResult<Option<GoogleManagedZone>>;
    async fn list_managed_zones(
        &self,
        page_token: Option<&str>,
        max_results: u16,
    ) -> GoogleCloudDnsApiResult<GoogleManagedZonePage>;
    async fn list_record_sets(
        &self,
        zone: &str,
        page_token: Option<&str>,
        max_results: u16,
    ) -> GoogleCloudDnsApiResult<GoogleRecordSetPage>;
    /// Implementations disable automatic retries and map ambiguous post-dispatch failures to UnknownOutcome.
    async fn create_change(
        &self,
        zone: &str,
        request: &GoogleChangeRequest,
    ) -> GoogleCloudDnsApiResult<GoogleChange>;
    async fn get_change(
        &self,
        zone: &str,
        change_id: &str,
    ) -> GoogleCloudDnsApiResult<Option<GoogleChange>>;
    async fn create_managed_zone(
        &self,
        _request: &GoogleManagedZoneCreate,
    ) -> GoogleCloudDnsApiResult<GoogleManagedZone> {
        Err(unsupported("google_zone_create_transport_unavailable"))
    }
    async fn delete_managed_zone(&self, _zone: &str) -> GoogleCloudDnsApiResult<()> {
        Err(unsupported("google_zone_delete_transport_unavailable"))
    }
    async fn set_managed_zone_dnssec(
        &self,
        _zone: &str,
        _state: GoogleDnsSecState,
    ) -> GoogleCloudDnsApiResult<GoogleManagedZone> {
        Err(unsupported("google_dnssec_transport_unavailable"))
    }
    async fn list_dns_keys(&self, _zone: &str) -> GoogleCloudDnsApiResult<Vec<GoogleDnsKey>> {
        Err(unsupported("google_dns_keys_transport_unavailable"))
    }
}

fn unsupported(code: &'static str) -> NormalizedProviderError {
    NormalizedProviderError::new(
        edgion_center_core::ProviderErrorCategory::Validation,
        code,
        "provider capability is unavailable",
        None,
        None,
    )
    .expect("static normalized provider error")
}
