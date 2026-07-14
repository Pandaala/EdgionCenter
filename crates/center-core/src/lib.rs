//! Platform-neutral domain types and capability ports for EdgionCenter.
//!
//! This crate must remain independent of HTTP/gRPC frameworks and platform
//! adapters such as SQLx and Kube.

mod audit;
mod authz;
mod capabilities;
mod controller;
mod coordination;
mod error;

pub use audit::{AuditEvent, AuditFilter, AuditPage, AuditReader, AuditWriter, Page};
pub use authz::{Action, Authorizer, AuthzMode, Decision, Principal};
pub use capabilities::{CenterCapabilities, CenterMode};
pub use controller::{
    ControllerDirectory, ControllerId, ControllerPhase, ControllerRecord, ControllerRegistration,
    EvictionOutcome, SessionId,
};
pub use coordination::{CoordinationRole, Coordinator, Leadership};
pub use error::{CoreError, CoreResult};
