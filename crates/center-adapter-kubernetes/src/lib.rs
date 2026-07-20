//! Kubernetes-native adapters for Edgion Center.
//!
//! This crate owns Kubernetes API types and client behavior. It may depend on
//! `center-core`, but shared runtime code must never depend on this crate.

mod cloud_capability_crd;
mod cloud_capability_store;
mod cloud_operation_crd;
mod cloud_operation_store;
mod controller_directory;
mod crd;
mod lease;
mod owner_locator;
mod provider_account_crd;
mod provider_account_store;
mod sar;
mod stdout_audit;

pub use cloud_capability_crd::{
    EdgionCapabilityAuthorityStatus, EdgionCapabilityScopeSpec, EdgionProviderCapabilitySnapshot,
    EdgionProviderCapabilitySnapshotSpec, EdgionProviderCapabilitySnapshotStatus,
};
pub use cloud_capability_store::{
    capability_snapshot_resource_name, KubernetesCapabilitySnapshotStore,
};
pub use cloud_operation_crd::{
    EdgionCloudOperation, EdgionCloudOperationSpec, EdgionCloudOperationStatus,
};
pub use cloud_operation_store::{cloud_operation_resource_name, KubernetesOperationStore};
pub use controller_directory::{controller_resource_name, KubernetesControllerDirectory};
pub use crd::{
    EdgionController, EdgionControllerPhase, EdgionControllerSpec, EdgionControllerStatus,
};
pub use lease::KubernetesLeaseCoordinator;
pub use owner_locator::KubernetesControllerOwnerLocator;
pub use provider_account_crd::{EdgionProviderAccount, EdgionProviderAccountSpec};
pub use provider_account_store::{provider_account_resource_name, KubernetesProviderAccountStore};
pub use sar::KubernetesSarAuthorizer;
pub use stdout_audit::StructuredStdoutAudit;
