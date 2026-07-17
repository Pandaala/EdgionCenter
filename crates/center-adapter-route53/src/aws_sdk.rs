use std::{fmt, net::IpAddr, str::FromStr, time::Duration};

use async_trait::async_trait;
use aws_config::SdkConfig;
use aws_sdk_route53::{
    config::{retry::RetryConfig, timeout::TimeoutConfig},
    error::ProvideErrorMetadata,
    types::{
        AliasTarget, Change, ChangeAction, ChangeBatch, GeoLocation, HostedZoneConfig,
        ResourceRecord, ResourceRecordSet, ResourceRecordSetFailover, ResourceRecordSetRegion,
        RrType,
    },
};
use edgion_center_core::{NormalizedProviderError, ProviderErrorCategory};

use crate::{
    Route53AliasTargetData, Route53Api, Route53ApiResult, Route53ChangeAction, Route53ChangeBatch,
    Route53ChangeInfo, Route53CreateHostedZoneRequest, Route53CreateHostedZoneResult,
    Route53DnssecInfo, Route53GeoLocationData, Route53HostedZone, Route53HostedZonePage,
    Route53KeySigningKey, Route53RecordCursor, Route53RecordPage, Route53RecordSet,
};

const READ_MAX_ATTEMPTS: u32 = 3;
const MUTATION_MAX_ATTEMPTS: u32 = 1;
const THROTTLE_RETRY_AFTER_MS: u64 = 1_000;

/// Validated, non-secret parameters for an STS AssumeRole credential provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwsAssumeRoleSpec {
    role_arn: String,
    session_name: String,
}

impl AwsAssumeRoleSpec {
    pub fn new(
        role_arn: impl Into<String>,
        session_name: impl Into<String>,
    ) -> Route53ApiResult<Self> {
        let value = Self {
            role_arn: role_arn.into(),
            session_name: session_name.into(),
        };
        validate_role_arn(&value.role_arn)?;
        validate_role_session_name(&value.session_name)?;
        Ok(value)
    }

    pub fn role_arn(&self) -> &str {
        &self.role_arn
    }

    pub fn session_name(&self) -> &str {
        &self.session_name
    }
}

/// Creates AWS SDK configurations without resolving Center secret references.
///
/// Standalone or Kubernetes composition resolves referenced base credentials and external IDs,
/// then passes them here. The returned standard AWS AssumeRole provider retains the external ID in
/// memory when one is required so it can refresh temporary credentials; the value is never exposed
/// by this crate's Debug output, logs, serialization, or normalized errors.
#[derive(Debug, Clone, Copy, Default)]
pub struct AwsRoute53SdkConfigFactory;

struct RedactedCredentialsProvider<P>(P);

impl<P> fmt::Debug for RedactedCredentialsProvider<P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RedactedCredentialsProvider")
    }
}

impl<P> aws_credential_types::provider::ProvideCredentials for RedactedCredentialsProvider<P>
where
    P: aws_credential_types::provider::ProvideCredentials + Send + Sync,
{
    fn provide_credentials<'a>(
        &'a self,
    ) -> aws_credential_types::provider::future::ProvideCredentials<'a>
    where
        Self: 'a,
    {
        self.0.provide_credentials()
    }
}

impl AwsRoute53SdkConfigFactory {
    /// Loads the AWS default chain, including environment, profile, ECS and EKS workload identity.
    pub async fn ambient() -> Route53ApiResult<SdkConfig> {
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .load()
            .await;
        if config.endpoint_url().is_some() {
            return Err(validation("ambient_aws_endpoint_override_forbidden"));
        }
        Ok(config)
    }

    /// Wraps an already-resolved base SDK configuration with a refreshable STS AssumeRole provider.
    pub async fn assume_role(
        base_config: &SdkConfig,
        spec: &AwsAssumeRoleSpec,
        external_id: Option<String>,
    ) -> Route53ApiResult<SdkConfig> {
        validate_endpoint(base_config.endpoint_url())?;
        if external_id
            .as_deref()
            .is_some_and(|value| !is_valid_external_id(value))
        {
            return Err(validation("invalid_aws_external_id"));
        }
        let mut builder = aws_config::sts::AssumeRoleProvider::builder(spec.role_arn.clone())
            .session_name(spec.session_name.clone())
            .configure(base_config);
        if let Some(external_id) = external_id {
            builder = builder.external_id(external_id);
        }
        let provider = builder.build().await;
        let mut config = base_config
            .to_builder()
            .identity_cache(aws_config::identity::IdentityCache::lazy().build())
            .credentials_provider(aws_sdk_route53::config::SharedCredentialsProvider::new(
                RedactedCredentialsProvider(provider),
            ));
        config.set_endpoint_url(None);
        Ok(config.build())
    }
}
const OPERATION_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(10);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(30);

/// Loopback service endpoints for hermetic tests.
///
/// Production composition must use [`AwsRoute53Api::new`] so signed requests and the STS
/// identity probe cannot be redirected to an untrusted HTTPS origin.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AwsRoute53ApiOptions {
    pub route53_endpoint_url: Option<String>,
    pub sts_endpoint_url: Option<String>,
}

/// Credential-owning AWS SDK implementation of the provider-neutral Route 53 seam.
pub struct AwsRoute53Api {
    account_id: String,
    read_client: aws_sdk_route53::Client,
    mutation_client: aws_sdk_route53::Client,
}

impl AwsRoute53Api {
    pub async fn new(sdk_config: &SdkConfig) -> Route53ApiResult<Self> {
        Self::with_options(sdk_config, AwsRoute53ApiOptions::default()).await
    }

    pub async fn with_options(
        sdk_config: &SdkConfig,
        options: AwsRoute53ApiOptions,
    ) -> Route53ApiResult<Self> {
        if sdk_config.endpoint_url().is_some() {
            return Err(validation("inherited_aws_endpoint_override_forbidden"));
        }
        validate_endpoint(options.route53_endpoint_url.as_deref())?;
        validate_endpoint(options.sts_endpoint_url.as_deref())?;
        let timeout = TimeoutConfig::builder()
            .operation_attempt_timeout(OPERATION_ATTEMPT_TIMEOUT)
            .operation_timeout(OPERATION_TIMEOUT)
            .build();
        let mut sts_config = aws_sdk_sts::config::Builder::from(sdk_config)
            .retry_config(
                aws_sdk_sts::config::retry::RetryConfig::standard()
                    .with_max_attempts(READ_MAX_ATTEMPTS),
            )
            .timeout_config(timeout.clone());
        if let Some(endpoint) = options.sts_endpoint_url {
            sts_config = sts_config.endpoint_url(endpoint);
        }
        let identity = aws_sdk_sts::Client::from_conf(sts_config.build())
            .get_caller_identity()
            .send()
            .await
            .map_err(|error| {
                map_read_error(error.as_service_error().and_then(|value| value.code()))
            })?;
        let account_id = identity
            .account()
            .filter(|value| is_aws_account_id(value))
            .ok_or_else(|| validation("invalid_sts_account_id"))?
            .to_string();

        let mut read_config = aws_sdk_route53::config::Builder::from(sdk_config)
            .retry_config(RetryConfig::standard().with_max_attempts(READ_MAX_ATTEMPTS))
            .timeout_config(timeout.clone());
        let mut mutation_config = aws_sdk_route53::config::Builder::from(sdk_config)
            .retry_config(RetryConfig::standard().with_max_attempts(MUTATION_MAX_ATTEMPTS))
            .timeout_config(timeout);
        if let Some(endpoint) = options.route53_endpoint_url {
            read_config = read_config.endpoint_url(endpoint.clone());
            mutation_config = mutation_config.endpoint_url(endpoint);
        }

        Ok(Self {
            account_id,
            read_client: aws_sdk_route53::Client::from_conf(read_config.build()),
            mutation_client: aws_sdk_route53::Client::from_conf(mutation_config.build()),
        })
    }
}

#[async_trait]
impl Route53Api for AwsRoute53Api {
    fn verified_account_id(&self) -> &str {
        &self.account_id
    }

    async fn create_hosted_zone(
        &self,
        request: &Route53CreateHostedZoneRequest,
    ) -> Route53ApiResult<Route53CreateHostedZoneResult> {
        let config = HostedZoneConfig::builder().private_zone(false).build();
        let output = self
            .mutation_client
            .create_hosted_zone()
            .name(&request.name)
            .caller_reference(&request.caller_reference)
            .hosted_zone_config(config)
            .send()
            .await
            .map_err(|error| map_mutation_error(&error))?;
        let mut hosted_zone = output
            .hosted_zone()
            .map(map_hosted_zone)
            .ok_or_else(|| unknown_outcome("missing_route53_hosted_zone"))?;
        hosted_zone.name_servers = output
            .delegation_set()
            .map(|value| {
                value
                    .name_servers()
                    .iter()
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default();
        Ok(Route53CreateHostedZoneResult {
            hosted_zone,
            change: output
                .change_info()
                .map(map_change_info)
                .ok_or_else(|| unknown_outcome("missing_route53_change_info"))?,
        })
    }

    async fn get_hosted_zone(&self, zone_id: &str) -> Route53ApiResult<Option<Route53HostedZone>> {
        match self.read_client.get_hosted_zone().id(zone_id).send().await {
            Ok(output) => {
                let mut zone = output
                    .hosted_zone()
                    .map(map_hosted_zone)
                    .ok_or_else(|| validation("missing_route53_hosted_zone"))?;
                zone.name_servers = output
                    .delegation_set()
                    .map(|value| {
                        value
                            .name_servers()
                            .iter()
                            .map(ToString::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                Ok(Some(zone))
            }
            Err(error) if service_code(&error) == Some("NoSuchHostedZone") => Ok(None),
            Err(error) => Err(map_read_error(service_code(&error))),
        }
    }

    async fn list_hosted_zones(
        &self,
        marker: Option<&str>,
        max_items: u16,
    ) -> Route53ApiResult<Route53HostedZonePage> {
        let output = self
            .read_client
            .list_hosted_zones()
            .set_marker(marker.map(str::to_string))
            .max_items(i32::from(max_items))
            .send()
            .await
            .map_err(|error| map_read_error(service_code(&error)))?;
        Ok(Route53HostedZonePage {
            items: output.hosted_zones().iter().map(map_hosted_zone).collect(),
            is_truncated: output.is_truncated(),
            next_marker: output.next_marker().map(str::to_string),
        })
    }

    async fn list_record_sets(
        &self,
        zone_id: &str,
        cursor: Option<&Route53RecordCursor>,
        max_items: u16,
    ) -> Route53ApiResult<Route53RecordPage> {
        let request = self
            .read_client
            .list_resource_record_sets()
            .hosted_zone_id(zone_id)
            .set_start_record_name(cursor.map(|value| value.name.clone()))
            .set_start_record_type(cursor.map(|value| RrType::from(value.record_type.as_str())))
            .set_start_record_identifier(cursor.and_then(|value| value.set_identifier.clone()))
            .max_items(i32::from(max_items));
        let output = request
            .send()
            .await
            .map_err(|error| map_read_error(service_code(&error)))?;
        let next = match (output.next_record_name(), output.next_record_type()) {
            (Some(name), Some(record_type)) => Some(Route53RecordCursor {
                name: name.to_string(),
                record_type: record_type.as_str().to_string(),
                set_identifier: output.next_record_identifier().map(str::to_string),
            }),
            (None, None) => None,
            _ => return Err(validation("incomplete_route53_record_cursor")),
        };
        Ok(Route53RecordPage {
            items: output
                .resource_record_sets()
                .iter()
                .map(map_record_set)
                .collect::<Route53ApiResult<_>>()?,
            is_truncated: output.is_truncated(),
            next,
        })
    }

    async fn change_record_sets(
        &self,
        zone_id: &str,
        batch: &Route53ChangeBatch,
    ) -> Route53ApiResult<Route53ChangeInfo> {
        let provider_batch = render_change_batch(batch)?;
        let output = self
            .mutation_client
            .change_resource_record_sets()
            .hosted_zone_id(zone_id)
            .change_batch(provider_batch)
            .send()
            .await
            .map_err(|error| map_mutation_error(&error))?;
        output
            .change_info()
            .map(map_change_info)
            .ok_or_else(|| unknown_outcome("missing_route53_change_info"))
    }

    async fn get_change(&self, change_id: &str) -> Route53ApiResult<Option<Route53ChangeInfo>> {
        match self.read_client.get_change().id(change_id).send().await {
            Ok(output) => Ok(output.change_info().map(map_change_info)),
            Err(error) if service_code(&error) == Some("NoSuchChange") => Ok(None),
            Err(error) => Err(map_read_error(service_code(&error))),
        }
    }

    async fn delete_hosted_zone(&self, zone_id: &str) -> Route53ApiResult<Route53ChangeInfo> {
        let output = self
            .mutation_client
            .delete_hosted_zone()
            .id(zone_id)
            .send()
            .await
            .map_err(|error| map_mutation_error(&error))?;
        output
            .change_info()
            .map(map_change_info)
            .ok_or_else(|| unknown_outcome("missing_route53_change_info"))
    }

    async fn get_dnssec(&self, zone_id: &str) -> Route53ApiResult<Route53DnssecInfo> {
        let output = self
            .read_client
            .get_dnssec()
            .hosted_zone_id(zone_id)
            .send()
            .await
            .map_err(|error| map_read_error(service_code(&error)))?;
        let status = output
            .status()
            .ok_or_else(|| validation("missing_route53_dnssec_status"))?;
        let serve_signature = status
            .serve_signature()
            .ok_or_else(|| validation("missing_route53_dnssec_serve_signature"))?;
        Ok(Route53DnssecInfo {
            serve_signature: serve_signature.to_string(),
            key_signing_keys: output
                .key_signing_keys()
                .iter()
                .map(|value| Route53KeySigningKey {
                    status: value.status().unwrap_or("UNKNOWN").to_string(),
                    ds_record: value.ds_record().map(str::to_string),
                })
                .collect(),
        })
    }

    async fn enable_hosted_zone_dnssec(
        &self,
        zone_id: &str,
    ) -> Route53ApiResult<Route53ChangeInfo> {
        let output = self
            .mutation_client
            .enable_hosted_zone_dnssec()
            .hosted_zone_id(zone_id)
            .send()
            .await
            .map_err(|error| map_mutation_error(&error))?;
        output
            .change_info()
            .map(map_change_info)
            .ok_or_else(|| unknown_outcome("missing_route53_change_info"))
    }

    async fn disable_hosted_zone_dnssec(
        &self,
        zone_id: &str,
    ) -> Route53ApiResult<Route53ChangeInfo> {
        let output = self
            .mutation_client
            .disable_hosted_zone_dnssec()
            .hosted_zone_id(zone_id)
            .send()
            .await
            .map_err(|error| map_mutation_error(&error))?;
        output
            .change_info()
            .map(map_change_info)
            .ok_or_else(|| unknown_outcome("missing_route53_change_info"))
    }
}

fn map_hosted_zone(zone: &aws_sdk_route53::types::HostedZone) -> Route53HostedZone {
    Route53HostedZone {
        id: zone.id().to_string(),
        name: zone.name().to_string(),
        private_zone: zone.config().is_some_and(|value| value.private_zone()),
        caller_reference: zone.caller_reference().to_string(),
        resource_record_set_count: zone
            .resource_record_set_count()
            .and_then(|value| u64::try_from(value).ok())
            .unwrap_or_default(),
        name_servers: Vec::new(),
        has_linked_service: zone.linked_service().is_some(),
        has_unsupported_features: zone.features().is_some(),
    }
}

fn map_record_set(record: &ResourceRecordSet) -> Route53ApiResult<Route53RecordSet> {
    Ok(Route53RecordSet {
        name: record.name().to_string(),
        record_type: record.r#type().as_str().to_string(),
        ttl: record
            .ttl()
            .map(u32::try_from)
            .transpose()
            .map_err(|_| validation("invalid_route53_record_ttl"))?,
        resource_records: record
            .resource_records()
            .iter()
            .map(|value| value.value().to_string())
            .collect(),
        alias_target: record.alias_target().map(|value| Route53AliasTargetData {
            hosted_zone_id: value.hosted_zone_id().to_string(),
            dns_name: value.dns_name().to_string(),
            evaluate_target_health: value.evaluate_target_health(),
        }),
        set_identifier: record.set_identifier().map(str::to_string),
        weight: record
            .weight()
            .map(u8::try_from)
            .transpose()
            .map_err(|_| validation("invalid_route53_record_weight"))?,
        failover: record.failover().map(|value| value.as_str().to_string()),
        region: record.region().map(|value| value.as_str().to_string()),
        geolocation: record.geo_location().map(|value| Route53GeoLocationData {
            continent_code: value.continent_code().map(str::to_string),
            country_code: value.country_code().map(str::to_string),
            subdivision_code: value.subdivision_code().map(str::to_string),
        }),
        multivalue_answer: record.multi_value_answer(),
        health_check_id: record.health_check_id().map(str::to_string),
        traffic_policy_instance_id: record.traffic_policy_instance_id().map(str::to_string),
        has_cidr_routing_config: record.cidr_routing_config().is_some(),
        has_geoproximity_location: record.geo_proximity_location().is_some(),
    })
}

fn render_change_batch(batch: &Route53ChangeBatch) -> Route53ApiResult<ChangeBatch> {
    let changes = batch
        .changes
        .iter()
        .map(|change| {
            Change::builder()
                .action(match change.action {
                    Route53ChangeAction::Create => ChangeAction::Create,
                    Route53ChangeAction::Delete => ChangeAction::Delete,
                })
                .resource_record_set(render_record_set(&change.record_set)?)
                .build()
                .map_err(|_| validation("invalid_route53_change"))
        })
        .collect::<Route53ApiResult<Vec<_>>>()?;
    ChangeBatch::builder()
        .comment(batch.comment.clone())
        .set_changes(Some(changes))
        .build()
        .map_err(|_| validation("invalid_route53_change_batch"))
}

fn render_record_set(record: &Route53RecordSet) -> Route53ApiResult<ResourceRecordSet> {
    let mut builder = ResourceRecordSet::builder()
        .name(record.name.clone())
        .r#type(RrType::from(record.record_type.as_str()))
        .set_set_identifier(record.set_identifier.clone())
        .set_weight(record.weight.map(i64::from))
        .set_failover(
            record
                .failover
                .as_deref()
                .map(ResourceRecordSetFailover::from),
        )
        .set_region(record.region.as_deref().map(ResourceRecordSetRegion::from))
        .set_multi_value_answer(record.multivalue_answer)
        .set_ttl(record.ttl.map(i64::from))
        .set_health_check_id(record.health_check_id.clone());
    if let Some(location) = record.geolocation.as_ref() {
        builder = builder.geo_location(
            GeoLocation::builder()
                .set_continent_code(location.continent_code.clone())
                .set_country_code(location.country_code.clone())
                .set_subdivision_code(location.subdivision_code.clone())
                .build(),
        );
    }
    if let Some(target) = record.alias_target.as_ref() {
        builder = builder.alias_target(render_alias_target(target)?);
    }
    let resource_records = record
        .resource_records
        .iter()
        .map(|value| {
            ResourceRecord::builder()
                .value(value)
                .build()
                .map_err(|_| validation("invalid_route53_record_value"))
        })
        .collect::<Route53ApiResult<Vec<_>>>()?;
    builder
        .set_resource_records((!resource_records.is_empty()).then_some(resource_records))
        .build()
        .map_err(|_| validation("invalid_route53_record_set"))
}

fn render_alias_target(target: &Route53AliasTargetData) -> Route53ApiResult<AliasTarget> {
    AliasTarget::builder()
        .hosted_zone_id(target.hosted_zone_id.clone())
        .dns_name(target.dns_name.clone())
        .evaluate_target_health(target.evaluate_target_health)
        .build()
        .map_err(|_| validation("invalid_route53_alias_target"))
}

fn map_change_info(info: &aws_sdk_route53::types::ChangeInfo) -> Route53ChangeInfo {
    Route53ChangeInfo {
        id: info.id().to_string(),
        status: info.status().as_str().to_string(),
        submitted_at_unix_seconds: info.submitted_at().secs(),
        comment: info.comment().map(str::to_string),
    }
}

fn is_aws_account_id(value: &str) -> bool {
    value.len() == 12 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn validate_role_arn(value: &str) -> Route53ApiResult<()> {
    let mut parts = value.splitn(6, ':');
    let valid = parts.next() == Some("arn")
        && parts.next().is_some_and(|partition| {
            !partition.is_empty()
                && partition
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
        && parts.next() == Some("iam")
        && parts.next() == Some("")
        && parts.next().is_some_and(is_aws_account_id)
        && parts.next().is_some_and(|resource| {
            resource
                .strip_prefix("role/")
                .is_some_and(|role| !role.is_empty() && role.len() <= 512 && is_role_path(role))
        });
    if valid {
        Ok(())
    } else {
        Err(validation("invalid_aws_role_arn"))
    }
}

fn is_role_path(value: &str) -> bool {
    value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(byte, b'/' | b'_' | b'+' | b'=' | b',' | b'.' | b'@' | b'-')
    })
}

fn validate_role_session_name(value: &str) -> Route53ApiResult<()> {
    if (2..=64).contains(&value.len())
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'_' | b'+' | b'=' | b',' | b'.' | b'@' | b'-')
        })
    {
        Ok(())
    } else {
        Err(validation("invalid_aws_role_session_name"))
    }
}

fn is_valid_external_id(value: &str) -> bool {
    (2..=1_224).contains(&value.len())
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'_' | b'+' | b'=' | b',' | b'.' | b'@' | b':' | b'/' | b'-'
                )
        })
}

fn validate_endpoint(endpoint: Option<&str>) -> Route53ApiResult<()> {
    let Some(endpoint) = endpoint else {
        return Ok(());
    };
    let parsed = url::Url::parse(endpoint).map_err(|_| validation("invalid_aws_endpoint_url"))?;
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(validation("invalid_aws_endpoint_url"));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| validation("invalid_aws_endpoint_url"))?;
    match parsed.scheme() {
        "http" | "https" if is_loopback_host(host) => Ok(()),
        _ => Err(validation("untrusted_aws_endpoint_url")),
    }
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || IpAddr::from_str(host.trim_start_matches('[').trim_end_matches(']'))
            .is_ok_and(|address| address.is_loopback())
}

fn service_code<E, R>(error: &aws_sdk_route53::error::SdkError<E, R>) -> Option<&str>
where
    E: ProvideErrorMetadata,
{
    error
        .as_service_error()
        .and_then(ProvideErrorMetadata::code)
}

fn map_read_error(code: Option<&str>) -> NormalizedProviderError {
    match code {
        Some(code) => map_service_error(code),
        None => provider_error(
            ProviderErrorCategory::Transient,
            "route53_transport_error",
            None,
        ),
    }
}

fn map_mutation_error<E>(error: &aws_sdk_route53::error::SdkError<E>) -> NormalizedProviderError
where
    E: ProvideErrorMetadata,
{
    if matches!(
        error,
        aws_sdk_route53::error::SdkError::ConstructionFailure(_)
    ) {
        return validation("invalid_route53_request");
    }
    let status = error
        .raw_response()
        .map(|response| response.status().as_u16());
    if status.is_some_and(|status| matches!(status, 408 | 500..=599)) {
        return unknown_outcome("route53_mutation_outcome_unknown");
    }
    match (status, service_code(error)) {
        (Some(400..=499), Some(code)) => map_service_error(code),
        (Some(400..=499), None) => validation("route53_service_rejection"),
        (_, Some(code)) if is_known_service_code(code) => map_service_error(code),
        _ => unknown_outcome("route53_mutation_outcome_unknown"),
    }
}

fn map_service_error(code: &str) -> NormalizedProviderError {
    let (category, retry_after_ms) = match code {
        "InvalidClientTokenId"
        | "SignatureDoesNotMatch"
        | "ExpiredToken"
        | "UnrecognizedClientException" => (ProviderErrorCategory::Authentication, None),
        "AccessDenied" | "AccessDeniedException" => (ProviderErrorCategory::Authorization, None),
        "Throttling" | "ThrottlingException" | "PriorRequestNotComplete" => (
            ProviderErrorCategory::Throttled,
            Some(THROTTLE_RETRY_AFTER_MS),
        ),
        "LimitsExceeded" | "TooManyHostedZones" | "TooManyKeySigningKeys" => {
            (ProviderErrorCategory::Quota, None)
        }
        "NoSuchHostedZone"
        | "NoSuchChange"
        | "NoSuchHealthCheck"
        | "NoSuchKeySigningKey"
        | "NoSuchDelegationSet" => (ProviderErrorCategory::NotFound, None),
        "ConflictingDomainExists"
        | "HostedZoneAlreadyExists"
        | "HostedZoneNotEmpty"
        | "HostedZonePartiallyDelegated"
        | "InvalidChangeBatch"
        | "InvalidKeySigningKeyStatus"
        | "InvalidSigningStatus"
        | "KeySigningKeyInParentDSRecord"
        | "KeySigningKeyWithActiveStatusNotFound"
        | "ConcurrentModification"
        | "DelegationSetNotReusable"
        | "DelegationSetNotAvailable" => (ProviderErrorCategory::Conflict, None),
        "InvalidInput" | "InvalidArgument" | "InvalidDomainName" | "InvalidKmsArn"
        | "InvalidVpcId" | "DNSSECNotFound" => (ProviderErrorCategory::Validation, None),
        "InternalFailure" | "ServiceUnavailable" => (ProviderErrorCategory::Transient, None),
        _ => (ProviderErrorCategory::Transient, None),
    };
    provider_error(category, "route53_service_error", retry_after_ms)
}

fn is_known_service_code(code: &str) -> bool {
    matches!(
        code,
        "InvalidClientTokenId"
            | "SignatureDoesNotMatch"
            | "ExpiredToken"
            | "UnrecognizedClientException"
            | "AccessDenied"
            | "AccessDeniedException"
            | "Throttling"
            | "ThrottlingException"
            | "PriorRequestNotComplete"
            | "LimitsExceeded"
            | "TooManyHostedZones"
            | "TooManyKeySigningKeys"
            | "NoSuchHostedZone"
            | "NoSuchChange"
            | "NoSuchHealthCheck"
            | "NoSuchKeySigningKey"
            | "NoSuchDelegationSet"
            | "ConflictingDomainExists"
            | "HostedZoneAlreadyExists"
            | "HostedZoneNotEmpty"
            | "HostedZonePartiallyDelegated"
            | "InvalidChangeBatch"
            | "InvalidKeySigningKeyStatus"
            | "InvalidSigningStatus"
            | "KeySigningKeyInParentDSRecord"
            | "KeySigningKeyWithActiveStatusNotFound"
            | "ConcurrentModification"
            | "DelegationSetNotReusable"
            | "DelegationSetNotAvailable"
            | "InvalidInput"
            | "InvalidArgument"
            | "InvalidDomainName"
            | "InvalidKmsArn"
            | "InvalidVpcId"
            | "DNSSECNotFound"
            | "InternalFailure"
            | "ServiceUnavailable"
    )
}

fn validation(code: &str) -> NormalizedProviderError {
    provider_error(ProviderErrorCategory::Validation, code, None)
}

fn unknown_outcome(code: &str) -> NormalizedProviderError {
    provider_error(ProviderErrorCategory::UnknownOutcome, code, None)
}

fn provider_error(
    category: ProviderErrorCategory,
    code: &str,
    retry_after_ms: Option<u64>,
) -> NormalizedProviderError {
    NormalizedProviderError::new(
        category,
        code,
        "AWS provider request failed",
        retry_after_ms,
        None,
    )
    .expect("static normalized provider error")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_override_allows_https_and_loopback_http() {
        for endpoint in [
            "http://localhost:8080",
            "http://127.0.0.1:8080/base",
            "https://127.0.0.1:8443",
            "http://[::1]:8080",
        ] {
            assert!(validate_endpoint(Some(endpoint)).is_ok(), "{endpoint}");
        }
    }

    #[test]
    fn endpoint_override_rejects_unsafe_urls() {
        for endpoint in [
            "http://route53.example.test",
            "http://localhost.example.test",
            "https://route53.example.test",
            "https://user@route53.example.test",
            "https://route53.example.test?token=secret",
            "https://route53.example.test#fragment",
            "ftp://route53.example.test",
            "not-a-url",
        ] {
            assert!(validate_endpoint(Some(endpoint)).is_err(), "{endpoint}");
        }
    }

    #[test]
    fn record_mapping_preserves_multivalue_presence_for_exact_delete() {
        let absent = ResourceRecordSet::builder()
            .name("simple.example.test.")
            .r#type(RrType::A)
            .ttl(60)
            .resource_records(
                ResourceRecord::builder()
                    .value("192.0.2.1")
                    .build()
                    .unwrap(),
            )
            .build()
            .unwrap();
        let mapped = map_record_set(&absent).unwrap();
        assert_eq!(mapped.multivalue_answer, None);
        assert_eq!(
            render_record_set(&mapped).unwrap().multi_value_answer(),
            None
        );

        let explicit = ResourceRecordSet::builder()
            .name("multi.example.test.")
            .r#type(RrType::A)
            .ttl(60)
            .resource_records(
                ResourceRecord::builder()
                    .value("192.0.2.2")
                    .build()
                    .unwrap(),
            )
            .multi_value_answer(true)
            .build()
            .unwrap();
        let mapped = map_record_set(&explicit).unwrap();
        assert_eq!(mapped.multivalue_answer, Some(true));
        assert_eq!(
            render_record_set(&mapped).unwrap().multi_value_answer(),
            Some(true)
        );
    }

    #[test]
    fn assume_role_parameters_fail_closed() {
        assert!(AwsAssumeRoleSpec::new(
            "arn:aws:iam::123456789012:role/dns/edgion-center",
            "edgion-route53"
        )
        .is_ok());
        for role in [
            "",
            "arn:aws:s3::123456789012:role/example",
            "arn:aws:iam::123:role/example",
            "arn:aws:iam::123456789012:user/example",
            "arn:aws:iam::123456789012:role/invalid role",
        ] {
            assert!(AwsAssumeRoleSpec::new(role, "edgion-route53").is_err());
        }
        for session in [
            "",
            "x",
            "invalid session",
            "this-session-name-is-over-sixty-four-characters-long-and-must-fail-closed",
        ] {
            assert!(
                AwsAssumeRoleSpec::new("arn:aws:iam::123456789012:role/example", session).is_err()
            );
        }
        assert!(is_valid_external_id("tenant/example:123"));
        assert!(!is_valid_external_id("x"));
        assert!(!is_valid_external_id("contains a space"));
    }

    #[test]
    fn lifecycle_service_errors_keep_deterministic_categories() {
        for code in [
            "InvalidArgument",
            "InvalidDomainName",
            "InvalidKmsArn",
            "InvalidVpcId",
            "DNSSECNotFound",
        ] {
            assert_eq!(
                map_service_error(code).category(),
                ProviderErrorCategory::Validation,
                "{code}"
            );
        }
        for code in [
            "HostedZoneAlreadyExists",
            "DelegationSetNotReusable",
            "DelegationSetNotAvailable",
            "KeySigningKeyInParentDSRecord",
            "ConcurrentModification",
        ] {
            assert_eq!(
                map_service_error(code).category(),
                ProviderErrorCategory::Conflict,
                "{code}"
            );
        }
        assert_eq!(
            map_service_error("NoSuchDelegationSet").category(),
            ProviderErrorCategory::NotFound
        );
        assert_eq!(
            map_service_error("TooManyKeySigningKeys").category(),
            ProviderErrorCategory::Quota
        );
    }
}
