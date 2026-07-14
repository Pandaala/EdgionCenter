//! Kubernetes-native adapters for Edgion Center.
//!
//! This crate owns Kubernetes API types and client behavior. It may depend on
//! `center-core`, but shared runtime code must never depend on this crate.

mod controller_directory;
mod crd;
mod lease;
mod owner_locator;
mod sar;
mod stdout_audit;

pub use controller_directory::{controller_resource_name, KubernetesControllerDirectory};
pub use crd::{
    EdgionController, EdgionControllerPhase, EdgionControllerSpec, EdgionControllerStatus,
};
pub use lease::KubernetesLeaseCoordinator;
pub use owner_locator::KubernetesControllerOwnerLocator;
pub use sar::KubernetesSarAuthorizer;
pub use stdout_audit::StructuredStdoutAudit;
