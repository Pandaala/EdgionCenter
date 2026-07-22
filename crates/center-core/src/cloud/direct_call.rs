//! Minimal direct-call outcome types shared by retained DNS integrations.

use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::{CoreError, CoreResult};

const MAX_IDEMPOTENCY_KEY_LEN: usize = 512;
const MAX_ERROR_CODE_LEN: usize = 128;
const MAX_ERROR_MESSAGE_LEN: usize = 4096;

/// Caller-supplied key for one synchronous provider request.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IdempotencyKey(String);

impl IdempotencyKey {
    pub fn new(value: impl Into<String>) -> CoreResult<Self> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_IDEMPOTENCY_KEY_LEN
            || value.trim() != value
            || value.chars().any(char::is_control)
        {
            return Err(CoreError::InvalidIdentifier {
                kind: "idempotency key",
                value,
            });
        }
        Ok(Self(value))
    }

    pub fn validate(&self) -> CoreResult<()> {
        Self::new(self.0.clone()).map(|_| ())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for IdempotencyKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationErrorKind {
    Transient,
    Throttled,
    ConflictRequiresReplan,
    Permanent,
    UnknownOutcome,
}

/// Sanitized outcome of a direct provider request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationError {
    pub kind: OperationErrorKind,
    pub code: String,
    pub message: String,
    pub retry_after_ms: Option<u64>,
}

impl OperationError {
    pub fn validate(&self) -> CoreResult<()> {
        validate_bounded_text(&self.code, "cloud operation error code", MAX_ERROR_CODE_LEN)?;
        validate_bounded_text(
            &self.message,
            "cloud operation error message",
            MAX_ERROR_MESSAGE_LEN,
        )?;
        if self
            .retry_after_ms
            .is_some_and(|delay| delay > i64::MAX as u64)
        {
            return Err(CoreError::Conflict(
                "cloud operation retry delay exceeds the persisted time range".to_string(),
            ));
        }
        Ok(())
    }
}

fn validate_bounded_text(value: &str, kind: &'static str, max_len: usize) -> CoreResult<()> {
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

    #[test]
    fn direct_call_types_preserve_the_retained_wire_shape() {
        let key = IdempotencyKey::new("zone-create-1").unwrap();
        assert_eq!(serde_json::to_string(&key).unwrap(), "\"zone-create-1\"");

        let error = OperationError {
            kind: OperationErrorKind::UnknownOutcome,
            code: "unknown_outcome".to_string(),
            message: "provider response was ambiguous".to_string(),
            retry_after_ms: None,
        };
        error.validate().unwrap();
        assert_eq!(
            serde_json::to_value(error).unwrap()["kind"],
            "unknown_outcome"
        );
    }
}
