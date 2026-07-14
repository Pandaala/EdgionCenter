use std::error::Error;
use std::fmt::{Display, Formatter};

/// Errors shared across capability ports without exposing adapter libraries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    InvalidIdentifier { kind: &'static str, value: String },
    Conflict(String),
    NotFound(String),
    Unsupported(&'static str),
    Adapter(String),
}

impl Display for CoreError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidIdentifier { kind, value } => {
                write!(formatter, "invalid {kind} identifier: {value:?}")
            }
            Self::Conflict(message) => write!(formatter, "conflict: {message}"),
            Self::NotFound(message) => write!(formatter, "not found: {message}"),
            Self::Unsupported(capability) => {
                write!(formatter, "unsupported capability: {capability}")
            }
            Self::Adapter(message) => write!(formatter, "adapter error: {message}"),
        }
    }
}

impl Error for CoreError {}

pub type CoreResult<T> = Result<T, CoreError>;
