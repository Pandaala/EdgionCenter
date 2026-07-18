//! Observation-bound CloudFront cache-behavior fragments and deterministic local preview.

use std::collections::{BTreeMap, BTreeSet};

use edgion_center_core::{CloudResourceId, DomainName};
use serde::Serialize;

use crate::{
    model::validation, CloudFrontApiResult, CloudFrontCacheBehaviorProjection,
    CloudFrontCustomOriginFragment, CloudFrontDetailObservation,
    CloudFrontDistributionObservationBinding, CloudFrontObservationAuthority,
    CloudFrontOriginGroupFragment, CloudFrontOriginId, CloudFrontOriginTargetRef,
    CloudFrontPlanningInventory, CloudFrontPolicyKind, CloudFrontPolicyScope,
    CloudFrontPolicySummary, CloudFrontViewerMethod,
};

const MAX_ORDERED_BEHAVIORS: usize = 75;
const MAX_PATH_PATTERN_LEN: usize = 255;
const MAX_POLICY_ID_LEN: usize = 128;
const MAX_POLICY_NAME_LEN: usize = 256;
const MAX_POLICY_ETAG_LEN: usize = 512;
const MAX_POLICY_OBSERVATIONS: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudFrontViewerProtocolPolicy {
    AllowAll,
    HttpsOnly,
    RedirectToHttps,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct CloudFrontPolicyId(String);

impl CloudFrontPolicyId {
    pub fn new(value: impl Into<String>) -> CloudFrontApiResult<Self> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_POLICY_ID_LEN
            || value.trim() != value
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(validation("invalid_cloudfront_policy_id"));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct CloudFrontPathPattern(String);

impl CloudFrontPathPattern {
    pub fn new(value: impl Into<String>) -> CloudFrontApiResult<Self> {
        let mut value = value.into();
        if value.is_empty() {
            return Err(validation("invalid_cloudfront_behavior_path_pattern"));
        }
        if !value.starts_with('/') {
            value.insert(0, '/');
        }
        if value.len() > MAX_PATH_PATTERN_LEN {
            return Err(validation("invalid_cloudfront_behavior_path_pattern"));
        }
        let wildcard_count = value.bytes().filter(|byte| *byte == b'*').count();
        if value == "/*"
            || value.contains('?')
            || value.contains('%')
            || value.contains('#')
            || value.contains('&')
            || value.chars().any(char::is_control)
            || !value.is_ascii()
            || wildcard_count > 1
            || (wildcard_count == 1 && !value.ends_with('*'))
            || !value.bytes().all(is_supported_pattern_byte)
        {
            return Err(validation("unsupported_cloudfront_behavior_path_pattern"));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn matches(&self, path: &str) -> bool {
        self.0
            .strip_suffix('*')
            .map_or(self.0 == path, |prefix| path.starts_with(prefix))
    }

    fn definitely_shadows(&self, later: &Self) -> bool {
        match self.0.strip_suffix('*') {
            Some(prefix) => later.0.starts_with(prefix),
            None => self == later,
        }
    }

    fn overlaps(&self, other: &Self) -> bool {
        match (self.0.strip_suffix('*'), other.0.strip_suffix('*')) {
            (None, None) => self == other,
            (Some(prefix), None) => other.0.starts_with(prefix),
            (None, Some(prefix)) => self.0.starts_with(prefix),
            (Some(left), Some(right)) => left.starts_with(right) || right.starts_with(left),
        }
    }
}

fn is_supported_pattern_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'_' | b'-' | b'.' | b'*' | b'$' | b'/' | b'~' | b'"' | b'\'' | b'@' | b':' | b'+'
        )
}

/// A complete, bounded, non-deserializable policy observation from one live read window.
pub struct CloudFrontPolicyPlanningInventory {
    authority: CloudFrontObservationAuthority,
    policies: BTreeMap<(CloudFrontPolicyKind, String), CloudFrontPolicySummary>,
}

impl CloudFrontPolicyPlanningInventory {
    pub(crate) fn new(
        authority: CloudFrontObservationAuthority,
        policies: Vec<CloudFrontPolicySummary>,
    ) -> CloudFrontApiResult<Self> {
        authority.validate()?;
        if policies.len() > MAX_POLICY_OBSERVATIONS {
            return Err(validation("cloudfront_policy_inventory_limit"));
        }
        let mut indexed = BTreeMap::new();
        for policy in policies {
            validate_policy_summary(&policy)?;
            let key = (policy.kind, policy.id.clone());
            if indexed.insert(key, policy).is_some() {
                return Err(validation("duplicate_cloudfront_policy_observation"));
            }
        }
        Ok(Self {
            authority,
            policies: indexed,
        })
    }

    fn reference(
        &self,
        binding: &CloudFrontDistributionObservationBinding,
        kind: CloudFrontPolicyKind,
        id: &CloudFrontPolicyId,
        now_unix_ms: i64,
    ) -> CloudFrontApiResult<CloudFrontObservedPolicyRef> {
        validate_authority_compatibility(&self.authority, binding, now_unix_ms)?;
        let policy = self
            .policies
            .get(&(kind, id.as_str().to_string()))
            .ok_or_else(|| validation("cloudfront_policy_observation_missing"))?;
        Ok(CloudFrontObservedPolicyRef {
            authority: self.authority.clone(),
            kind,
            id: id.clone(),
            scope: policy.scope,
            etag: policy.etag.clone(),
            last_modified_unix_seconds: policy.last_modified_unix_seconds,
        })
    }
}

fn validate_policy_summary(policy: &CloudFrontPolicySummary) -> CloudFrontApiResult<()> {
    CloudFrontPolicyId::new(policy.id.clone())?;
    if policy.name.is_empty()
        || policy.name.len() > MAX_POLICY_NAME_LEN
        || policy.name.trim() != policy.name
        || policy.name.chars().any(char::is_control)
        || policy.etag.is_empty()
        || policy.etag.len() > MAX_POLICY_ETAG_LEN
        || policy.etag.trim() != policy.etag
        || policy.etag.chars().any(char::is_control)
        || policy.last_modified_unix_seconds <= 0
    {
        return Err(validation("invalid_cloudfront_policy_observation"));
    }
    Ok(())
}

fn validate_authority_compatibility(
    authority: &CloudFrontObservationAuthority,
    binding: &CloudFrontDistributionObservationBinding,
    now_unix_ms: i64,
) -> CloudFrontApiResult<()> {
    authority.validate()?;
    binding.validate_at(now_unix_ms)?;
    let distribution = binding.authority();
    if !authority.is_fresh_at(now_unix_ms)
        || authority.provider_account_id() != distribution.provider_account_id()
        || authority.aws_account_id() != distribution.aws_account_id()
        || authority.partition() != distribution.partition()
        || authority.account_generation() != distribution.account_generation()
        || authority.credential_revision() != distribution.credential_revision()
    {
        return Err(validation(
            "cloudfront_policy_distribution_authority_mismatch",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontObservedPolicyRef {
    authority: CloudFrontObservationAuthority,
    kind: CloudFrontPolicyKind,
    id: CloudFrontPolicyId,
    scope: CloudFrontPolicyScope,
    etag: String,
    last_modified_unix_seconds: i64,
}

impl CloudFrontObservedPolicyRef {
    pub fn id(&self) -> &CloudFrontPolicyId {
        &self.id
    }

    pub fn kind(&self) -> CloudFrontPolicyKind {
        self.kind
    }

    fn validate_at(
        &self,
        binding: &CloudFrontDistributionObservationBinding,
        expected_kind: CloudFrontPolicyKind,
        now_unix_ms: i64,
    ) -> CloudFrontApiResult<()> {
        validate_authority_compatibility(&self.authority, binding, now_unix_ms)?;
        if self.kind != expected_kind
            || self.id.as_str().is_empty()
            || self.etag.is_empty()
            || self.last_modified_unix_seconds <= 0
        {
            return Err(validation("invalid_cloudfront_policy_reference"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontBehaviorPolicyIntent {
    pub cache_policy_id: CloudFrontPolicyId,
    pub origin_request_policy_id: Option<CloudFrontPolicyId>,
    pub response_headers_policy_id: Option<CloudFrontPolicyId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontBehaviorPolicyRefs {
    cache_policy: CloudFrontObservedPolicyRef,
    origin_request_policy: Option<CloudFrontObservedPolicyRef>,
    response_headers_policy: Option<CloudFrontObservedPolicyRef>,
}

impl CloudFrontBehaviorPolicyRefs {
    fn validate_at(
        &self,
        binding: &CloudFrontDistributionObservationBinding,
        now_unix_ms: i64,
    ) -> CloudFrontApiResult<()> {
        self.cache_policy
            .validate_at(binding, CloudFrontPolicyKind::Cache, now_unix_ms)?;
        if let Some(policy) = &self.origin_request_policy {
            policy.validate_at(binding, CloudFrontPolicyKind::OriginRequest, now_unix_ms)?;
        }
        if let Some(policy) = &self.response_headers_policy {
            policy.validate_at(binding, CloudFrontPolicyKind::ResponseHeaders, now_unix_ms)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontBehaviorIntent {
    pub behavior_id: CloudResourceId,
    pub generation: u64,
    pub path_pattern: Option<CloudFrontPathPattern>,
    pub target: CloudFrontOriginTargetRef,
    pub viewer_protocol_policy: CloudFrontViewerProtocolPolicy,
    pub allowed_methods: BTreeSet<CloudFrontViewerMethod>,
    pub cached_methods: BTreeSet<CloudFrontViewerMethod>,
    pub compress: bool,
    pub policies: CloudFrontBehaviorPolicyIntent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudFrontBehaviorTargetDetails {
    Origin,
    OriginGroup {
        primary_origin_id: CloudFrontOriginId,
        secondary_origin_id: CloudFrontOriginId,
        failover_status_codes: BTreeSet<u16>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudFrontBehaviorTargetCatalog {
    binding: CloudFrontDistributionObservationBinding,
    targets: BTreeMap<CloudFrontOriginTargetRef, CloudFrontBehaviorTargetObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CloudFrontBehaviorTargetObservation {
    details: CloudFrontBehaviorTargetDetails,
    valid_until_unix_ms: i64,
}

impl CloudFrontBehaviorTargetCatalog {
    pub fn from_inventory(
        inventory: &CloudFrontPlanningInventory,
        distribution_id: &str,
        now_unix_ms: i64,
    ) -> CloudFrontApiResult<Self> {
        let binding = CloudFrontDistributionObservationBinding::from_inventory(
            inventory,
            distribution_id,
            now_unix_ms,
        )?;
        let entry = inventory
            .inventory()
            .entries
            .iter()
            .find(|entry| entry.summary.id == distribution_id)
            .ok_or_else(|| validation("cloudfront_distribution_observation_missing"))?;
        let CloudFrontDetailObservation::Complete(observed) = &entry.detail else {
            return Err(validation("cloudfront_distribution_observation_incomplete"));
        };
        let mut targets = BTreeMap::new();
        for origin in &observed.detail.config.origins {
            let id = CloudFrontOriginId::new(origin.id.clone())?;
            if targets
                .insert(
                    CloudFrontOriginTargetRef::Origin(id),
                    CloudFrontBehaviorTargetObservation {
                        details: CloudFrontBehaviorTargetDetails::Origin,
                        valid_until_unix_ms: binding.authority().valid_until_unix_ms(),
                    },
                )
                .is_some()
            {
                return Err(validation("duplicate_cloudfront_origin_target_id"));
            }
        }
        for group in &observed.detail.config.origin_groups {
            let group_id = crate::CloudFrontOriginGroupId::new(group.id.clone())?;
            let details = CloudFrontBehaviorTargetDetails::OriginGroup {
                primary_origin_id: CloudFrontOriginId::new(group.primary_origin_id.clone())?,
                secondary_origin_id: CloudFrontOriginId::new(group.secondary_origin_id.clone())?,
                failover_status_codes: group.failover_status_codes.clone(),
            };
            if targets
                .insert(
                    CloudFrontOriginTargetRef::OriginGroup(group_id),
                    CloudFrontBehaviorTargetObservation {
                        details,
                        valid_until_unix_ms: binding.authority().valid_until_unix_ms(),
                    },
                )
                .is_some()
            {
                return Err(validation("duplicate_cloudfront_origin_target_id"));
            }
        }
        Ok(Self { binding, targets })
    }

    pub fn include_origin_fragment(
        &mut self,
        fragment: &CloudFrontCustomOriginFragment,
        now_unix_ms: i64,
    ) -> CloudFrontApiResult<()> {
        fragment.validate_at(now_unix_ms)?;
        let target = CloudFrontOriginTargetRef::Origin(fragment.origin_id().clone());
        if fragment.binding() != &self.binding || self.targets.contains_key(&target) {
            return Err(validation("cloudfront_behavior_target_binding_mismatch"));
        }
        self.targets.insert(
            target,
            CloudFrontBehaviorTargetObservation {
                details: CloudFrontBehaviorTargetDetails::Origin,
                valid_until_unix_ms: fragment.approval_valid_until_unix_ms(),
            },
        );
        Ok(())
    }

    pub fn include_origin_group_fragment(
        &mut self,
        fragment: &CloudFrontOriginGroupFragment,
        now_unix_ms: i64,
    ) -> CloudFrontApiResult<()> {
        fragment.validate_at(now_unix_ms)?;
        let details = CloudFrontBehaviorTargetDetails::OriginGroup {
            primary_origin_id: fragment.primary_origin_id().clone(),
            secondary_origin_id: fragment.secondary_origin_id().clone(),
            failover_status_codes: fragment.failover_status_codes().clone(),
        };
        let target = CloudFrontOriginTargetRef::OriginGroup(fragment.group_id().clone());
        if fragment.binding() != &self.binding
            || !self
                .targets
                .contains_key(&CloudFrontOriginTargetRef::Origin(
                    fragment.primary_origin_id().clone(),
                ))
            || !self
                .targets
                .contains_key(&CloudFrontOriginTargetRef::Origin(
                    fragment.secondary_origin_id().clone(),
                ))
            || self.targets.contains_key(&target)
        {
            return Err(validation("cloudfront_behavior_target_binding_mismatch"));
        }
        self.targets.insert(
            target,
            CloudFrontBehaviorTargetObservation {
                details,
                valid_until_unix_ms: fragment.approval_valid_until_unix_ms(),
            },
        );
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontBehaviorFragment {
    binding: CloudFrontDistributionObservationBinding,
    behavior_id: CloudResourceId,
    generation: u64,
    path_pattern: Option<CloudFrontPathPattern>,
    target: CloudFrontOriginTargetRef,
    target_details: CloudFrontBehaviorTargetDetails,
    viewer_protocol_policy: CloudFrontViewerProtocolPolicy,
    allowed_methods: BTreeSet<CloudFrontViewerMethod>,
    cached_methods: BTreeSet<CloudFrontViewerMethod>,
    compress: bool,
    policies: CloudFrontBehaviorPolicyRefs,
    target_valid_until_unix_ms: i64,
}

impl CloudFrontBehaviorFragment {
    pub fn binding(&self) -> &CloudFrontDistributionObservationBinding {
        &self.binding
    }

    pub fn path_pattern(&self) -> Option<&CloudFrontPathPattern> {
        self.path_pattern.as_ref()
    }

    fn validate_at(&self, now_unix_ms: i64) -> CloudFrontApiResult<()> {
        self.binding.validate_at(now_unix_ms)?;
        if self.generation == 0 || now_unix_ms >= self.target_valid_until_unix_ms {
            return Err(validation("stale_cloudfront_behavior_target"));
        }
        self.policies.validate_at(&self.binding, now_unix_ms)
    }
}

pub fn build_behavior_fragment(
    intent: CloudFrontBehaviorIntent,
    targets: &CloudFrontBehaviorTargetCatalog,
    policies: &CloudFrontPolicyPlanningInventory,
    now_unix_ms: i64,
) -> CloudFrontApiResult<CloudFrontBehaviorFragment> {
    intent
        .behavior_id
        .validate()
        .map_err(|_| validation("invalid_cloudfront_behavior_id"))?;
    targets.binding.validate_at(now_unix_ms)?;
    if intent.generation == 0 {
        return Err(validation("invalid_cloudfront_behavior_generation"));
    }
    let target_observation = targets
        .targets
        .get(&intent.target)
        .cloned()
        .ok_or_else(|| validation("cloudfront_behavior_target_missing"))?;
    if now_unix_ms >= target_observation.valid_until_unix_ms {
        return Err(validation("stale_cloudfront_behavior_target"));
    }
    crate::validate_target_methods(
        &intent.target,
        &intent.allowed_methods,
        &intent.cached_methods,
    )?;
    let cache_policy = policies.reference(
        &targets.binding,
        CloudFrontPolicyKind::Cache,
        &intent.policies.cache_policy_id,
        now_unix_ms,
    )?;
    let origin_request_policy = intent
        .policies
        .origin_request_policy_id
        .as_ref()
        .map(|id| {
            policies.reference(
                &targets.binding,
                CloudFrontPolicyKind::OriginRequest,
                id,
                now_unix_ms,
            )
        })
        .transpose()?;
    let response_headers_policy = intent
        .policies
        .response_headers_policy_id
        .as_ref()
        .map(|id| {
            policies.reference(
                &targets.binding,
                CloudFrontPolicyKind::ResponseHeaders,
                id,
                now_unix_ms,
            )
        })
        .transpose()?;
    Ok(CloudFrontBehaviorFragment {
        binding: targets.binding.clone(),
        behavior_id: intent.behavior_id,
        generation: intent.generation,
        path_pattern: intent.path_pattern,
        target: intent.target,
        target_details: target_observation.details,
        viewer_protocol_policy: intent.viewer_protocol_policy,
        allowed_methods: intent.allowed_methods,
        cached_methods: intent.cached_methods,
        compress: intent.compress,
        policies: CloudFrontBehaviorPolicyRefs {
            cache_policy,
            origin_request_policy,
            response_headers_policy,
        },
        target_valid_until_unix_ms: target_observation.valid_until_unix_ms,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "source", content = "behavior", rename_all = "snake_case")]
pub enum CloudFrontEffectiveBehavior {
    Observed(Box<CloudFrontCacheBehaviorProjection>),
    Planned(Box<CloudFrontBehaviorFragment>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudFrontBehaviorDiagnosticKind {
    DefinitelyShadowed,
    PotentialOverlap,
    OpaqueObservedPattern,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontBehaviorDiagnostic {
    pub earlier_index: usize,
    pub later_index: usize,
    pub kind: CloudFrontBehaviorDiagnosticKind,
}

#[derive(Debug, Clone)]
pub struct CloudFrontAppendBehaviorPlanRequest {
    pub distribution_id: String,
    pub append: Vec<CloudFrontBehaviorFragment>,
    pub preview_requests: Vec<CloudFrontBehaviorPreviewRequest>,
    pub now_unix_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontAppendBehaviorPlan {
    binding: CloudFrontDistributionObservationBinding,
    default_behavior: CloudFrontCacheBehaviorProjection,
    effective_order: Vec<CloudFrontEffectiveBehavior>,
    diagnostics: Vec<CloudFrontBehaviorDiagnostic>,
    previews: Vec<CloudFrontBehaviorPreview>,
    default_fallback_may_change: bool,
    dispatch_authorized: bool,
}

pub fn build_append_only_behavior_plan(
    request: CloudFrontAppendBehaviorPlanRequest,
    inventory: &CloudFrontPlanningInventory,
    targets: &CloudFrontBehaviorTargetCatalog,
) -> CloudFrontApiResult<CloudFrontAppendBehaviorPlan> {
    let binding = CloudFrontDistributionObservationBinding::from_inventory(
        inventory,
        &request.distribution_id,
        request.now_unix_ms,
    )?;
    if binding != targets.binding {
        return Err(validation("cloudfront_behavior_plan_binding_mismatch"));
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
    if observed.detail.config.ordered_cache_behaviors.len() + request.append.len()
        > MAX_ORDERED_BEHAVIORS
    {
        return Err(validation("cloudfront_cache_behavior_quota"));
    }
    let mut effective_order = observed
        .detail
        .config
        .ordered_cache_behaviors
        .iter()
        .cloned()
        .map(|behavior| CloudFrontEffectiveBehavior::Observed(Box::new(behavior)))
        .collect::<Vec<_>>();
    let mut behavior_ids = BTreeSet::new();
    for fragment in &request.append {
        fragment.validate_at(request.now_unix_ms)?;
        if fragment.binding != binding || fragment.path_pattern.is_none() {
            return Err(validation("cloudfront_behavior_plan_binding_mismatch"));
        }
        if !behavior_ids.insert(fragment.behavior_id.clone()) {
            return Err(validation("duplicate_cloudfront_behavior_id"));
        }
    }
    effective_order.extend(
        request
            .append
            .iter()
            .cloned()
            .map(|fragment| CloudFrontEffectiveBehavior::Planned(Box::new(fragment))),
    );
    let diagnostics = analyze_behavior_order(&effective_order)?;
    let first_new = observed.detail.config.ordered_cache_behaviors.len();
    if diagnostics.iter().any(|diagnostic| {
        diagnostic.later_index >= first_new
            && matches!(
                diagnostic.kind,
                CloudFrontBehaviorDiagnosticKind::DefinitelyShadowed
                    | CloudFrontBehaviorDiagnosticKind::OpaqueObservedPattern
            )
    }) {
        return Err(validation("cloudfront_appended_behavior_impact_unproven"));
    }
    let previews = request
        .preview_requests
        .iter()
        .map(|preview| {
            preview_behavior_order(
                preview,
                &entry.summary.domain_name,
                &observed.detail.config.aliases,
                &effective_order,
                &observed.detail.config.default_cache_behavior,
                targets,
            )
        })
        .collect::<CloudFrontApiResult<Vec<_>>>()?;
    Ok(CloudFrontAppendBehaviorPlan {
        binding,
        default_behavior: observed.detail.config.default_cache_behavior.clone(),
        effective_order,
        diagnostics,
        previews,
        default_fallback_may_change: !request.append.is_empty(),
        dispatch_authorized: false,
    })
}

fn analyze_behavior_order(
    effective: &[CloudFrontEffectiveBehavior],
) -> CloudFrontApiResult<Vec<CloudFrontBehaviorDiagnostic>> {
    let patterns = effective.iter().map(effective_pattern).collect::<Vec<_>>();
    let mut diagnostics = Vec::new();
    for later in 0..patterns.len() {
        for earlier in 0..later {
            match (&patterns[earlier], &patterns[later]) {
                (Ok(left), Ok(right)) if left.definitely_shadows(right) => {
                    diagnostics.push(CloudFrontBehaviorDiagnostic {
                        earlier_index: earlier,
                        later_index: later,
                        kind: CloudFrontBehaviorDiagnosticKind::DefinitelyShadowed,
                    });
                }
                (Ok(left), Ok(right)) if left.overlaps(right) => {
                    diagnostics.push(CloudFrontBehaviorDiagnostic {
                        earlier_index: earlier,
                        later_index: later,
                        kind: CloudFrontBehaviorDiagnosticKind::PotentialOverlap,
                    });
                }
                (Err(_), _) => diagnostics.push(CloudFrontBehaviorDiagnostic {
                    earlier_index: earlier,
                    later_index: later,
                    kind: CloudFrontBehaviorDiagnosticKind::OpaqueObservedPattern,
                }),
                _ => {}
            }
        }
    }
    Ok(diagnostics)
}

fn effective_pattern(
    behavior: &CloudFrontEffectiveBehavior,
) -> CloudFrontApiResult<CloudFrontPathPattern> {
    match behavior {
        CloudFrontEffectiveBehavior::Observed(observed) => observed
            .path_pattern
            .as_ref()
            .ok_or_else(|| validation("missing_cloudfront_ordered_behavior_path"))
            .and_then(|path| CloudFrontPathPattern::new(path.clone())),
        CloudFrontEffectiveBehavior::Planned(planned) => planned
            .path_pattern
            .clone()
            .ok_or_else(|| validation("missing_cloudfront_ordered_behavior_path")),
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontBehaviorPreviewRequest {
    pub hostname: DomainName,
    pub raw_path: String,
    pub method: CloudFrontViewerMethod,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudFrontPreviewTarget {
    Origin {
        origin_id: CloudFrontOriginId,
        provider_failover: bool,
    },
    OriginGroup {
        group_id: crate::CloudFrontOriginGroupId,
        primary_origin_id: CloudFrontOriginId,
        secondary_origin_id: CloudFrontOriginId,
        failover_status_codes: BTreeSet<u16>,
        method_failover_eligible: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudFrontBehaviorPreview {
    pub hostname: DomainName,
    pub raw_path: String,
    pub normalized_path: String,
    pub normalized_path_changed: bool,
    pub matched_order_index: Option<usize>,
    pub matched_pattern: Option<String>,
    pub default_fallback: bool,
    pub method_allowed: bool,
    pub target: CloudFrontPreviewTarget,
    pub local_projection_only: bool,
}

fn preview_behavior_order(
    request: &CloudFrontBehaviorPreviewRequest,
    provider_domain: &str,
    aliases: &BTreeSet<String>,
    ordered: &[CloudFrontEffectiveBehavior],
    default: &CloudFrontCacheBehaviorProjection,
    targets: &CloudFrontBehaviorTargetCatalog,
) -> CloudFrontApiResult<CloudFrontBehaviorPreview> {
    let host_matches = DomainName::new(provider_domain.to_string())
        .is_ok_and(|domain| domain == request.hostname)
        || aliases.iter().any(|alias| {
            DomainName::new(alias.clone()).is_ok_and(|domain| domain == request.hostname)
        });
    if !host_matches {
        return Err(validation("cloudfront_preview_hostname_mismatch"));
    }
    let normalized_path = normalize_preview_path(&request.raw_path)?;
    for (index, behavior) in ordered.iter().enumerate() {
        let pattern = effective_pattern(behavior)?;
        if pattern.matches(&normalized_path) {
            return preview_result(
                request,
                &normalized_path,
                Some(index),
                Some(pattern.as_str().to_string()),
                behavior_target(behavior, targets)?,
                (
                    behavior_allowed_methods(behavior)?,
                    behavior_cached_methods(behavior)?,
                ),
                targets,
            );
        }
    }
    preview_result(
        request,
        &normalized_path,
        None,
        None,
        observed_target(default, targets)?,
        (
            parse_observed_methods(&default.allowed_methods)?,
            parse_observed_methods(&default.cached_methods)?,
        ),
        targets,
    )
}

fn preview_result(
    request: &CloudFrontBehaviorPreviewRequest,
    normalized_path: &str,
    index: Option<usize>,
    pattern: Option<String>,
    target_ref: CloudFrontOriginTargetRef,
    methods: (
        BTreeSet<CloudFrontViewerMethod>,
        BTreeSet<CloudFrontViewerMethod>,
    ),
    targets: &CloudFrontBehaviorTargetCatalog,
) -> CloudFrontApiResult<CloudFrontBehaviorPreview> {
    let (allowed_methods, cached_methods) = methods;
    let observation = targets
        .targets
        .get(&target_ref)
        .ok_or_else(|| validation("cloudfront_behavior_target_missing"))?;
    let target = match (&target_ref, &observation.details) {
        (CloudFrontOriginTargetRef::Origin(origin_id), CloudFrontBehaviorTargetDetails::Origin) => {
            CloudFrontPreviewTarget::Origin {
                origin_id: origin_id.clone(),
                provider_failover: false,
            }
        }
        (
            CloudFrontOriginTargetRef::OriginGroup(group_id),
            CloudFrontBehaviorTargetDetails::OriginGroup {
                primary_origin_id,
                secondary_origin_id,
                failover_status_codes,
            },
        ) => CloudFrontPreviewTarget::OriginGroup {
            group_id: group_id.clone(),
            primary_origin_id: primary_origin_id.clone(),
            secondary_origin_id: secondary_origin_id.clone(),
            failover_status_codes: failover_status_codes.clone(),
            method_failover_eligible: allowed_methods.contains(&request.method)
                && (matches!(
                    request.method,
                    CloudFrontViewerMethod::Get | CloudFrontViewerMethod::Head
                ) || (request.method == CloudFrontViewerMethod::Options
                    && cached_methods.contains(&CloudFrontViewerMethod::Options))),
        },
        _ => return Err(validation("cloudfront_behavior_target_kind_mismatch")),
    };
    Ok(CloudFrontBehaviorPreview {
        hostname: request.hostname.clone(),
        raw_path: request.raw_path.clone(),
        normalized_path: normalized_path.to_string(),
        normalized_path_changed: request.raw_path != normalized_path,
        matched_order_index: index,
        matched_pattern: pattern,
        default_fallback: index.is_none(),
        method_allowed: allowed_methods.contains(&request.method),
        target,
        local_projection_only: true,
    })
}

fn behavior_target(
    behavior: &CloudFrontEffectiveBehavior,
    targets: &CloudFrontBehaviorTargetCatalog,
) -> CloudFrontApiResult<CloudFrontOriginTargetRef> {
    match behavior {
        CloudFrontEffectiveBehavior::Observed(observed) => observed_target(observed, targets),
        CloudFrontEffectiveBehavior::Planned(planned) => Ok(planned.target.clone()),
    }
}

fn observed_target(
    behavior: &CloudFrontCacheBehaviorProjection,
    targets: &CloudFrontBehaviorTargetCatalog,
) -> CloudFrontApiResult<CloudFrontOriginTargetRef> {
    let matches = targets
        .targets
        .keys()
        .filter(|target| match target {
            CloudFrontOriginTargetRef::Origin(id) => id.as_str() == behavior.target_origin_id,
            CloudFrontOriginTargetRef::OriginGroup(id) => id.as_str() == behavior.target_origin_id,
        })
        .cloned()
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(validation("cloudfront_behavior_target_missing"));
    }
    Ok(matches[0].clone())
}

fn behavior_allowed_methods(
    behavior: &CloudFrontEffectiveBehavior,
) -> CloudFrontApiResult<BTreeSet<CloudFrontViewerMethod>> {
    match behavior {
        CloudFrontEffectiveBehavior::Observed(observed) => {
            parse_observed_methods(&observed.allowed_methods)
        }
        CloudFrontEffectiveBehavior::Planned(planned) => Ok(planned.allowed_methods.clone()),
    }
}

fn behavior_cached_methods(
    behavior: &CloudFrontEffectiveBehavior,
) -> CloudFrontApiResult<BTreeSet<CloudFrontViewerMethod>> {
    match behavior {
        CloudFrontEffectiveBehavior::Observed(observed) => {
            parse_observed_methods(&observed.cached_methods)
        }
        CloudFrontEffectiveBehavior::Planned(planned) => Ok(planned.cached_methods.clone()),
    }
}

fn parse_observed_methods(
    methods: &BTreeSet<String>,
) -> CloudFrontApiResult<BTreeSet<CloudFrontViewerMethod>> {
    methods
        .iter()
        .map(|method| match method.as_str() {
            "GET" => Ok(CloudFrontViewerMethod::Get),
            "HEAD" => Ok(CloudFrontViewerMethod::Head),
            "OPTIONS" => Ok(CloudFrontViewerMethod::Options),
            "POST" => Ok(CloudFrontViewerMethod::Post),
            "PUT" => Ok(CloudFrontViewerMethod::Put),
            "PATCH" => Ok(CloudFrontViewerMethod::Patch),
            "DELETE" => Ok(CloudFrontViewerMethod::Delete),
            _ => Err(validation("invalid_cloudfront_behavior_method")),
        })
        .collect()
}

fn normalize_preview_path(raw: &str) -> CloudFrontApiResult<String> {
    if raw.is_empty()
        || raw.len() > 8_192
        || !raw.starts_with('/')
        || !raw.is_ascii()
        || raw.contains('?')
        || raw.contains('#')
        || raw.chars().any(char::is_control)
    {
        return Err(validation("invalid_cloudfront_preview_path"));
    }
    let decoded = decode_unreserved(raw)?;
    let mut input = decoded.as_str();
    let mut output = String::with_capacity(decoded.len());
    while !input.is_empty() {
        if let Some(rest) = input.strip_prefix("../") {
            input = rest;
        } else if let Some(rest) = input.strip_prefix("./") {
            input = rest;
        } else if input.starts_with("/./") {
            input = &input[2..];
        } else if input == "/." {
            input = "/";
        } else if input.starts_with("/../") {
            input = &input[3..];
            remove_last_path_segment(&mut output);
        } else if input == "/.." {
            input = "/";
            remove_last_path_segment(&mut output);
        } else if matches!(input, "." | "..") {
            input = "";
        } else {
            let next_slash = if let Some(after_slash) = input.strip_prefix('/') {
                after_slash.find('/').map(|index| index + 1)
            } else {
                input.find('/')
            };
            let end = next_slash.unwrap_or(input.len());
            output.push_str(&input[..end]);
            input = &input[end..];
        }
    }
    Ok(output)
}

fn remove_last_path_segment(output: &mut String) {
    if let Some(index) = output.rfind('/') {
        output.truncate(index);
    } else {
        output.clear();
    }
}

fn decode_unreserved(raw: &str) -> CloudFrontApiResult<String> {
    let bytes = raw.as_bytes();
    let mut output = String::with_capacity(raw.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            output.push(char::from(bytes[index]));
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len()
            || !bytes[index + 1].is_ascii_hexdigit()
            || !bytes[index + 2].is_ascii_hexdigit()
        {
            return Err(validation("invalid_cloudfront_preview_percent_encoding"));
        }
        let decoded = (hex(bytes[index + 1])? << 4) | hex(bytes[index + 2])?;
        if decoded.is_ascii_alphanumeric() || matches!(decoded, b'-' | b'.' | b'_' | b'~') {
            output.push(char::from(decoded));
        } else {
            output.push('%');
            output.push(char::from(bytes[index + 1].to_ascii_uppercase()));
            output.push(char::from(bytes[index + 2].to_ascii_uppercase()));
        }
        index += 3;
    }
    Ok(output)
}

fn hex(value: u8) -> CloudFrontApiResult<u8> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(validation("invalid_cloudfront_preview_percent_encoding")),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::tests::{adapter, detail, summary, FakeApi, ACCOUNT_ID};
    use crate::{
        AwsPartition, CloudFrontDistributionPage, CloudFrontOriginGroupProjection,
        CloudFrontOriginProjection, CloudFrontTags,
    };

    fn inventory() -> CloudFrontPlanningInventory {
        let summary = summary();
        let api = Arc::new(FakeApi {
            account_id: ACCOUNT_ID.to_string(),
            partition: AwsPartition::Aws,
            pages: vec![CloudFrontDistributionPage {
                items: vec![summary.clone()],
                is_truncated: false,
                next_marker: None,
            }],
            detail: Some(detail(summary)),
            tags: CloudFrontTags::default(),
        });
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            adapter(api)
                .planning_inventory("behavior-observation", 1_000, 2_000)
                .await
                .unwrap()
        })
    }

    fn policies(inventory: &CloudFrontPlanningInventory) -> CloudFrontPolicyPlanningInventory {
        CloudFrontPolicyPlanningInventory::new(
            inventory.inventory().authority.clone(),
            vec![
                policy("cache-1", CloudFrontPolicyKind::Cache),
                policy("origin-request-1", CloudFrontPolicyKind::OriginRequest),
                policy("response-headers-1", CloudFrontPolicyKind::ResponseHeaders),
            ],
        )
        .unwrap()
    }

    fn policy(id: &str, kind: CloudFrontPolicyKind) -> CloudFrontPolicySummary {
        CloudFrontPolicySummary {
            id: id.to_string(),
            name: format!("policy-{id}"),
            kind,
            scope: CloudFrontPolicyScope::AwsManaged,
            etag: format!("etag-{id}"),
            last_modified_unix_seconds: 900,
        }
    }

    fn intent(path: &str, target: CloudFrontOriginTargetRef) -> CloudFrontBehaviorIntent {
        CloudFrontBehaviorIntent {
            behavior_id: CloudResourceId::new("behavior-api").unwrap(),
            generation: 1,
            path_pattern: Some(CloudFrontPathPattern::new(path).unwrap()),
            target,
            viewer_protocol_policy: CloudFrontViewerProtocolPolicy::RedirectToHttps,
            allowed_methods: BTreeSet::from([
                CloudFrontViewerMethod::Get,
                CloudFrontViewerMethod::Head,
            ]),
            cached_methods: BTreeSet::from([
                CloudFrontViewerMethod::Get,
                CloudFrontViewerMethod::Head,
            ]),
            compress: true,
            policies: CloudFrontBehaviorPolicyIntent {
                cache_policy_id: CloudFrontPolicyId::new("cache-1").unwrap(),
                origin_request_policy_id: Some(
                    CloudFrontPolicyId::new("origin-request-1").unwrap(),
                ),
                response_headers_policy_id: Some(
                    CloudFrontPolicyId::new("response-headers-1").unwrap(),
                ),
            },
        }
    }

    #[test]
    fn append_only_plan_previews_first_match_and_default_without_dispatch_authority() {
        let inventory = inventory();
        let targets =
            CloudFrontBehaviorTargetCatalog::from_inventory(&inventory, "E123EXAMPLE", 1_500)
                .unwrap();
        let fragment = build_behavior_fragment(
            intent(
                "/api/*",
                CloudFrontOriginTargetRef::Origin(CloudFrontOriginId::new("origin-1").unwrap()),
            ),
            &targets,
            &policies(&inventory),
            1_500,
        )
        .unwrap();
        let plan = build_append_only_behavior_plan(
            CloudFrontAppendBehaviorPlanRequest {
                distribution_id: "E123EXAMPLE".to_string(),
                append: vec![fragment],
                preview_requests: vec![
                    CloudFrontBehaviorPreviewRequest {
                        hostname: DomainName::new("d123example.cloudfront.net").unwrap(),
                        raw_path: "/api/users".to_string(),
                        method: CloudFrontViewerMethod::Get,
                    },
                    CloudFrontBehaviorPreviewRequest {
                        hostname: DomainName::new("d123example.cloudfront.net").unwrap(),
                        raw_path: "/other".to_string(),
                        method: CloudFrontViewerMethod::Get,
                    },
                ],
                now_unix_ms: 1_500,
            },
            &inventory,
            &targets,
        )
        .unwrap();
        assert_eq!(plan.effective_order.len(), 1);
        assert_eq!(plan.previews[0].matched_pattern.as_deref(), Some("/api/*"));
        assert!(!plan.previews[0].default_fallback);
        assert!(plan.previews[1].default_fallback);
        assert!(plan.default_fallback_may_change);
        assert!(!plan.dispatch_authorized);
        assert!(!serde_json::to_string(&plan)
            .unwrap()
            .contains("dispatchMethod"));
    }

    #[test]
    fn append_preserves_observed_order_and_rejects_stale_reuse_and_wrong_host_preview() {
        let mut inventory = inventory();
        let CloudFrontDetailObservation::Complete(observed) =
            &mut inventory.inventory_mut().entries[0].detail
        else {
            unreachable!();
        };
        let mut existing = observed.detail.config.default_cache_behavior.clone();
        existing.path_pattern = Some("/existing/*".to_string());
        observed
            .detail
            .config
            .ordered_cache_behaviors
            .push(existing.clone());

        let targets =
            CloudFrontBehaviorTargetCatalog::from_inventory(&inventory, "E123EXAMPLE", 1_500)
                .unwrap();
        let fragment = build_behavior_fragment(
            intent(
                "/new/*",
                CloudFrontOriginTargetRef::Origin(CloudFrontOriginId::new("origin-1").unwrap()),
            ),
            &targets,
            &policies(&inventory),
            1_500,
        )
        .unwrap();
        let plan = build_append_only_behavior_plan(
            CloudFrontAppendBehaviorPlanRequest {
                distribution_id: "E123EXAMPLE".to_string(),
                append: vec![fragment.clone()],
                preview_requests: vec![],
                now_unix_ms: 1_500,
            },
            &inventory,
            &targets,
        )
        .unwrap();
        assert!(matches!(
            &plan.effective_order[0],
            CloudFrontEffectiveBehavior::Observed(value) if value.as_ref() == &existing
        ));
        assert!(matches!(
            &plan.effective_order[1],
            CloudFrontEffectiveBehavior::Planned(value) if value.as_ref() == &fragment
        ));

        assert_eq!(
            build_append_only_behavior_plan(
                CloudFrontAppendBehaviorPlanRequest {
                    distribution_id: "E123EXAMPLE".to_string(),
                    append: vec![fragment.clone()],
                    preview_requests: vec![],
                    now_unix_ms: 2_000,
                },
                &inventory,
                &targets,
            )
            .unwrap_err()
            .code(),
            "stale_cloudfront_distribution_observation"
        );
        assert_eq!(
            build_append_only_behavior_plan(
                CloudFrontAppendBehaviorPlanRequest {
                    distribution_id: "E123EXAMPLE".to_string(),
                    append: vec![fragment],
                    preview_requests: vec![CloudFrontBehaviorPreviewRequest {
                        hostname: DomainName::new("unrelated.example.test").unwrap(),
                        raw_path: "/new/item".to_string(),
                        method: CloudFrontViewerMethod::Get,
                    }],
                    now_unix_ms: 1_500,
                },
                &inventory,
                &targets,
            )
            .unwrap_err()
            .code(),
            "cloudfront_preview_hostname_mismatch"
        );
    }

    #[test]
    fn path_subset_is_explicit_and_preview_normalizes_security_sensitive_paths() {
        for invalid in ["*", "/a/?", "/a/*/b", "/a/**", "/a/%2f", "/a/#x"] {
            assert!(CloudFrontPathPattern::new(invalid).is_err(), "{invalid}");
        }
        assert_eq!(
            CloudFrontPathPattern::new("api/*").unwrap().as_str(),
            "/api/*"
        );
        assert_eq!(
            normalize_preview_path("//api/./v1/../users").unwrap(),
            "//api/users"
        );
        assert_eq!(normalize_preview_path("/a//b").unwrap(), "/a//b");
        assert_eq!(normalize_preview_path("/a//").unwrap(), "/a//");
        assert_eq!(
            normalize_preview_path("/api/%2e%2e/admin").unwrap(),
            "/admin"
        );
        assert_eq!(
            normalize_preview_path("/api/%2fadmin").unwrap(),
            "/api/%2Fadmin"
        );
        for invalid in ["api", "/api?x=1", "/bad%2", "/bad\0"] {
            assert!(normalize_preview_path(invalid).is_err(), "{invalid:?}");
        }
        assert!(CloudFrontPathPattern::new("a".repeat(255)).is_err());
    }

    #[test]
    fn append_rejects_duplicate_shadow_and_opaque_observed_impact() {
        let inventory = inventory();
        let targets =
            CloudFrontBehaviorTargetCatalog::from_inventory(&inventory, "E123EXAMPLE", 1_500)
                .unwrap();
        let policy_inventory = policies(&inventory);
        let broad = build_behavior_fragment(
            intent(
                "/api/*",
                CloudFrontOriginTargetRef::Origin(CloudFrontOriginId::new("origin-1").unwrap()),
            ),
            &targets,
            &policy_inventory,
            1_500,
        )
        .unwrap();
        let mut narrow_intent = intent(
            "/api/admin/*",
            CloudFrontOriginTargetRef::Origin(CloudFrontOriginId::new("origin-1").unwrap()),
        );
        narrow_intent.behavior_id = CloudResourceId::new("behavior-admin").unwrap();
        let narrow =
            build_behavior_fragment(narrow_intent, &targets, &policy_inventory, 1_500).unwrap();
        let mut duplicate_id = narrow.clone();
        duplicate_id.behavior_id = broad.behavior_id.clone();
        assert_eq!(
            build_append_only_behavior_plan(
                CloudFrontAppendBehaviorPlanRequest {
                    distribution_id: "E123EXAMPLE".to_string(),
                    append: vec![broad.clone(), duplicate_id],
                    preview_requests: vec![],
                    now_unix_ms: 1_500,
                },
                &inventory,
                &targets,
            )
            .unwrap_err()
            .code(),
            "duplicate_cloudfront_behavior_id"
        );
        assert_eq!(
            build_append_only_behavior_plan(
                CloudFrontAppendBehaviorPlanRequest {
                    distribution_id: "E123EXAMPLE".to_string(),
                    append: vec![broad, narrow],
                    preview_requests: vec![],
                    now_unix_ms: 1_500,
                },
                &inventory,
                &targets,
            )
            .unwrap_err()
            .code(),
            "cloudfront_appended_behavior_impact_unproven"
        );
    }

    #[test]
    fn policy_kind_and_freshness_are_bound_to_the_distribution_authority() {
        let inventory = inventory();
        let targets =
            CloudFrontBehaviorTargetCatalog::from_inventory(&inventory, "E123EXAMPLE", 1_500)
                .unwrap();
        let wrong_kind = CloudFrontPolicyPlanningInventory::new(
            inventory.inventory().authority.clone(),
            vec![policy("cache-1", CloudFrontPolicyKind::OriginRequest)],
        )
        .unwrap();
        assert_eq!(
            build_behavior_fragment(
                intent(
                    "/api/*",
                    CloudFrontOriginTargetRef::Origin(CloudFrontOriginId::new("origin-1").unwrap()),
                ),
                &targets,
                &wrong_kind,
                1_500,
            )
            .unwrap_err()
            .code(),
            "cloudfront_policy_observation_missing"
        );
        assert_eq!(
            build_behavior_fragment(
                intent(
                    "/api/*",
                    CloudFrontOriginTargetRef::Origin(CloudFrontOriginId::new("origin-1").unwrap()),
                ),
                &targets,
                &policies(&inventory),
                2_000,
            )
            .unwrap_err()
            .code(),
            "invalid_cloudfront_distribution_binding"
        );
    }

    #[test]
    fn observed_group_target_rejects_writes_and_preview_is_conditional() {
        let mut inventory = inventory();
        let CloudFrontDetailObservation::Complete(observed) =
            &mut inventory.inventory_mut().entries[0].detail
        else {
            unreachable!();
        };
        let mut secondary: CloudFrontOriginProjection = observed.detail.config.origins[0].clone();
        secondary.id = "origin-2".to_string();
        observed.detail.config.origins.push(secondary);
        observed.detail.config.origin_groups = vec![CloudFrontOriginGroupProjection {
            id: "group-1".to_string(),
            primary_origin_id: "origin-1".to_string(),
            secondary_origin_id: "origin-2".to_string(),
            failover_status_codes: BTreeSet::from([503]),
            unsupported_features: BTreeSet::new(),
        }];
        observed
            .detail
            .config
            .default_cache_behavior
            .target_origin_id = "group-1".to_string();
        observed
            .detail
            .config
            .default_cache_behavior
            .allowed_methods
            .insert("OPTIONS".to_string());
        let targets =
            CloudFrontBehaviorTargetCatalog::from_inventory(&inventory, "E123EXAMPLE", 1_500)
                .unwrap();
        let preview = build_append_only_behavior_plan(
            CloudFrontAppendBehaviorPlanRequest {
                distribution_id: "E123EXAMPLE".to_string(),
                append: vec![],
                preview_requests: vec![CloudFrontBehaviorPreviewRequest {
                    hostname: DomainName::new("d123example.cloudfront.net").unwrap(),
                    raw_path: "/options".to_string(),
                    method: CloudFrontViewerMethod::Options,
                }],
                now_unix_ms: 1_500,
            },
            &inventory,
            &targets,
        )
        .unwrap();
        assert!(preview.previews[0].method_allowed);
        assert!(matches!(
            &preview.previews[0].target,
            CloudFrontPreviewTarget::OriginGroup {
                method_failover_eligible: false,
                ..
            }
        ));
        let mut write = intent(
            "/write/*",
            CloudFrontOriginTargetRef::OriginGroup(
                crate::CloudFrontOriginGroupId::new("group-1").unwrap(),
            ),
        );
        write.allowed_methods = BTreeSet::from([
            CloudFrontViewerMethod::Get,
            CloudFrontViewerMethod::Head,
            CloudFrontViewerMethod::Options,
            CloudFrontViewerMethod::Post,
            CloudFrontViewerMethod::Put,
            CloudFrontViewerMethod::Patch,
            CloudFrontViewerMethod::Delete,
        ]);
        write.cached_methods = BTreeSet::from([
            CloudFrontViewerMethod::Get,
            CloudFrontViewerMethod::Head,
            CloudFrontViewerMethod::Options,
        ]);
        assert_eq!(
            build_behavior_fragment(write, &targets, &policies(&inventory), 1_500)
                .unwrap_err()
                .code(),
            "cloudfront_origin_group_method_policy"
        );
    }
}
