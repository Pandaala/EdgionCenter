//! Provider-neutral, sanitized error classification.

use serde::{Deserialize, Serialize};

use super::{OperationError, OperationErrorKind};
use crate::{CoreError, CoreResult};

const MAX_ERROR_CODE_LEN: usize = 128;
const MAX_ERROR_MESSAGE_LEN: usize = 4096;
const MAX_PROVIDER_REQUEST_ID_LEN: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderErrorCategory {
    Authentication,
    Authorization,
    Quota,
    Conflict,
    Validation,
    NotFound,
    Transient,
    Throttled,
    UnknownOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(try_from = "NormalizedProviderErrorWire")]
pub struct NormalizedProviderError {
    category: ProviderErrorCategory,
    code: String,
    message: String,
    retry_after_ms: Option<u64>,
    provider_request_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NormalizedProviderErrorWire {
    category: ProviderErrorCategory,
    code: String,
    message: String,
    retry_after_ms: Option<u64>,
    provider_request_id: Option<String>,
}

impl TryFrom<NormalizedProviderErrorWire> for NormalizedProviderError {
    type Error = CoreError;

    fn try_from(value: NormalizedProviderErrorWire) -> Result<Self, Self::Error> {
        Self::new(
            value.category,
            value.code,
            value.message,
            value.retry_after_ms,
            value.provider_request_id,
        )
    }
}

impl NormalizedProviderError {
    /// Creates an error from already-sanitized adapter output. Provider bodies,
    /// headers, and credential material must never cross this boundary.
    pub fn new(
        category: ProviderErrorCategory,
        code: impl Into<String>,
        message: impl Into<String>,
        retry_after_ms: Option<u64>,
        provider_request_id: Option<String>,
    ) -> CoreResult<Self> {
        let value = Self {
            category,
            code: code.into(),
            message: message.into(),
            retry_after_ms,
            provider_request_id,
        };
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> CoreResult<()> {
        validate_text(&self.code, "provider error code", MAX_ERROR_CODE_LEN)?;
        validate_text(
            &self.message,
            "provider error message",
            MAX_ERROR_MESSAGE_LEN,
        )?;
        if let Some(request_id) = self.provider_request_id.as_deref() {
            validate_text(
                request_id,
                "provider request ID",
                MAX_PROVIDER_REQUEST_ID_LEN,
            )?;
        }
        if self
            .retry_after_ms
            .is_some_and(|delay| delay > i64::MAX as u64)
        {
            return Err(CoreError::Conflict(
                "provider retry delay exceeds the persisted time range".to_string(),
            ));
        }
        match self.category {
            ProviderErrorCategory::Throttled if self.retry_after_ms.is_none() => {
                return Err(CoreError::Conflict(
                    "throttled provider errors require a retry delay".to_string(),
                ));
            }
            ProviderErrorCategory::Authentication
            | ProviderErrorCategory::Authorization
            | ProviderErrorCategory::Conflict
            | ProviderErrorCategory::Validation
            | ProviderErrorCategory::NotFound
            | ProviderErrorCategory::UnknownOutcome
                if self.retry_after_ms.is_some() =>
            {
                return Err(CoreError::Conflict(
                    "terminal provider error category cannot carry a retry delay".to_string(),
                ));
            }
            _ => {}
        }
        Ok(())
    }

    pub fn category(&self) -> ProviderErrorCategory {
        self.category
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn retry_after_ms(&self) -> Option<u64> {
        self.retry_after_ms
    }

    pub fn provider_request_id(&self) -> Option<&str> {
        self.provider_request_id.as_deref()
    }

    pub fn operation_kind(&self) -> OperationErrorKind {
        match self.category {
            ProviderErrorCategory::Authentication
            | ProviderErrorCategory::Authorization
            | ProviderErrorCategory::Validation
            | ProviderErrorCategory::NotFound => OperationErrorKind::Permanent,
            ProviderErrorCategory::Quota => {
                if self.retry_after_ms.is_some() {
                    OperationErrorKind::Throttled
                } else {
                    OperationErrorKind::Permanent
                }
            }
            ProviderErrorCategory::Conflict => OperationErrorKind::ConflictRequiresReplan,
            ProviderErrorCategory::Transient => OperationErrorKind::Transient,
            ProviderErrorCategory::Throttled => OperationErrorKind::Throttled,
            ProviderErrorCategory::UnknownOutcome => OperationErrorKind::UnknownOutcome,
        }
    }

    pub fn into_operation_error(self) -> CoreResult<OperationError> {
        self.validate()?;
        Ok(OperationError {
            kind: self.operation_kind(),
            code: self.code,
            message: self.message,
            retry_after_ms: self.retry_after_ms,
        })
    }
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

    #[test]
    fn categories_map_to_durable_operation_outcomes() {
        let cases = [
            (
                ProviderErrorCategory::Authentication,
                None,
                OperationErrorKind::Permanent,
            ),
            (
                ProviderErrorCategory::Authorization,
                None,
                OperationErrorKind::Permanent,
            ),
            (
                ProviderErrorCategory::Quota,
                None,
                OperationErrorKind::Permanent,
            ),
            (
                ProviderErrorCategory::Quota,
                Some(5),
                OperationErrorKind::Throttled,
            ),
            (
                ProviderErrorCategory::Conflict,
                None,
                OperationErrorKind::ConflictRequiresReplan,
            ),
            (
                ProviderErrorCategory::Validation,
                None,
                OperationErrorKind::Permanent,
            ),
            (
                ProviderErrorCategory::NotFound,
                None,
                OperationErrorKind::Permanent,
            ),
            (
                ProviderErrorCategory::Transient,
                None,
                OperationErrorKind::Transient,
            ),
            (
                ProviderErrorCategory::Throttled,
                Some(5),
                OperationErrorKind::Throttled,
            ),
            (
                ProviderErrorCategory::UnknownOutcome,
                None,
                OperationErrorKind::UnknownOutcome,
            ),
        ];
        for (category, retry_after_ms, expected) in cases {
            let error = NormalizedProviderError::new(
                category,
                "StableCode",
                "Sanitized provider failure",
                retry_after_ms,
                Some("request-1".to_string()),
            )
            .unwrap();
            assert_eq!(error.operation_kind(), expected);
            assert_eq!(error.clone().into_operation_error().unwrap().kind, expected);
            let encoded = serde_json::to_string(&error).unwrap();
            assert_eq!(
                serde_json::from_str::<NormalizedProviderError>(&encoded).unwrap(),
                error
            );
        }
    }

    #[test]
    fn invalid_retry_and_text_shapes_fail_closed() {
        assert!(NormalizedProviderError::new(
            ProviderErrorCategory::Throttled,
            "RateLimited",
            "Retry later",
            None,
            None,
        )
        .is_err());
        assert!(NormalizedProviderError::new(
            ProviderErrorCategory::Authorization,
            "Denied",
            "Denied",
            Some(1),
            None,
        )
        .is_err());
        assert!(NormalizedProviderError::new(
            ProviderErrorCategory::Transient,
            "Bad\nCode",
            "Safe",
            None,
            None,
        )
        .is_err());
        assert!(NormalizedProviderError::new(
            ProviderErrorCategory::Transient,
            "Temporary",
            "x".repeat(MAX_ERROR_MESSAGE_LEN + 1),
            None,
            None,
        )
        .is_err());
        assert!(serde_json::from_str::<NormalizedProviderError>(
            r#"{"category":"throttled","code":"RateLimited","message":"Retry later","retryAfterMs":null,"providerRequestId":null}"#,
        )
        .is_err());
    }
}
