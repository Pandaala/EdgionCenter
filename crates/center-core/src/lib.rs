//! Platform-neutral domain types and capability ports for EdgionCenter.
//!
//! This crate must remain independent of HTTP/gRPC frameworks and platform
//! adapters such as SQLx and Kube.

mod admin;
mod audit;
mod authz;
mod capabilities;
mod controller;
mod coordination;
mod error;

pub use admin::{CreateRole, CreateUser, RoleAdmin, RoleRecord, UpdateUser, UserAdmin, UserRecord};
pub use audit::{AuditEvent, AuditFilter, AuditPage, AuditReader, AuditWriter, Page};
pub use authz::{
    Action, ActionOperation, AllowAllAuthorizer, Authorizer, AuthzMode, Decision, Principal,
};
pub use capabilities::{CenterCapabilities, CenterMode};
pub use controller::{
    ControllerDirectory, ControllerId, ControllerOwnerLocator, ControllerOwnerRoute,
    ControllerPhase, ControllerRecord, ControllerRegistration, ControllerRuntimeObservation,
    EvictionOutcome, EvictionResult, EvictionTarget, OfflineOutcome, OwnershipFence, SessionId,
};
pub use coordination::{CoordinationRole, Coordinator, Leadership, ReleaseOutcome, RenewalOutcome};
pub use error::{CoreError, CoreResult};
