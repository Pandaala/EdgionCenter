//! Sanitized Route 53 provider-observation to Admin DTO mapping.

use edgion_center_app::api::route53_dns::{
    Route53DnsAdminError, Route53RecordControlDto, Route53RecordSetDto, Route53ZoneDto,
};
use edgion_center_core::{
    CloudProvider, DnsRecordExtension, ObservedDnsRecordSet, ObservedDnsZone, ZoneVisibility,
};

pub(crate) fn map_zone(observed: ObservedDnsZone) -> Result<Route53ZoneDto, Route53DnsAdminError> {
    observed
        .validate()
        .map_err(|_| Route53DnsAdminError::InvalidProviderObservation)?;
    if observed.zone.provider != CloudProvider::Aws
        || observed.zone.visibility != ZoneVisibility::Public
    {
        return Err(Route53DnsAdminError::InvalidProviderObservation);
    }
    Ok(Route53ZoneDto {
        provider_account_id: observed.zone.provider_account_id,
        zone_id: observed.zone.zone_id,
        apex: observed.zone.apex,
        visibility: observed.zone.visibility,
    })
}

pub(crate) fn map_record(
    observed: ObservedDnsRecordSet,
) -> Result<Route53RecordSetDto, Route53DnsAdminError> {
    observed
        .validate()
        .map_err(|_| Route53DnsAdminError::InvalidProviderObservation)?;
    if observed.zone.provider != CloudProvider::Aws
        || observed.zone.visibility != ZoneVisibility::Public
        || !observed.provider_object_ids.is_empty()
        || matches!(
            observed.record_set.extension.as_ref(),
            Some(DnsRecordExtension::Cloudflare { .. })
        )
    {
        return Err(Route53DnsAdminError::InvalidProviderObservation);
    }
    Ok(Route53RecordSetDto {
        provider_account_id: observed.zone.provider_account_id,
        zone_id: observed.zone.zone_id,
        zone_apex: observed.zone.apex,
        zone_visibility: observed.zone.visibility,
        record_set: observed.record_set,
        control: Route53RecordControlDto::ExternalOrManual,
        revision: observed.revision,
    })
}
