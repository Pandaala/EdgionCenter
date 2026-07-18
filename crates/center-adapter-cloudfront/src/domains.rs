//! Observation-bound CloudFront custom-domain and Route 53 alias planning.

use std::collections::BTreeSet;

use async_trait::async_trait;
use edgion_center_core::{
    AbsoluteDnsName, CertificateName, CloudProvider, CloudResourceId, DnsOwnerName,
    DnsRecordExtension, DnsRecordSetKey, DnsRoutingIdentity, DnsTtl, DnsZoneId, DnsZoneRef,
    DomainName, ProviderDnsRecordSet, ProviderDnsRecordType, Route53AliasTarget, ZoneVisibility,
};
use serde::Serialize;

use crate::{
    model::validation, AcmCertificateKeyAlgorithm, AcmCertificateObservation, AcmCertificateStatus,
    AcmCertificateType, AwsPartition, CloudFrontApiResult, CloudFrontDetailObservation,
    CloudFrontDistributionObservationBinding, CloudFrontInventoryAdapter,
    CloudFrontPlanningInventory,
};

const MAX_ALIASES: usize = 100;
const MAX_EVIDENCE_FRESHNESS_MS: i64 = 5 * 60 * 1_000;
const MIN_CERTIFICATE_VALIDITY_SECONDS: i64 = 24 * 60 * 60;
const MAX_CATALOG_REVISION_LEN: usize = 512;
const DOMAIN_CONFLICT_PAGE_SIZE: u16 = 100;
const MAX_DOMAIN_CONFLICT_PAGES: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudFrontAliasCatalogTargetKind {
    StandardDistribution,
}

/// Raw output from a composition-owned, versioned AWS endpoint catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudFrontAliasCatalogRecord {
    pub source_id: CloudResourceId,
    pub revision: String,
    pub partition: AwsPartition,
    pub target_kind: CloudFrontAliasCatalogTargetKind,
    pub dns_suffix: String,
    pub hosted_zone_id: String,
}

#[async_trait]
pub trait CloudFrontAliasCatalogSource: Send + Sync {
    async fn standard_distribution_alias(
        &self,
        partition: AwsPartition,
    ) -> CloudFrontApiResult<Option<CloudFrontAliasCatalogRecord>>;
}

/// Sealed catalog evidence; request payloads cannot mint a CloudFront target zone ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontAliasCatalogEvidence {
    source_id: CloudResourceId,
    revision: String,
    partition: AwsPartition,
    dns_suffix: String,
    hosted_zone_id: DnsZoneId,
    observed_at_unix_ms: i64,
    valid_until_unix_ms: i64,
}

impl CloudFrontAliasCatalogEvidence {
    fn validate_at(&self, now_unix_ms: i64) -> CloudFrontApiResult<()> {
        self.source_id
            .validate()
            .map_err(|_| validation("invalid_cloudfront_alias_catalog_source"))?;
        if self.partition != AwsPartition::Aws
            || self.dns_suffix != "cloudfront.net"
            || self.revision.is_empty()
            || self.revision.len() > MAX_CATALOG_REVISION_LEN
            || self.revision.trim() != self.revision
            || self.revision.chars().any(char::is_control)
            || self.observed_at_unix_ms <= 0
            || self.valid_until_unix_ms <= self.observed_at_unix_ms
            || self
                .valid_until_unix_ms
                .saturating_sub(self.observed_at_unix_ms)
                > MAX_EVIDENCE_FRESHNESS_MS
            || now_unix_ms < self.observed_at_unix_ms
            || now_unix_ms >= self.valid_until_unix_ms
        {
            return Err(validation("invalid_cloudfront_alias_catalog_evidence"));
        }
        Ok(())
    }
}

pub async fn observe_cloudfront_alias_catalog(
    source: &dyn CloudFrontAliasCatalogSource,
    partition: AwsPartition,
    now_unix_ms: i64,
) -> CloudFrontApiResult<CloudFrontAliasCatalogEvidence> {
    if partition != AwsPartition::Aws {
        return Err(validation("unsupported_cloudfront_custom_domain_partition"));
    }
    if now_unix_ms <= 0 {
        return Err(validation("invalid_cloudfront_alias_catalog_evidence"));
    }
    let record = tokio::time::timeout(
        crate::INVENTORY_TIMEOUT,
        source.standard_distribution_alias(partition),
    )
    .await
    .map_err(|_| validation("cloudfront_alias_catalog_deadline_exceeded"))??
    .ok_or_else(|| validation("cloudfront_alias_catalog_missing"))?;
    if record.partition != partition
        || record.target_kind != CloudFrontAliasCatalogTargetKind::StandardDistribution
    {
        return Err(validation("cloudfront_alias_catalog_scope_mismatch"));
    }
    let evidence = CloudFrontAliasCatalogEvidence {
        source_id: record.source_id,
        revision: record.revision,
        partition: record.partition,
        dns_suffix: record.dns_suffix,
        hosted_zone_id: DnsZoneId::new(record.hosted_zone_id)
            .map_err(|_| validation("invalid_cloudfront_alias_catalog_zone_id"))?,
        observed_at_unix_ms: now_unix_ms,
        valid_until_unix_ms: now_unix_ms.saturating_add(MAX_EVIDENCE_FRESHNESS_MS),
    };
    evidence.validate_at(now_unix_ms)?;
    Ok(evidence)
}

/// Sealed compatibility proof for an externally managed ACM certificate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontExternalCertificateEvidence {
    binding: CloudFrontDistributionObservationBinding,
    certificate_arn: String,
    required_hostnames: BTreeSet<DomainName>,
    status: AcmCertificateStatus,
    certificate_type: AcmCertificateType,
    key_algorithm: AcmCertificateKeyAlgorithm,
    subject_alternative_names: BTreeSet<String>,
    not_before_unix_seconds: i64,
    not_after_unix_seconds: i64,
    observed_at_unix_ms: i64,
    valid_until_unix_ms: i64,
}

impl CloudFrontExternalCertificateEvidence {
    fn validate_at(&self, now_unix_ms: i64) -> CloudFrontApiResult<()> {
        self.binding.validate_at(now_unix_ms)?;
        let now_seconds = now_unix_ms.div_euclid(1_000);
        if self.binding.partition() != AwsPartition::Aws
            || self.required_hostnames.is_empty()
            || self.required_hostnames.len() > MAX_ALIASES
            || self
                .required_hostnames
                .iter()
                .any(|hostname| !hostname.as_str().is_ascii())
            || self.status != AcmCertificateStatus::Issued
            || self.certificate_type != AcmCertificateType::AmazonIssued
            || !matches!(
                self.key_algorithm,
                AcmCertificateKeyAlgorithm::Rsa1024
                    | AcmCertificateKeyAlgorithm::Rsa2048
                    | AcmCertificateKeyAlgorithm::Rsa3072
                    | AcmCertificateKeyAlgorithm::Rsa4096
                    | AcmCertificateKeyAlgorithm::EcPrime256v1
            )
            || self.not_before_unix_seconds > now_seconds
            || self.not_after_unix_seconds
                <= now_seconds.saturating_add(MIN_CERTIFICATE_VALIDITY_SECONDS)
            || self.observed_at_unix_ms <= 0
            || self.valid_until_unix_ms <= self.observed_at_unix_ms
            || self
                .valid_until_unix_ms
                .saturating_sub(self.observed_at_unix_ms)
                > MAX_EVIDENCE_FRESHNESS_MS
            || now_unix_ms < self.observed_at_unix_ms
            || now_unix_ms >= self.valid_until_unix_ms
            || !certificate_covers_all(&self.subject_alternative_names, &self.required_hostnames)?
        {
            return Err(validation("invalid_cloudfront_certificate_evidence"));
        }
        Ok(())
    }
}

impl CloudFrontInventoryAdapter {
    #[allow(clippy::too_many_arguments)]
    pub async fn planning_external_certificate_evidence(
        &self,
        inventory: &CloudFrontPlanningInventory,
        distribution_id: &str,
        certificate_arn: &str,
        required_hostnames: BTreeSet<DomainName>,
        observed_at_unix_ms: i64,
        valid_until_unix_ms: i64,
    ) -> CloudFrontApiResult<CloudFrontExternalCertificateEvidence> {
        let binding = CloudFrontDistributionObservationBinding::from_inventory(
            inventory,
            distribution_id,
            observed_at_unix_ms,
        )?;
        if binding.partition() != AwsPartition::Aws
            || valid_until_unix_ms <= observed_at_unix_ms
            || valid_until_unix_ms.saturating_sub(observed_at_unix_ms) > MAX_EVIDENCE_FRESHNESS_MS
        {
            return Err(validation("invalid_cloudfront_certificate_evidence_window"));
        }
        let observation = tokio::time::timeout(
            crate::INVENTORY_TIMEOUT,
            self.api.describe_acm_certificate(certificate_arn),
        )
        .await
        .map_err(|_| validation("cloudfront_acm_observation_deadline_exceeded"))??
        .ok_or_else(|| validation("cloudfront_acm_certificate_missing"))?;
        validate_acm_observation(&binding, certificate_arn, &observation)?;
        let evidence = CloudFrontExternalCertificateEvidence {
            binding,
            certificate_arn: observation.arn,
            required_hostnames,
            status: observation.status,
            certificate_type: observation.certificate_type,
            key_algorithm: observation.key_algorithm,
            subject_alternative_names: observation.subject_alternative_names,
            not_before_unix_seconds: observation
                .not_before_unix_seconds
                .ok_or_else(|| validation("cloudfront_acm_validity_missing"))?,
            not_after_unix_seconds: observation
                .not_after_unix_seconds
                .ok_or_else(|| validation("cloudfront_acm_validity_missing"))?,
            observed_at_unix_ms,
            valid_until_unix_ms,
        };
        evidence.validate_at(observed_at_unix_ms)?;
        Ok(evidence)
    }
}

fn validate_acm_observation(
    binding: &CloudFrontDistributionObservationBinding,
    certificate_arn: &str,
    observation: &AcmCertificateObservation,
) -> CloudFrontApiResult<()> {
    if observation.arn != certificate_arn
        || observation.account_id != binding.aws_account_id()
        || observation.partition != binding.partition()
        || observation.region != "us-east-1"
        || observation.managed_by.is_some()
        || observation.subject_alternative_names.is_empty()
        || observation
            .in_use_by
            .iter()
            .any(|resource| resource != binding.distribution_arn())
    {
        return Err(validation("cloudfront_acm_certificate_scope_mismatch"));
    }
    Ok(())
}

fn certificate_covers_all(
    names: &BTreeSet<String>,
    required: &BTreeSet<DomainName>,
) -> CloudFrontApiResult<bool> {
    let names = names
        .iter()
        .map(|name| CertificateName::new(name.clone()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| validation("invalid_cloudfront_acm_san"))?;
    Ok(required.iter().all(|hostname| {
        names.iter().any(|name| {
            if !name.wildcard {
                return name.domain == *hostname;
            }
            hostname
                .as_str()
                .strip_suffix(&format!(".{}", name.domain.as_str()))
                .is_some_and(|prefix| !prefix.is_empty() && !prefix.contains('.'))
        })
    }))
}

/// Sealed proof that every newly requested exact alias had a complete empty conflict scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontDomainConflictEvidence {
    binding: CloudFrontDistributionObservationBinding,
    certificate_arn: String,
    checked_hostnames: BTreeSet<DomainName>,
    observed_at_unix_ms: i64,
    valid_until_unix_ms: i64,
}

impl CloudFrontDomainConflictEvidence {
    fn validate_at(&self, now_unix_ms: i64) -> CloudFrontApiResult<()> {
        self.binding.validate_at(now_unix_ms)?;
        if self.binding.partition() != AwsPartition::Aws
            || self.certificate_arn.is_empty()
            || self.checked_hostnames.len() > MAX_ALIASES
            || self.observed_at_unix_ms <= 0
            || self.valid_until_unix_ms <= self.observed_at_unix_ms
            || self
                .valid_until_unix_ms
                .saturating_sub(self.observed_at_unix_ms)
                > MAX_EVIDENCE_FRESHNESS_MS
            || now_unix_ms < self.observed_at_unix_ms
            || now_unix_ms >= self.valid_until_unix_ms
        {
            return Err(validation("invalid_cloudfront_domain_conflict_evidence"));
        }
        Ok(())
    }
}

impl CloudFrontInventoryAdapter {
    #[allow(clippy::too_many_arguments)]
    pub async fn planning_domain_conflict_evidence(
        &self,
        inventory: &CloudFrontPlanningInventory,
        certificate: &CloudFrontExternalCertificateEvidence,
        distribution_id: &str,
        aliases_to_attach: &BTreeSet<DomainName>,
        observed_at_unix_ms: i64,
        valid_until_unix_ms: i64,
    ) -> CloudFrontApiResult<CloudFrontDomainConflictEvidence> {
        let binding = CloudFrontDistributionObservationBinding::from_inventory(
            inventory,
            distribution_id,
            observed_at_unix_ms,
        )?;
        certificate.validate_at(observed_at_unix_ms)?;
        if binding != certificate.binding
            || valid_until_unix_ms <= observed_at_unix_ms
            || valid_until_unix_ms.saturating_sub(observed_at_unix_ms) > MAX_EVIDENCE_FRESHNESS_MS
        {
            return Err(validation(
                "cloudfront_domain_conflict_evidence_scope_mismatch",
            ));
        }
        let entry = inventory
            .inventory()
            .entries
            .iter()
            .find(|entry| entry.summary.id == distribution_id)
            .ok_or_else(|| validation("cloudfront_distribution_observation_missing"))?;
        let CloudFrontDetailObservation::Complete(observed) = &entry.detail else {
            return Err(validation("cloudfront_distribution_observation_incomplete"));
        };
        let current_aliases = observed
            .detail
            .config
            .aliases
            .iter()
            .map(|alias| {
                DomainName::new(alias.clone()).map_err(|_| validation("invalid_cloudfront_alias"))
            })
            .collect::<CloudFrontApiResult<BTreeSet<_>>>()?;
        if aliases_to_attach
            .iter()
            .any(|hostname| !hostname.as_str().is_ascii())
        {
            return Err(validation("cloudfront_ascii_alias_required"));
        }
        let checked_hostnames = aliases_to_attach
            .difference(&current_aliases)
            .cloned()
            .collect::<BTreeSet<_>>();
        let effective_aliases = current_aliases
            .union(aliases_to_attach)
            .cloned()
            .collect::<BTreeSet<_>>();
        if effective_aliases != certificate.required_hostnames {
            return Err(validation("cloudfront_certificate_hostname_set_mismatch"));
        }
        if !checked_hostnames.is_empty()
            && (entry.summary.status != "Deployed"
                || observed
                    .detail
                    .config
                    .viewer_certificate
                    .cloudfront_default_certificate
                || observed
                    .detail
                    .config
                    .viewer_certificate
                    .certificate_arn
                    .as_deref()
                    != Some(certificate.certificate_arn.as_str()))
        {
            return Err(validation(
                "cloudfront_conflict_validation_certificate_not_deployed",
            ));
        }
        tokio::time::timeout(
            crate::INVENTORY_TIMEOUT,
            self.observe_domain_conflicts_within_deadline(distribution_id, &checked_hostnames),
        )
        .await
        .map_err(|_| validation("cloudfront_domain_conflict_observation_deadline_exceeded"))??;
        let evidence = CloudFrontDomainConflictEvidence {
            binding,
            certificate_arn: certificate.certificate_arn.clone(),
            checked_hostnames,
            observed_at_unix_ms,
            valid_until_unix_ms,
        };
        evidence.validate_at(observed_at_unix_ms)?;
        Ok(evidence)
    }

    async fn observe_domain_conflicts_within_deadline(
        &self,
        distribution_id: &str,
        hostnames: &BTreeSet<DomainName>,
    ) -> CloudFrontApiResult<()> {
        let mut total_pages = 0_usize;
        for hostname in hostnames {
            let mut marker = None::<String>;
            let mut seen_markers = BTreeSet::new();
            loop {
                total_pages = total_pages.saturating_add(1);
                if total_pages > MAX_DOMAIN_CONFLICT_PAGES {
                    return Err(validation("cloudfront_domain_conflict_page_limit"));
                }
                let page = self
                    .api
                    .list_domain_conflicts(
                        hostname.as_str(),
                        distribution_id,
                        marker.as_deref(),
                        DOMAIN_CONFLICT_PAGE_SIZE,
                    )
                    .await?;
                if page.queried_domain != hostname.as_str()
                    || page.validation_distribution_id != distribution_id
                {
                    return Err(validation("cloudfront_domain_conflict_page_scope_mismatch"));
                }
                if !page.items.is_empty() {
                    return Err(validation("cloudfront_domain_conflict_detected"));
                }
                let Some(next) = page.next_marker else {
                    break;
                };
                if next.is_empty() || !seen_markers.insert(next.clone()) {
                    return Err(validation("cloudfront_domain_conflict_marker_cycle"));
                }
                marker = Some(next);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct CloudFrontAliasDnsBindingIntent {
    pub hostname: DomainName,
    pub zone: DnsZoneRef,
}

#[derive(Debug, Clone)]
pub struct CloudFrontCustomDomainPlanRequest {
    pub distribution_id: String,
    pub aliases_to_attach: BTreeSet<DomainName>,
    pub certificate_arn: String,
    pub dns_bindings: Vec<CloudFrontAliasDnsBindingIntent>,
    pub now_unix_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudFrontCustomDomainStage {
    AttachViewerCertificate,
    WaitForCertificateDeployment,
    ObserveDomainConflicts,
    AttachAliases,
    WaitForAliasDeployment,
    ApplyRoute53Aliases,
    WaitForRoute53InSync,
    VerifyAuthoritativeAndRecursiveDns,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontCustomDomainPlan {
    binding: CloudFrontDistributionObservationBinding,
    certificate_arn: String,
    aliases_to_attach: BTreeSet<DomainName>,
    effective_aliases: BTreeSet<DomainName>,
    desired_route53_aliases: Vec<CloudFrontRoute53AliasDesiredState>,
    stages: Vec<CloudFrontCustomDomainStage>,
    distribution_binding_ready: bool,
    domain_conflict_evidence_available: bool,
    dispatch_blockers: BTreeSet<String>,
    dispatch_authorized: bool,
}

/// Zone-bound Route 53 desired state; account and hosted-zone scope are never discarded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontRoute53AliasDesiredState {
    zone: DnsZoneRef,
    hostname: DomainName,
    record_sets: Vec<ProviderDnsRecordSet>,
}

pub fn build_custom_domain_plan(
    request: CloudFrontCustomDomainPlanRequest,
    inventory: &CloudFrontPlanningInventory,
    certificate: &CloudFrontExternalCertificateEvidence,
    domain_conflicts: Option<&CloudFrontDomainConflictEvidence>,
    catalog: &CloudFrontAliasCatalogEvidence,
) -> CloudFrontApiResult<CloudFrontCustomDomainPlan> {
    let binding = CloudFrontDistributionObservationBinding::from_inventory(
        inventory,
        &request.distribution_id,
        request.now_unix_ms,
    )?;
    certificate.validate_at(request.now_unix_ms)?;
    catalog.validate_at(request.now_unix_ms)?;
    if binding != certificate.binding
        || catalog.partition != binding.partition()
        || request.certificate_arn != certificate.certificate_arn
        || request.aliases_to_attach.is_empty()
    {
        return Err(validation("cloudfront_custom_domain_evidence_mismatch"));
    }
    if request
        .aliases_to_attach
        .iter()
        .any(|hostname| !hostname.as_str().is_ascii())
    {
        return Err(validation("cloudfront_ascii_alias_required"));
    }
    let entry = inventory
        .inventory()
        .entries
        .iter()
        .find(|entry| entry.summary.id == request.distribution_id)
        .ok_or_else(|| validation("cloudfront_distribution_observation_missing"))?;
    let CloudFrontDetailObservation::Complete(observed) = &entry.detail else {
        return Err(validation("cloudfront_distribution_observation_incomplete"));
    };
    let current_aliases = observed
        .detail
        .config
        .aliases
        .iter()
        .map(|alias| {
            DomainName::new(alias.clone()).map_err(|_| validation("invalid_cloudfront_alias"))
        })
        .collect::<CloudFrontApiResult<BTreeSet<_>>>()?;
    let effective_aliases = current_aliases
        .union(&request.aliases_to_attach)
        .cloned()
        .collect::<BTreeSet<_>>();
    if effective_aliases.len() > MAX_ALIASES || effective_aliases != certificate.required_hostnames
    {
        return Err(validation("cloudfront_certificate_hostname_set_mismatch"));
    }
    let new_aliases = request
        .aliases_to_attach
        .difference(&current_aliases)
        .cloned()
        .collect::<BTreeSet<_>>();
    let domain_conflict_evidence_available = if let Some(evidence) = domain_conflicts {
        evidence.validate_at(request.now_unix_ms)?;
        if evidence.binding != binding
            || evidence.certificate_arn != request.certificate_arn
            || evidence.checked_hostnames != new_aliases
        {
            return Err(validation("cloudfront_domain_conflict_evidence_mismatch"));
        }
        true
    } else {
        new_aliases.is_empty()
    };
    let dns_binding_hostnames = request
        .dns_bindings
        .iter()
        .map(|binding| binding.hostname.clone())
        .collect::<BTreeSet<_>>();
    if dns_binding_hostnames != request.aliases_to_attach
        || dns_binding_hostnames.len() != request.dns_bindings.len()
    {
        return Err(validation("cloudfront_alias_dns_binding_set_mismatch"));
    }
    let desired_route53_aliases = build_route53_alias_desired_state(
        &request.dns_bindings,
        &effective_aliases,
        &entry.summary.domain_name,
        observed.detail.config.ipv6_enabled,
        catalog,
    )?;
    let distribution_binding_ready = request
        .aliases_to_attach
        .iter()
        .all(|alias| current_aliases.contains(alias))
        && observed
            .detail
            .config
            .viewer_certificate
            .certificate_arn
            .as_deref()
            == Some(request.certificate_arn.as_str())
        && entry.summary.status == "Deployed";
    let mut dispatch_blockers = BTreeSet::from([
        "cloudfront_distribution_ownership_proof_missing".to_string(),
        "cloudfront_distribution_mutation_executor_missing".to_string(),
        "cloudfront_domain_conflict_evidence_unavailable".to_string(),
        "route53_record_ownership_evidence_missing".to_string(),
        "route53_exact_revision_guard_missing".to_string(),
        "cloudfront_custom_domain_approval_missing".to_string(),
    ]);
    if domain_conflict_evidence_available {
        dispatch_blockers.remove("cloudfront_domain_conflict_evidence_unavailable");
    }
    let certificate_binding_ready = !observed
        .detail
        .config
        .viewer_certificate
        .cloudfront_default_certificate
        && observed
            .detail
            .config
            .viewer_certificate
            .certificate_arn
            .as_deref()
            == Some(request.certificate_arn.as_str())
        && entry.summary.status == "Deployed";
    if !new_aliases.is_empty() && !certificate_binding_ready {
        dispatch_blockers.insert("cloudfront_certificate_only_deployment_required".to_string());
    }
    if !distribution_binding_ready {
        dispatch_blockers.insert("cloudfront_alias_deployment_required".to_string());
    }
    Ok(CloudFrontCustomDomainPlan {
        binding,
        certificate_arn: request.certificate_arn,
        aliases_to_attach: request.aliases_to_attach,
        effective_aliases,
        desired_route53_aliases,
        stages: vec![
            CloudFrontCustomDomainStage::AttachViewerCertificate,
            CloudFrontCustomDomainStage::WaitForCertificateDeployment,
            CloudFrontCustomDomainStage::ObserveDomainConflicts,
            CloudFrontCustomDomainStage::AttachAliases,
            CloudFrontCustomDomainStage::WaitForAliasDeployment,
            CloudFrontCustomDomainStage::ApplyRoute53Aliases,
            CloudFrontCustomDomainStage::WaitForRoute53InSync,
            CloudFrontCustomDomainStage::VerifyAuthoritativeAndRecursiveDns,
        ],
        distribution_binding_ready,
        domain_conflict_evidence_available,
        dispatch_blockers,
        dispatch_authorized: false,
    })
}

fn build_route53_alias_desired_state(
    bindings: &[CloudFrontAliasDnsBindingIntent],
    effective_aliases: &BTreeSet<DomainName>,
    distribution_domain: &str,
    ipv6_enabled: bool,
    catalog: &CloudFrontAliasCatalogEvidence,
) -> CloudFrontApiResult<Vec<CloudFrontRoute53AliasDesiredState>> {
    let target = AbsoluteDnsName::new(distribution_domain.to_string())
        .map_err(|_| validation("invalid_cloudfront_distribution_domain"))?;
    if !target
        .as_str()
        .ends_with(&format!(".{}", catalog.dns_suffix))
    {
        return Err(validation("cloudfront_alias_catalog_target_mismatch"));
    }
    let mut seen = BTreeSet::new();
    let mut desired = Vec::new();
    for binding in bindings {
        if !seen.insert(binding.hostname.clone()) || !effective_aliases.contains(&binding.hostname)
        {
            return Err(validation("invalid_cloudfront_alias_dns_binding"));
        }
        binding
            .zone
            .validate()
            .map_err(|_| validation("invalid_route53_alias_zone"))?;
        if binding.zone.provider != CloudProvider::Aws
            || binding.zone.visibility != ZoneVisibility::Public
        {
            return Err(validation("public_route53_alias_zone_required"));
        }
        let owner = DnsOwnerName::new(binding.hostname.as_str().to_string())
            .map_err(|_| validation("invalid_route53_alias_owner"))?;
        let mut record_sets = Vec::new();
        for record_type in std::iter::once(ProviderDnsRecordType::A)
            .chain(ipv6_enabled.then_some(ProviderDnsRecordType::Aaaa))
        {
            let record = ProviderDnsRecordSet {
                key: DnsRecordSetKey {
                    owner: owner.clone(),
                    record_type,
                    routing: DnsRoutingIdentity::Simple,
                },
                ttl: DnsTtl::Inherited,
                values: BTreeSet::new(),
                extension: Some(DnsRecordExtension::Route53 {
                    alias_target: Some(Route53AliasTarget {
                        target_zone_id: catalog.hosted_zone_id.clone(),
                        target: target.clone(),
                        evaluate_target_health: false,
                    }),
                    routing_policy: None,
                    health_check_id: None,
                }),
            };
            record
                .validate(&binding.zone)
                .map_err(|_| validation("invalid_route53_cloudfront_alias_record"))?;
            record_sets.push(record);
        }
        record_sets.sort_by(|left, right| left.key.cmp(&right.key));
        desired.push(CloudFrontRoute53AliasDesiredState {
            zone: binding.zone.clone(),
            hostname: binding.hostname.clone(),
            record_sets,
        });
    }
    desired.sort_by(|left, right| {
        (
            left.zone.provider_account_id.as_str(),
            left.zone.zone_id.as_str(),
            left.hostname.as_str(),
        )
            .cmp(&(
                right.zone.provider_account_id.as_str(),
                right.zone.zone_id.as_str(),
                right.hostname.as_str(),
            ))
    });
    Ok(desired)
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use async_trait::async_trait;
    use edgion_center_core::{CredentialSource, ProviderAccountScope, ProviderAccountSpec};

    use super::*;
    use crate::tests::{detail, summary, FakeApi, ACCOUNT_ID};
    use crate::{
        CloudFrontApi, CloudFrontDistributionDetail, CloudFrontDistributionPage,
        CloudFrontDomainConflict, CloudFrontDomainConflictPage,
        CloudFrontDomainConflictResourceType, CloudFrontPolicyKind, CloudFrontPolicyPage,
        CloudFrontPolicyScope, CloudFrontTags,
    };

    const CERTIFICATE_ARN: &str =
        "arn:aws:acm:us-east-1:123456789012:certificate/12345678-1234-1234-1234-123456789012";

    struct DomainFakeApi {
        base: FakeApi,
        certificate: Option<AcmCertificateObservation>,
        conflict_pages: BTreeMap<(String, Option<String>), CloudFrontDomainConflictPage>,
    }

    #[async_trait]
    impl CloudFrontApi for DomainFakeApi {
        fn verified_account_id(&self) -> &str {
            self.base.verified_account_id()
        }

        fn verified_partition(&self) -> AwsPartition {
            self.base.verified_partition()
        }

        fn credential_revision(&self) -> &str {
            self.base.credential_revision()
        }

        async fn list_distributions(
            &self,
            marker: Option<&str>,
            max_items: u16,
        ) -> CloudFrontApiResult<CloudFrontDistributionPage> {
            self.base.list_distributions(marker, max_items).await
        }

        async fn get_distribution(
            &self,
            distribution_id: &str,
        ) -> CloudFrontApiResult<Option<CloudFrontDistributionDetail>> {
            self.base.get_distribution(distribution_id).await
        }

        async fn list_policies(
            &self,
            kind: CloudFrontPolicyKind,
            scope: CloudFrontPolicyScope,
            marker: Option<&str>,
            max_items: u16,
        ) -> CloudFrontApiResult<CloudFrontPolicyPage> {
            self.base
                .list_policies(kind, scope, marker, max_items)
                .await
        }

        async fn describe_acm_certificate(
            &self,
            _certificate_arn: &str,
        ) -> CloudFrontApiResult<Option<AcmCertificateObservation>> {
            Ok(self.certificate.clone())
        }

        async fn list_domain_conflicts(
            &self,
            domain: &str,
            validation_distribution_id: &str,
            marker: Option<&str>,
            _max_items: u16,
        ) -> CloudFrontApiResult<CloudFrontDomainConflictPage> {
            Ok(self
                .conflict_pages
                .get(&(domain.to_string(), marker.map(ToString::to_string)))
                .cloned()
                .unwrap_or_else(|| CloudFrontDomainConflictPage {
                    queried_domain: domain.to_string(),
                    validation_distribution_id: validation_distribution_id.to_string(),
                    items: Vec::new(),
                    next_marker: None,
                }))
        }

        async fn list_tags_for_resource(&self, arn: &str) -> CloudFrontApiResult<CloudFrontTags> {
            self.base.list_tags_for_resource(arn).await
        }
    }

    struct CatalogSource(CloudFrontAliasCatalogRecord);

    #[async_trait]
    impl CloudFrontAliasCatalogSource for CatalogSource {
        async fn standard_distribution_alias(
            &self,
            _partition: AwsPartition,
        ) -> CloudFrontApiResult<Option<CloudFrontAliasCatalogRecord>> {
            Ok(Some(self.0.clone()))
        }
    }

    fn certificate() -> AcmCertificateObservation {
        AcmCertificateObservation {
            arn: CERTIFICATE_ARN.to_string(),
            account_id: ACCOUNT_ID.to_string(),
            partition: AwsPartition::Aws,
            region: "us-east-1".to_string(),
            domain_name: "unused.example.test".to_string(),
            subject_alternative_names: BTreeSet::from(["*.example.test".to_string()]),
            status: AcmCertificateStatus::Issued,
            certificate_type: AcmCertificateType::AmazonIssued,
            key_algorithm: AcmCertificateKeyAlgorithm::Rsa2048,
            managed_by: None,
            not_before_unix_seconds: Some(1),
            not_after_unix_seconds: Some(200_000),
            in_use_by: BTreeSet::new(),
        }
    }

    fn catalog() -> CatalogSource {
        CatalogSource(CloudFrontAliasCatalogRecord {
            source_id: CloudResourceId::new("aws-endpoint-catalog").unwrap(),
            revision: "catalog-2026-07-18".to_string(),
            partition: AwsPartition::Aws,
            target_kind: CloudFrontAliasCatalogTargetKind::StandardDistribution,
            dns_suffix: "cloudfront.net".to_string(),
            hosted_zone_id: "Z2FDTNDATAQYW2".to_string(),
        })
    }

    fn adapter_and_inventory(
        certificate: AcmCertificateObservation,
        ipv6_enabled: bool,
    ) -> (CloudFrontInventoryAdapter, CloudFrontPlanningInventory) {
        adapter_and_inventory_with_conflicts(certificate, ipv6_enabled, false, BTreeMap::new())
    }

    fn adapter_and_inventory_with_conflicts(
        certificate: AcmCertificateObservation,
        ipv6_enabled: bool,
        certificate_attached: bool,
        conflict_pages: BTreeMap<(String, Option<String>), CloudFrontDomainConflictPage>,
    ) -> (CloudFrontInventoryAdapter, CloudFrontPlanningInventory) {
        let summary = summary();
        let mut distribution_detail = detail(summary.clone());
        distribution_detail.config.ipv6_enabled = ipv6_enabled;
        if certificate_attached {
            distribution_detail
                .config
                .viewer_certificate
                .cloudfront_default_certificate = false;
            distribution_detail
                .config
                .viewer_certificate
                .certificate_arn = Some(CERTIFICATE_ARN.to_string());
            distribution_detail
                .config
                .viewer_certificate
                .certificate_source = Some("acm".to_string());
            distribution_detail
                .config
                .viewer_certificate
                .ssl_support_method = Some("sni-only".to_string());
        }
        let api = Arc::new(DomainFakeApi {
            base: FakeApi {
                account_id: ACCOUNT_ID.to_string(),
                partition: AwsPartition::Aws,
                pages: vec![CloudFrontDistributionPage {
                    items: vec![summary],
                    is_truncated: false,
                    next_marker: None,
                }],
                detail: Some(distribution_detail),
                tags: CloudFrontTags::default(),
            },
            certificate: Some(certificate),
            conflict_pages,
        });
        let adapter = CloudFrontInventoryAdapter::new(
            CloudResourceId::new("aws-main").unwrap(),
            7,
            &ProviderAccountSpec {
                provider: CloudProvider::Aws,
                scope: Some(ProviderAccountScope::Aws {
                    account_id: ACCOUNT_ID.to_string(),
                }),
                credential_source: CredentialSource::Ambient,
            },
            api,
        )
        .unwrap();
        let inventory = tokio::runtime::Runtime::new().unwrap().block_on(async {
            adapter
                .planning_inventory("domain-observation", 1_000, 2_000)
                .await
                .unwrap()
        });
        (adapter, inventory)
    }

    fn zone(visibility: ZoneVisibility) -> DnsZoneRef {
        DnsZoneRef {
            provider_account_id: CloudResourceId::new("route53-other-account").unwrap(),
            provider: CloudProvider::Aws,
            zone_id: DnsZoneId::new("ZROUTE53EXAMPLE").unwrap(),
            apex: AbsoluteDnsName::new("example.test").unwrap(),
            visibility,
        }
    }

    fn public_zone(account: &str, zone_id: &str, apex: &str) -> DnsZoneRef {
        DnsZoneRef {
            provider_account_id: CloudResourceId::new(account).unwrap(),
            provider: CloudProvider::Aws,
            zone_id: DnsZoneId::new(zone_id).unwrap(),
            apex: AbsoluteDnsName::new(apex).unwrap(),
            visibility: ZoneVisibility::Public,
        }
    }

    #[test]
    fn live_certificate_and_catalog_build_cross_account_a_and_aaaa_desired_state() {
        let (adapter, inventory) = adapter_and_inventory(certificate(), true);
        let hostname = DomainName::new("www.example.test").unwrap();
        let required = BTreeSet::from([hostname.clone()]);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let certificate = runtime.block_on(async {
            adapter
                .planning_external_certificate_evidence(
                    &inventory,
                    "E123EXAMPLE",
                    CERTIFICATE_ARN,
                    required,
                    1_500,
                    1_900,
                )
                .await
                .unwrap()
        });
        let catalog_evidence = runtime
            .block_on(observe_cloudfront_alias_catalog(
                &catalog(),
                AwsPartition::Aws,
                1_500,
            ))
            .unwrap();
        assert_eq!(catalog_evidence.observed_at_unix_ms, 1_500);
        assert_eq!(catalog_evidence.valid_until_unix_ms, 301_500);
        let plan = build_custom_domain_plan(
            CloudFrontCustomDomainPlanRequest {
                distribution_id: "E123EXAMPLE".to_string(),
                aliases_to_attach: BTreeSet::from([hostname.clone()]),
                certificate_arn: CERTIFICATE_ARN.to_string(),
                dns_bindings: vec![CloudFrontAliasDnsBindingIntent {
                    hostname,
                    zone: zone(ZoneVisibility::Public),
                }],
                now_unix_ms: 1_500,
            },
            &inventory,
            &certificate,
            None,
            &catalog_evidence,
        )
        .unwrap();
        assert_eq!(plan.desired_route53_aliases.len(), 1);
        assert_eq!(plan.desired_route53_aliases[0].record_sets.len(), 2);
        assert_eq!(
            plan.desired_route53_aliases[0]
                .record_sets
                .iter()
                .map(|record| record.key.record_type)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([ProviderDnsRecordType::A, ProviderDnsRecordType::Aaaa])
        );
        assert!(plan.desired_route53_aliases[0]
            .record_sets
            .iter()
            .all(|record| matches!(
                &record.extension,
                Some(DnsRecordExtension::Route53 {
                    alias_target: Some(target),
                    ..
                }) if target.target_zone_id.as_str() == "Z2FDTNDATAQYW2"
                    && !target.evaluate_target_health
            )));
        assert!(!plan.dispatch_authorized);
        assert!(!plan.domain_conflict_evidence_available);
        let serialized = serde_json::to_string(&plan).unwrap();
        assert!(serialized.contains("\"dispatchAuthorized\":false"));
        assert!(!serialized.contains("privateKey"));
        assert!(!serialized.contains("certificateBody"));
    }

    #[test]
    fn deployed_certificate_and_complete_empty_scan_seal_conflict_free_plan() {
        let hostname = DomainName::new("www.example.test").unwrap();
        let page = |next_marker| CloudFrontDomainConflictPage {
            queried_domain: hostname.as_str().to_string(),
            validation_distribution_id: "E123EXAMPLE".to_string(),
            items: Vec::new(),
            next_marker,
        };
        let conflict_pages = BTreeMap::from([
            (
                (hostname.as_str().to_string(), None),
                page(Some("page-2".to_string())),
            ),
            (
                (hostname.as_str().to_string(), Some("page-2".to_string())),
                page(None),
            ),
        ]);
        let (adapter, inventory) =
            adapter_and_inventory_with_conflicts(certificate(), false, true, conflict_pages);
        let aliases = BTreeSet::from([hostname.clone()]);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let certificate_evidence = runtime
            .block_on(adapter.planning_external_certificate_evidence(
                &inventory,
                "E123EXAMPLE",
                CERTIFICATE_ARN,
                aliases.clone(),
                1_500,
                1_900,
            ))
            .unwrap();
        let conflicts = runtime
            .block_on(adapter.planning_domain_conflict_evidence(
                &inventory,
                &certificate_evidence,
                "E123EXAMPLE",
                &aliases,
                1_500,
                1_900,
            ))
            .unwrap();
        let catalog = runtime
            .block_on(observe_cloudfront_alias_catalog(
                &catalog(),
                AwsPartition::Aws,
                1_500,
            ))
            .unwrap();
        let plan = build_custom_domain_plan(
            CloudFrontCustomDomainPlanRequest {
                distribution_id: "E123EXAMPLE".to_string(),
                aliases_to_attach: aliases,
                certificate_arn: CERTIFICATE_ARN.to_string(),
                dns_bindings: vec![CloudFrontAliasDnsBindingIntent {
                    hostname,
                    zone: zone(ZoneVisibility::Public),
                }],
                now_unix_ms: 1_500,
            },
            &inventory,
            &certificate_evidence,
            Some(&conflicts),
            &catalog,
        )
        .unwrap();
        assert!(plan.domain_conflict_evidence_available);
        assert!(!plan
            .dispatch_blockers
            .contains("cloudfront_domain_conflict_evidence_unavailable"));
        assert!(!plan.dispatch_authorized);
    }

    #[test]
    fn conflict_observation_requires_deployed_certificate_and_blocks_any_result() {
        let hostname = DomainName::new("www.example.test").unwrap();
        let aliases = BTreeSet::from([hostname.clone()]);
        let (adapter, inventory) = adapter_and_inventory(certificate(), false);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let undeployed_certificate = runtime
            .block_on(adapter.planning_external_certificate_evidence(
                &inventory,
                "E123EXAMPLE",
                CERTIFICATE_ARN,
                aliases.clone(),
                1_500,
                1_900,
            ))
            .unwrap();
        let error = runtime
            .block_on(adapter.planning_domain_conflict_evidence(
                &inventory,
                &undeployed_certificate,
                "E123EXAMPLE",
                &aliases,
                1_500,
                1_900,
            ))
            .unwrap_err();
        assert_eq!(
            error.code(),
            "cloudfront_conflict_validation_certificate_not_deployed"
        );

        let conflict_pages = BTreeMap::from([(
            (hostname.as_str().to_string(), None),
            CloudFrontDomainConflictPage {
                queried_domain: hostname.as_str().to_string(),
                validation_distribution_id: "E123EXAMPLE".to_string(),
                items: vec![CloudFrontDomainConflict {
                    domain: "*.example.test".to_string(),
                    resource_type: CloudFrontDomainConflictResourceType::DistributionTenant,
                    resource_id: "********foreign".to_string(),
                    account_id: "******789012".to_string(),
                }],
                next_marker: None,
            },
        )]);
        let (adapter, inventory) =
            adapter_and_inventory_with_conflicts(certificate(), false, true, conflict_pages);
        let certificate = runtime
            .block_on(adapter.planning_external_certificate_evidence(
                &inventory,
                "E123EXAMPLE",
                CERTIFICATE_ARN,
                aliases.clone(),
                1_500,
                1_900,
            ))
            .unwrap();
        let error = runtime
            .block_on(adapter.planning_domain_conflict_evidence(
                &inventory,
                &certificate,
                "E123EXAMPLE",
                &aliases,
                1_500,
                1_900,
            ))
            .unwrap_err();
        assert_eq!(error.code(), "cloudfront_domain_conflict_detected");
        assert!(!error.message().contains("foreign"));
    }

    #[test]
    fn later_page_conflicts_and_marker_cycles_never_seal_evidence() {
        let hostname = DomainName::new("www.example.test").unwrap();
        let aliases = BTreeSet::from([hostname.clone()]);
        let page = |items, next_marker| CloudFrontDomainConflictPage {
            queried_domain: hostname.as_str().to_string(),
            validation_distribution_id: "E123EXAMPLE".to_string(),
            items,
            next_marker,
        };
        let first_key = (hostname.as_str().to_string(), None);
        let second_key = (hostname.as_str().to_string(), Some("page-2".to_string()));
        for (second_page, expected_code) in [
            (
                page(
                    vec![CloudFrontDomainConflict {
                        domain: hostname.as_str().to_string(),
                        resource_type: CloudFrontDomainConflictResourceType::Distribution,
                        resource_id: "********conflict".to_string(),
                        account_id: "******789012".to_string(),
                    }],
                    None,
                ),
                "cloudfront_domain_conflict_detected",
            ),
            (
                page(Vec::new(), Some("page-2".to_string())),
                "cloudfront_domain_conflict_marker_cycle",
            ),
        ] {
            let conflict_pages = BTreeMap::from([
                (
                    first_key.clone(),
                    page(Vec::new(), Some("page-2".to_string())),
                ),
                (second_key.clone(), second_page),
            ]);
            let (adapter, inventory) =
                adapter_and_inventory_with_conflicts(certificate(), false, true, conflict_pages);
            let runtime = tokio::runtime::Runtime::new().unwrap();
            let certificate = runtime
                .block_on(adapter.planning_external_certificate_evidence(
                    &inventory,
                    "E123EXAMPLE",
                    CERTIFICATE_ARN,
                    aliases.clone(),
                    1_500,
                    1_900,
                ))
                .unwrap();
            let error = runtime
                .block_on(adapter.planning_domain_conflict_evidence(
                    &inventory,
                    &certificate,
                    "E123EXAMPLE",
                    &aliases,
                    1_500,
                    1_900,
                ))
                .unwrap_err();
            assert_eq!(error.code(), expected_code);
        }
    }

    #[test]
    fn ipv4_only_plan_requires_one_dns_binding_for_every_requested_alias() {
        let (adapter, inventory) = adapter_and_inventory(certificate(), false);
        let hostname = DomainName::new("www.example.test").unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let evidence = runtime
            .block_on(adapter.planning_external_certificate_evidence(
                &inventory,
                "E123EXAMPLE",
                CERTIFICATE_ARN,
                BTreeSet::from([hostname.clone()]),
                1_500,
                1_900,
            ))
            .unwrap();
        let catalog_evidence = runtime
            .block_on(observe_cloudfront_alias_catalog(
                &catalog(),
                AwsPartition::Aws,
                1_500,
            ))
            .unwrap();
        let request = |dns_bindings| CloudFrontCustomDomainPlanRequest {
            distribution_id: "E123EXAMPLE".to_string(),
            aliases_to_attach: BTreeSet::from([hostname.clone()]),
            certificate_arn: CERTIFICATE_ARN.to_string(),
            dns_bindings,
            now_unix_ms: 1_500,
        };
        let plan = build_custom_domain_plan(
            request(vec![CloudFrontAliasDnsBindingIntent {
                hostname: hostname.clone(),
                zone: zone(ZoneVisibility::Public),
            }]),
            &inventory,
            &evidence,
            None,
            &catalog_evidence,
        )
        .unwrap();
        assert_eq!(plan.desired_route53_aliases.len(), 1);
        assert_eq!(plan.desired_route53_aliases[0].record_sets.len(), 1);
        assert_eq!(
            plan.desired_route53_aliases[0].record_sets[0]
                .key
                .record_type,
            ProviderDnsRecordType::A
        );

        let error = build_custom_domain_plan(
            request(Vec::new()),
            &inventory,
            &evidence,
            None,
            &catalog_evidence,
        )
        .unwrap_err();
        assert_eq!(error.code(), "cloudfront_alias_dns_binding_set_mismatch");
    }

    #[test]
    fn serialized_plan_retains_each_cross_account_zone_binding() {
        let mut observed_certificate = certificate();
        observed_certificate
            .subject_alternative_names
            .insert("*.sub.example.test".to_string());
        let (adapter, inventory) = adapter_and_inventory(observed_certificate, false);
        let first = DomainName::new("www.example.test").unwrap();
        let second = DomainName::new("api.sub.example.test").unwrap();
        let aliases = BTreeSet::from([first.clone(), second.clone()]);
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let evidence = runtime
            .block_on(adapter.planning_external_certificate_evidence(
                &inventory,
                "E123EXAMPLE",
                CERTIFICATE_ARN,
                aliases.clone(),
                1_500,
                1_900,
            ))
            .unwrap();
        let catalog_evidence = runtime
            .block_on(observe_cloudfront_alias_catalog(
                &catalog(),
                AwsPartition::Aws,
                1_500,
            ))
            .unwrap();
        let plan = build_custom_domain_plan(
            CloudFrontCustomDomainPlanRequest {
                distribution_id: "E123EXAMPLE".to_string(),
                aliases_to_attach: aliases,
                certificate_arn: CERTIFICATE_ARN.to_string(),
                dns_bindings: vec![
                    CloudFrontAliasDnsBindingIntent {
                        hostname: second,
                        zone: public_zone("route53-b", "ZBEXAMPLE", "sub.example.test"),
                    },
                    CloudFrontAliasDnsBindingIntent {
                        hostname: first,
                        zone: public_zone("route53-a", "ZAEXAMPLE", "example.test"),
                    },
                ],
                now_unix_ms: 1_500,
            },
            &inventory,
            &evidence,
            None,
            &catalog_evidence,
        )
        .unwrap();
        assert_eq!(plan.desired_route53_aliases.len(), 2);
        assert_eq!(
            plan.desired_route53_aliases[0]
                .zone
                .provider_account_id
                .as_str(),
            "route53-a"
        );
        assert_eq!(
            plan.desired_route53_aliases[1].zone.zone_id.as_str(),
            "ZBEXAMPLE"
        );
        let serialized = serde_json::to_string(&plan).unwrap();
        assert!(serialized.contains("route53-a"));
        assert!(serialized.contains("ZAEXAMPLE"));
        assert!(serialized.contains("route53-b"));
        assert!(serialized.contains("ZBEXAMPLE"));
    }

    #[test]
    fn certificate_compatibility_is_exact_fresh_and_san_only() {
        for mutate in [
            |value: &mut AcmCertificateObservation| value.status = AcmCertificateStatus::Expired,
            |value: &mut AcmCertificateObservation| {
                value.certificate_type = AcmCertificateType::Imported;
            },
            |value: &mut AcmCertificateObservation| {
                value.key_algorithm = AcmCertificateKeyAlgorithm::Unsupported("EC_384".to_string());
            },
            |value: &mut AcmCertificateObservation| {
                value.managed_by = Some("CLOUDFRONT".to_string())
            },
            |value: &mut AcmCertificateObservation| {
                value
                    .in_use_by
                    .insert("arn:aws:cloudfront::123456789012:distribution/EOTHER".to_string());
            },
        ] {
            let mut observed = certificate();
            mutate(&mut observed);
            let (adapter, inventory) = adapter_and_inventory(observed, false);
            let result = tokio::runtime::Runtime::new().unwrap().block_on(async {
                adapter
                    .planning_external_certificate_evidence(
                        &inventory,
                        "E123EXAMPLE",
                        CERTIFICATE_ARN,
                        BTreeSet::from([DomainName::new("www.example.test").unwrap()]),
                        1_500,
                        1_900,
                    )
                    .await
            });
            assert!(result.is_err());
        }

        let (adapter, inventory) = adapter_and_inventory(certificate(), false);
        let result = tokio::runtime::Runtime::new().unwrap().block_on(async {
            adapter
                .planning_external_certificate_evidence(
                    &inventory,
                    "E123EXAMPLE",
                    CERTIFICATE_ARN,
                    BTreeSet::from([DomainName::new("deep.www.example.test").unwrap()]),
                    1_500,
                    1_900,
                )
                .await
        });
        assert_eq!(
            result.unwrap_err().code(),
            "invalid_cloudfront_certificate_evidence"
        );

        let mut insufficient_validity = certificate();
        insufficient_validity.not_after_unix_seconds = Some(86_401);
        let (adapter, inventory) = adapter_and_inventory(insufficient_validity, false);
        let result = tokio::runtime::Runtime::new().unwrap().block_on(async {
            adapter
                .planning_external_certificate_evidence(
                    &inventory,
                    "E123EXAMPLE",
                    CERTIFICATE_ARN,
                    BTreeSet::from([DomainName::new("www.example.test").unwrap()]),
                    1_500,
                    1_900,
                )
                .await
        });
        assert_eq!(
            result.unwrap_err().code(),
            "invalid_cloudfront_certificate_evidence"
        );

        let mut wrong_account = certificate();
        wrong_account.account_id = "999999999999".to_string();
        let (adapter, inventory) = adapter_and_inventory(wrong_account, false);
        let result = tokio::runtime::Runtime::new().unwrap().block_on(async {
            adapter
                .planning_external_certificate_evidence(
                    &inventory,
                    "E123EXAMPLE",
                    CERTIFICATE_ARN,
                    BTreeSet::from([DomainName::new("www.example.test").unwrap()]),
                    1_500,
                    1_900,
                )
                .await
        });
        assert_eq!(
            result.unwrap_err().code(),
            "cloudfront_acm_certificate_scope_mismatch"
        );
    }

    #[test]
    fn certificate_and_catalog_evidence_expire_before_plan_reuse() {
        let (adapter, inventory) = adapter_and_inventory(certificate(), false);
        let hostname = DomainName::new("www.example.test").unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let evidence = runtime
            .block_on(adapter.planning_external_certificate_evidence(
                &inventory,
                "E123EXAMPLE",
                CERTIFICATE_ARN,
                BTreeSet::from([hostname.clone()]),
                1_500,
                1_600,
            ))
            .unwrap();
        let catalog_evidence = runtime
            .block_on(observe_cloudfront_alias_catalog(
                &catalog(),
                AwsPartition::Aws,
                1_500,
            ))
            .unwrap();
        let error = build_custom_domain_plan(
            CloudFrontCustomDomainPlanRequest {
                distribution_id: "E123EXAMPLE".to_string(),
                aliases_to_attach: BTreeSet::from([hostname.clone()]),
                certificate_arn: CERTIFICATE_ARN.to_string(),
                dns_bindings: vec![CloudFrontAliasDnsBindingIntent {
                    hostname,
                    zone: zone(ZoneVisibility::Public),
                }],
                now_unix_ms: 1_700,
            },
            &inventory,
            &evidence,
            None,
            &catalog_evidence,
        )
        .unwrap_err();
        assert_eq!(error.code(), "invalid_cloudfront_certificate_evidence");

        assert_eq!(
            catalog_evidence.validate_at(301_500).unwrap_err().code(),
            "invalid_cloudfront_alias_catalog_evidence"
        );

        let mut wrong_suffix = catalog().0;
        wrong_suffix.dns_suffix = "example.invalid".to_string();
        let error = runtime
            .block_on(observe_cloudfront_alias_catalog(
                &CatalogSource(wrong_suffix),
                AwsPartition::Aws,
                1_500,
            ))
            .unwrap_err();
        assert_eq!(error.code(), "invalid_cloudfront_alias_catalog_evidence");
    }

    #[test]
    fn catalog_and_dns_boundaries_fail_closed_without_mutation_authority() {
        assert_eq!(
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(observe_cloudfront_alias_catalog(
                    &catalog(),
                    AwsPartition::AwsChina,
                    1_500,
                ))
                .unwrap_err()
                .code(),
            "unsupported_cloudfront_custom_domain_partition"
        );

        let (adapter, inventory) = adapter_and_inventory(certificate(), false);
        let hostname = DomainName::new("www.example.test").unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let evidence = runtime.block_on(async {
            adapter
                .planning_external_certificate_evidence(
                    &inventory,
                    "E123EXAMPLE",
                    CERTIFICATE_ARN,
                    BTreeSet::from([hostname.clone()]),
                    1_500,
                    1_900,
                )
                .await
                .unwrap()
        });
        let catalog = runtime
            .block_on(observe_cloudfront_alias_catalog(
                &catalog(),
                AwsPartition::Aws,
                1_500,
            ))
            .unwrap();
        let error = build_custom_domain_plan(
            CloudFrontCustomDomainPlanRequest {
                distribution_id: "E123EXAMPLE".to_string(),
                aliases_to_attach: BTreeSet::from([hostname.clone()]),
                certificate_arn: CERTIFICATE_ARN.to_string(),
                dns_bindings: vec![CloudFrontAliasDnsBindingIntent {
                    hostname,
                    zone: zone(ZoneVisibility::Private),
                }],
                now_unix_ms: 1_500,
            },
            &inventory,
            &evidence,
            None,
            &catalog,
        )
        .unwrap_err();
        assert_eq!(error.code(), "public_route53_alias_zone_required");
    }
}
