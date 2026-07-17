//! Stable cloud status, condition, and bounded event semantics.

use std::{collections::BTreeSet, fmt::Display};

use serde::{Deserialize, Serialize};

use super::{
    CloudCondition, CloudConditionStatus, CloudConditionType, CloudResourceId, CloudResourceKind,
    CloudResourceStatus, ProviderResourceRef,
};
use crate::{CoreError, CoreResult};

const MAX_REASON_LEN: usize = 128;
const MAX_MESSAGE_LEN: usize = 4096;
const MAX_CORRELATION_ID_LEN: usize = 128;
const MAX_PROVIDER_EXTERNAL_ID_LEN: usize = 2048;
const MAX_EVENT_HISTORY: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CloudCorrelationId(String);

impl CloudCorrelationId {
    pub fn new(value: impl Into<String>) -> CoreResult<Self> {
        let value = value.into();
        validate_text(&value, "cloud correlation ID", MAX_CORRELATION_ID_LEN)?;
        Ok(Self(value))
    }

    pub fn validate(&self) -> CoreResult<()> {
        Self::new(self.0.clone()).map(|_| ())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for CloudCorrelationId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudEvent {
    pub correlation_id: CloudCorrelationId,
    pub resource_kind: CloudResourceKind,
    pub resource_id: CloudResourceId,
    pub observed_generation: u64,
    pub condition_type: Option<CloudConditionType>,
    pub reason: String,
    pub message: String,
    pub occurred_at_unix_ms: i64,
}

impl CloudEvent {
    pub fn validate(&self) -> CoreResult<()> {
        self.correlation_id.validate()?;
        self.resource_id.validate()?;
        validate_reason(&self.reason)?;
        validate_text(&self.message, "cloud event message", MAX_MESSAGE_LEN)?;
        if self.observed_generation == 0 || self.occurred_at_unix_ms < 0 {
            return Err(CoreError::Conflict(
                "cloud event generation or time is invalid".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedCloudEventHistory {
    max_events: usize,
    events: Vec<CloudEvent>,
}

impl BoundedCloudEventHistory {
    pub fn new(max_events: usize) -> CoreResult<Self> {
        if max_events == 0 || max_events > MAX_EVENT_HISTORY {
            return Err(CoreError::Conflict(
                "cloud event history capacity is invalid".to_string(),
            ));
        }
        Ok(Self {
            max_events,
            events: Vec::new(),
        })
    }

    pub fn push(&mut self, event: CloudEvent) -> CoreResult<()> {
        event.validate()?;
        if self
            .events
            .last()
            .is_some_and(|last| event.occurred_at_unix_ms < last.occurred_at_unix_ms)
        {
            return Err(CoreError::Conflict(
                "cloud event history must be time ordered".to_string(),
            ));
        }
        if self.events.len() == self.max_events {
            self.events.remove(0);
        }
        self.events.push(event);
        Ok(())
    }

    pub fn events(&self) -> &[CloudEvent] {
        &self.events
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl CloudCondition {
    pub fn validate(&self) -> CoreResult<()> {
        validate_reason(&self.reason)?;
        validate_text(&self.message, "cloud condition message", MAX_MESSAGE_LEN)?;
        if self.observed_generation == 0 || self.last_transition_unix_ms < 0 {
            return Err(CoreError::Conflict(
                "cloud condition generation or transition time is invalid".to_string(),
            ));
        }
        Ok(())
    }

    pub fn is_true_and_fresh_for(&self, generation: u64) -> bool {
        self.status == CloudConditionStatus::True && self.observed_generation == generation
    }
}

impl ProviderResourceRef {
    pub fn validate(&self) -> CoreResult<()> {
        self.provider_account_id.validate()?;
        validate_text(
            &self.external_id,
            "provider external resource ID",
            MAX_PROVIDER_EXTERNAL_ID_LEN,
        )
    }
}

impl CloudResourceStatus {
    pub fn validate(&self, resource_generation: u64) -> CoreResult<()> {
        if resource_generation == 0
            || self
                .observed_generation
                .is_some_and(|generation| generation == 0 || generation > resource_generation)
        {
            return Err(CoreError::Conflict(
                "cloud resource status generation is invalid".to_string(),
            ));
        }
        if let Some(provider_resource) = self.provider_resource.as_ref() {
            provider_resource.validate()?;
        }
        let mut types = BTreeSet::new();
        for condition in &self.conditions {
            condition.validate()?;
            if condition.observed_generation > resource_generation
                || self
                    .observed_generation
                    .is_none_or(|observed| observed < condition.observed_generation)
                || !types.insert(condition.condition_type)
            {
                return Err(CoreError::Conflict(
                    "cloud resource conditions are inconsistent".to_string(),
                ));
            }
        }
        Ok(())
    }

    pub fn condition_is_true_and_fresh(
        &self,
        condition_type: CloudConditionType,
        generation: u64,
    ) -> bool {
        self.observed_generation == Some(generation)
            && self.conditions.iter().any(|condition| {
                condition.condition_type == condition_type
                    && condition.is_true_and_fresh_for(generation)
            })
    }

    pub fn set_condition(
        &mut self,
        mut condition: CloudCondition,
        resource_generation: u64,
    ) -> CoreResult<()> {
        condition.validate()?;
        if condition.observed_generation > resource_generation {
            return Err(CoreError::Conflict(
                "cloud condition observes a future generation".to_string(),
            ));
        }
        let condition_generation = condition.observed_generation;
        let mut updated = self.clone();
        if let Some(existing) = updated
            .conditions
            .iter_mut()
            .find(|existing| existing.condition_type == condition.condition_type)
        {
            if condition.observed_generation < existing.observed_generation {
                return Err(CoreError::Conflict(
                    "cloud condition observed generation moved backwards".to_string(),
                ));
            }
            if condition.last_transition_unix_ms < existing.last_transition_unix_ms {
                return Err(CoreError::Conflict(
                    "cloud condition transition time moved backwards".to_string(),
                ));
            }
            if condition.status == existing.status
                && condition.reason == existing.reason
                && condition.message == existing.message
            {
                condition.last_transition_unix_ms = existing.last_transition_unix_ms;
            }
            *existing = condition;
        } else {
            updated.conditions.push(condition);
        }
        updated.observed_generation = Some(
            updated
                .observed_generation
                .unwrap_or_default()
                .max(condition_generation),
        );
        updated.validate(resource_generation)?;
        *self = updated;
        Ok(())
    }
}

fn validate_reason(value: &str) -> CoreResult<()> {
    validate_text(value, "cloud status reason", MAX_REASON_LEN)?;
    if !value
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_uppercase())
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
    {
        return Err(CoreError::Conflict(
            "cloud status reason must be an UpperCamelCase code".to_string(),
        ));
    }
    Ok(())
}

fn validate_text(value: &str, kind: &'static str, max_len: usize) -> CoreResult<()> {
    if value.is_empty()
        || value.len() > max_len
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(CoreError::Conflict(format!("{kind} is invalid")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn condition(status: CloudConditionStatus, generation: u64, time: i64) -> CloudCondition {
        CloudCondition {
            condition_type: CloudConditionType::DnsReady,
            status,
            reason: "RecordsObserved".to_string(),
            message: "Authoritative records match".to_string(),
            observed_generation: generation,
            last_transition_unix_ms: time,
        }
    }

    fn event(sequence: u64, time: i64) -> CloudEvent {
        CloudEvent {
            correlation_id: CloudCorrelationId::new(format!("correlation-{sequence}")).unwrap(),
            resource_kind: CloudResourceKind::ManagedZone,
            resource_id: CloudResourceId::new("zone-a").unwrap(),
            observed_generation: sequence,
            condition_type: Some(CloudConditionType::DnsReady),
            reason: "RecordsObserved".to_string(),
            message: "Authoritative records match".to_string(),
            occurred_at_unix_ms: time,
        }
    }

    #[test]
    fn condition_freshness_requires_true_and_exact_generation() {
        let mut status = CloudResourceStatus::default();
        status
            .set_condition(condition(CloudConditionStatus::True, 2, 10), 2)
            .unwrap();
        assert!(status.condition_is_true_and_fresh(CloudConditionType::DnsReady, 2));
        assert!(!status.condition_is_true_and_fresh(CloudConditionType::DnsReady, 3));
        status
            .set_condition(condition(CloudConditionStatus::Unknown, 2, 11), 2)
            .unwrap();
        assert!(!status.condition_is_true_and_fresh(CloudConditionType::DnsReady, 2));
    }

    #[test]
    fn same_condition_preserves_transition_time_and_duplicates_fail_validation() {
        let mut status = CloudResourceStatus::default();
        status
            .set_condition(condition(CloudConditionStatus::True, 1, 10), 1)
            .unwrap();
        status
            .set_condition(condition(CloudConditionStatus::True, 2, 20), 2)
            .unwrap();
        assert_eq!(status.conditions[0].last_transition_unix_ms, 10);
        assert_eq!(status.conditions[0].observed_generation, 2);
        status.conditions.push(status.conditions[0].clone());
        assert!(status.validate(2).is_err());
        let before = status.clone();
        assert!(status
            .set_condition(condition(CloudConditionStatus::False, 2, 30), 2)
            .is_err());
        assert_eq!(status, before);
    }

    #[test]
    fn future_generation_bad_reason_and_backwards_time_fail_closed() {
        let mut status = CloudResourceStatus::default();
        status
            .set_condition(condition(CloudConditionStatus::True, 2, 10), 2)
            .unwrap();
        assert!(status
            .set_condition(condition(CloudConditionStatus::False, 3, 11), 2)
            .is_err());
        assert!(status
            .set_condition(condition(CloudConditionStatus::False, 2, 9), 2)
            .is_err());
        let mut invalid = condition(CloudConditionStatus::False, 2, 11);
        invalid.reason = "not_stable".to_string();
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn delayed_old_generation_cannot_overwrite_a_newer_condition() {
        let mut status = CloudResourceStatus::default();
        status
            .set_condition(condition(CloudConditionStatus::True, 5, 10), 5)
            .unwrap();
        let before = status.clone();
        assert!(status
            .set_condition(condition(CloudConditionStatus::False, 4, 20), 5)
            .is_err());
        assert_eq!(status, before);
    }

    #[test]
    fn event_history_is_bounded_ordered_and_roundtrips() {
        let mut history = BoundedCloudEventHistory::new(2).unwrap();
        history.push(event(1, 10)).unwrap();
        history.push(event(2, 20)).unwrap();
        history.push(event(3, 30)).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history.events()[0].observed_generation, 2);
        assert!(history.push(event(4, 29)).is_err());
        let encoded = serde_json::to_string(&history.events()[0]).unwrap();
        assert_eq!(
            serde_json::from_str::<CloudEvent>(&encoded).unwrap(),
            history.events()[0]
        );
        assert!(BoundedCloudEventHistory::new(0).is_err());
        assert!(BoundedCloudEventHistory::new(MAX_EVENT_HISTORY + 1).is_err());
    }
}
