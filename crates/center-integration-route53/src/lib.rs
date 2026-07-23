//! Route 53-specific Admin API composition.
//!
//! Provider accounts, local cursor-key authority, AWS SDK clients, and raw provider failures stay
//! behind this boundary. The first slice is read-only, ambient-credential-only, and default-off.

mod dns_admin;
mod dns_admin_service;
mod dns_write_service;
mod zone_lifecycle_service;

pub use dns_admin_service::{compose_dns_admin, Route53DnsReadConfig};
pub use dns_write_service::{compose_dns_write_admin, Route53DnsWriteConfig};
pub use zone_lifecycle_service::{compose_zone_lifecycle_admin, Route53ZoneLifecycleConfig};
