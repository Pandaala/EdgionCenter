# Global outbound TLS capability alignment

Checkpoint: 2026-09-28, starting Center source d748c85. Current sibling source only;
no Edgion history was inspected and no Edgion source was changed in this pass.

## Confirmed Center defect and correction

The GatewayConfig editor offered hostname/SAN inputs and accepted them as valid.
Controller `handlers/san_guard.rs::check_egc_san` uses SAN_ENFORCED_EGC=false and
HOSTNAME_SNI_ENFORCED=false. A present hostname, including the empty string,
and a non-empty subjectAltNames list are rejected. An empty SAN list is allowed.

Center now mirrors those gates before submitting either editor mode. It retains
the original draft, including unsupported inputs, and lets the operator clear
them. Form help describes the restriction. The lossless adapter remains lossless;
mutation serialization is not a substitute for editor validation.

The previously valid test fixture itself contained both unsupported constraints.
It now uses a supported global validation block; dedicated tests retain the
unsupported round-trip evidence and assert rejection without draft mutation.
The editor submission test verifies neither constraint dispatches clusterUpdate,
then verifies a corrected document can be submitted normally.

## Global client identity: confirmed upstream gap

Controller `handlers/edgion_gateway_config.rs::parse` resolves outbound CA
material, but never resolves clientCertificateRef or populates its resolved
identity. A source search of Controller assignments found the identity resolvers
for Gateway, backend policies and plugin/provider paths, but no EGC resolver.
Gateway `config/edgion_gateway/conf_handler_impl.rs::recompute_global_config`
aggregates the reference and already-resolved identity; it does not load Secrets.
Common `link_sys_core/outbound_tls.rs::resolve_outbound_tls_with_defaults` inherits
the global identity when no local identity is configured and returns
OutboundTlsClientCertInvalid if the declared reference has no material.

There is no alternate lookup in this consumer chain. Center now explains that
current limitation beside the client reference without silently deleting it.
Repairing the Controller resolution/requeue path and verifying a real inherited
mTLS exchange remains open; a UI warning is not runtime completion. Existing
Gateway/BackendTLSPolicy identities have separate resolvers and are unaffected.

## Verification

29 tests across the GatewayConfig adapter/form and Gateway-family editor-submit
suites pass. Production TypeScript/Vite build and ESLint pass. Logs:
`/tmp/ws5-center-global-tls-constraints-{tests,build,lint}.log`.
No new native browser or TLS-handshake evidence is claimed.
