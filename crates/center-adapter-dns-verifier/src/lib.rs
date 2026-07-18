//! Independent network DNS verification adapter.
//!
//! It has no dependency on provider SDKs, persistence, Admin API, federation,
//! or Edgion resources. Network targets are resolved to concrete socket
//! addresses and checked against a profile policy before every exchange.

mod dnssec;
mod transport;
mod verifier;

pub use dnssec::{
    LocalDnssecReason, LocalDnssecResolverConfiguration, LocalDnssecSecurity,
    LocalDnssecValidation, LocalDnssecValidator, LocalParentDsValidation, LocalRrsetValidation,
};
pub use transport::{
    is_public_unicast, DnsQueryTransport, DnsQuestion, DnsTargetPolicy, DnsTransportError,
    DnsTransportProtocol, DnsWireResponse, IpNetwork, TokioDnsQueryTransport,
};
pub use verifier::{
    DnsVerificationClock, DnsVerificationMetricResult, DnsVerificationMetrics,
    NetworkDnsPropagationVerifier, NoopDnsVerificationMetrics, ResolverProfile,
    SystemDnsVerificationClock,
};
