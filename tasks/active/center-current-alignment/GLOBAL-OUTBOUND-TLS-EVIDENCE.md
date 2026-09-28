# Global outbound TLS capability alignment

Checkpoint: 2026-09-28, starting Center source d748c85. Current sibling source only;
no Edgion history was inspected. The initial Center-only correction is followed
by the uncommitted Controller repair recorded below.

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

## Global client identity: initial upstream gap

Controller `handlers/edgion_gateway_config.rs::parse` resolves outbound CA
material, but never resolves clientCertificateRef or populates its resolved
identity. A source search of Controller assignments found the identity resolvers
for Gateway, backend policies and plugin/provider paths, but no EGC resolver.
Gateway `config/edgion_gateway/conf_handler_impl.rs::recompute_global_config`
aggregates the reference and already-resolved identity; it does not load Secrets.
Common `link_sys_core/outbound_tls.rs::resolve_outbound_tls_with_defaults` inherits
the global identity when no local identity is configured and returns
OutboundTlsClientCertInvalid if the declared reference has no material.

There was no alternate lookup in this consumer chain. The initial Center change
explained the limitation without deleting the reference. The follow-up below
repairs resolution; real inherited mTLS verification remains open. Existing
Gateway/BackendTLSPolicy identities have separate resolvers and are unaffected.

## Verification

29 tests across the GatewayConfig adapter/form and Gateway-family editor-submit
suites pass. Production TypeScript/Vite build and ESLint pass. Logs:
`/tmp/ws5-center-global-tls-constraints-{tests,build,lint}.log`.
No new native browser or TLS-handshake evidence is claimed.

## Follow-up: Controller resolution repaired in the local worktree

`handlers/edgion_gateway_config.rs::parse` now clears both resolved TLS fields on
all generations and fetches a configured client identity with the shared
`fetch_secret` pipeline. The reference must name an explicit non-empty namespace
and the core Secret kind. The source-side namespace follows the existing global
CA convention (`default`), with cross-namespace ReferenceGrant checks unchanged.
No direct Kubernetes API request, extra queue, or broader RBAC was added.

The pipeline registers Secret and cross-namespace dependencies before checking
material/grant availability. Secret change/delete uses the existing generic
resource requeue path, and ReferenceGrant revalidation uses the same indexed
owner. Missing/deleted material or revoked permission cannot retain the prior
resolved identity. Changing/removing the reference and deleting EGC clear old
registrations. Runtime consumers still validate certificate/key material.

Two new Controller tests cover those transitions plus invalid kind/group and
missing namespace. All 15 EGC handler tests pass. These tests use synthetic Secret
payloads to prove resolution and dependency behavior, not a TLS handshake.
Focused rustfmt and the unit-test layout guard pass (953 mounted files, 9 crates).
Logs: `/tmp/ws5-global-client-identity-controller-tests.log` and
`/tmp/ws5-global-client-identity-layout.log`.

Center help now states the reference requirements and failure behavior instead
of claiming that resolution is absent. Its production build passes, recorded at
`/tmp/ws5-global-client-identity-center-build.log`. Existing Controllers need the
local repair deployed; this source change does not upgrade retained runtimes.
Edgion changes remain uncommitted under the user's repository-specific policy.
