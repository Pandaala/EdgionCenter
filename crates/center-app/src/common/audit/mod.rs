//! Platform-neutral audit middleware integration.

pub mod middleware;

pub type AuditSink = std::sync::Arc<dyn edgion_center_core::AuditWriter>;
