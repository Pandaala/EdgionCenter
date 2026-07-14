use serde::{Deserialize, Serialize};

use crate::CoreResult;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEvent {
    pub occurred_at_unix_ms: i64,
    pub actor: String,
    pub provider: String,
    pub action: String,
    pub target_controller: Option<String>,
    pub outcome: String,
    pub source_ip: Option<String>,
    pub request_id: Option<String>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditFilter {
    pub actor: Option<String>,
    pub controller_id: Option<String>,
    pub since_unix_ms: Option<i64>,
    pub until_unix_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub limit: u32,
    pub offset: u64,
}

impl Page {
    pub fn bounded(limit: u32, offset: u64, maximum: u32) -> Self {
        Self {
            limit: limit.clamp(1, maximum.max(1)),
            offset,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditPage {
    pub items: Vec<AuditEvent>,
    pub next_offset: Option<u64>,
}

/// Best-effort, non-blocking event recording contract.
pub trait AuditWriter: Send + Sync {
    fn record(&self, event: AuditEvent);
}

#[async_trait::async_trait]
pub trait AuditReader: Send + Sync {
    async fn query(&self, filter: AuditFilter, page: Page) -> CoreResult<AuditPage>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_limits_are_never_zero_or_above_maximum() {
        assert_eq!(Page::bounded(0, 7, 100).limit, 1);
        assert_eq!(Page::bounded(1000, 7, 100).limit, 100);
    }
}
