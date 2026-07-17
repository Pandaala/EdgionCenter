use async_trait::async_trait;
use edgion_center_core::NormalizedProviderError;
use serde::{Deserialize, Serialize};

pub type Route53ApiResult<T> = Result<T, NormalizedProviderError>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route53HostedZone {
    pub id: String,
    pub name: String,
    pub private_zone: bool,
    pub caller_reference: String,
    pub resource_record_set_count: u64,
    #[serde(default)]
    pub name_servers: Vec<String>,
    #[serde(default)]
    pub has_linked_service: bool,
    #[serde(default)]
    pub has_unsupported_features: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route53CreateHostedZoneRequest {
    pub name: String,
    pub caller_reference: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route53CreateHostedZoneResult {
    pub hosted_zone: Route53HostedZone,
    pub change: Route53ChangeInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route53KeySigningKey {
    pub status: String,
    pub ds_record: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route53DnssecInfo {
    pub serve_signature: String,
    #[serde(default)]
    pub key_signing_keys: Vec<Route53KeySigningKey>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route53HostedZonePage {
    pub items: Vec<Route53HostedZone>,
    pub is_truncated: bool,
    pub next_marker: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route53RecordCursor {
    pub name: String,
    pub record_type: String,
    pub set_identifier: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route53RecordPage {
    pub items: Vec<Route53RecordSet>,
    pub is_truncated: bool,
    pub next: Option<Route53RecordCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route53AliasTargetData {
    pub hosted_zone_id: String,
    pub dns_name: String,
    pub evaluate_target_health: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route53GeoLocationData {
    pub continent_code: Option<String>,
    pub country_code: Option<String>,
    pub subdivision_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route53RecordSet {
    pub name: String,
    pub record_type: String,
    pub ttl: Option<u32>,
    #[serde(default)]
    pub resource_records: Vec<String>,
    pub alias_target: Option<Route53AliasTargetData>,
    pub set_identifier: Option<String>,
    pub weight: Option<u8>,
    pub failover: Option<String>,
    pub region: Option<String>,
    pub geolocation: Option<Route53GeoLocationData>,
    pub multivalue_answer: Option<bool>,
    pub health_check_id: Option<String>,
    pub traffic_policy_instance_id: Option<String>,
    #[serde(default)]
    pub has_cidr_routing_config: bool,
    #[serde(default)]
    pub has_geoproximity_location: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Route53ChangeAction {
    Create,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route53RecordChange {
    pub action: Route53ChangeAction,
    pub record_set: Route53RecordSet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Route53ChangeBatch {
    pub changes: Vec<Route53RecordChange>,
    pub comment: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route53ChangeInfo {
    pub id: String,
    pub status: String,
    pub submitted_at_unix_seconds: i64,
    pub comment: Option<String>,
}

#[async_trait]
pub trait Route53Api: Send + Sync {
    /// AWS account ID established by the credential-owning client with STS
    /// `GetCallerIdentity` before this seam is injected into the adapter.
    fn verified_account_id(&self) -> &str;

    /// Creates a public hosted zone. Private creation requires an initial VPC,
    /// which is intentionally not accepted by this narrower seam.
    async fn create_hosted_zone(
        &self,
        request: &Route53CreateHostedZoneRequest,
    ) -> Route53ApiResult<Route53CreateHostedZoneResult>;

    async fn get_hosted_zone(&self, zone_id: &str) -> Route53ApiResult<Option<Route53HostedZone>>;

    async fn list_hosted_zones(
        &self,
        marker: Option<&str>,
        max_items: u16,
    ) -> Route53ApiResult<Route53HostedZonePage>;

    async fn list_record_sets(
        &self,
        zone_id: &str,
        cursor: Option<&Route53RecordCursor>,
        max_items: u16,
    ) -> Route53ApiResult<Route53RecordPage>;

    /// The transport must disable automatic retries for this call and map
    /// every ambiguous post-dispatch failure to `UnknownOutcome`.
    async fn change_record_sets(
        &self,
        zone_id: &str,
        batch: &Route53ChangeBatch,
    ) -> Route53ApiResult<Route53ChangeInfo>;

    async fn get_change(&self, change_id: &str) -> Route53ApiResult<Option<Route53ChangeInfo>>;

    async fn delete_hosted_zone(&self, zone_id: &str) -> Route53ApiResult<Route53ChangeInfo>;

    async fn get_dnssec(&self, zone_id: &str) -> Route53ApiResult<Route53DnssecInfo>;

    async fn enable_hosted_zone_dnssec(&self, zone_id: &str)
        -> Route53ApiResult<Route53ChangeInfo>;

    async fn disable_hosted_zone_dnssec(
        &self,
        zone_id: &str,
    ) -> Route53ApiResult<Route53ChangeInfo>;
}
