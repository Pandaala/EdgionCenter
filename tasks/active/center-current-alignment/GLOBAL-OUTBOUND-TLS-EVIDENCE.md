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

## Native inherited mTLS: success and a revocation regression

Both current native binaries build successfully. Shared common outbound-TLS tests
pass (19 tests). Logs: `/tmp/ws5-global-client-identity-common-tests.log`,
`/tmp/ws5-global-client-identity-controller-build.log`, and
`/tmp/ws5-global-client-identity-gateway-build.log`.

The isolated harness is `/tmp/ws5-global-client-identity-runtime-20260928/run.cjs`.
It uses private certificate material and dedicated ports, a fresh Controller and
Gateway, a TLS server requiring verified client certificates, and an HTTP backend.
LinkSys Webhook declares enabled TLS but no local CA or client identity. The
EGC supplies both; a ReferenceGrant permits its cross-namespace client Secret.
WebhookKeyGet feeds KeyAuth so a missing result is observable as request failure.

Authoritative run: `run-1790566102298`, log `run-host-fixed.log` in that directory's
parent. Its `result.json` is **failed**, not an overall pass:

- Initial inherited mTLS succeeded; the HTTPS server observed `client-first`.
- Updating the Secret through the Controller API changed the observed peer to
  `client-rotated` without restarting either process.
- Deleting the ReferenceGrant did not stop subsequent authenticated calls within
  the 45-second observation budget. Gateway logged OutboundTlsClientCertInvalid
  while preparing the replacement, but the old Webhook client remained published.
- Grant restoration and Secret deletion/recreation steps were not reached. They
  have no new native evidence from this run.

Current source explains the failure: EGC publication invokes
`LinkSysStore::rebuild_clients()` with forced=true; the Webhook arm in
`link_sys/runtime/store.rs::apply_entry` logs preparation failure and preserves
its old same-kind runtime. Existing tests and review guidance explicitly preserve
clients on ordinary failed updates. Global TLS authorization changes need a
separate invalidation decision; do not silently change ordinary last-good update
semantics. Caller-held in-flight handles have their own documented lifetime.
Persistent providers have a different runner and need their own scope assessment.

Harness corrections before the authoritative run: required integration-mode env
opt-in; current Kind_namespace_name filesystem names; explicit Host through
node:http (node:fetch probes reached the IP-host route and returned 404); and
KeyAuth to distinguish WebhookKeyGet's intentional missing-value continuation.
Earlier run logs remain as failed harness evidence. They are not product TLS
failures. Every run cleaned up only its owned Controller/Gateway and local servers;
private fixtures and diagnostics remain for review. The conf_sync channel used
the existing local skip-TLS test profile; the tested outbound Webhook used real
CA verification and mandatory client certificates.

## Forced Webhook TLS invalidation repaired and verified

The local Gateway change removes and stops the published Webhook entry before a
forced global TLS rebuild. A replacement that is not ready or fails preparation
therefore leaves no current client carrying the revoked material. Ordinary
same-kind failed updates retain their existing last-good behavior. Caller-held
in-flight handles retain their documented lifetime; this does not promise
instant cancellation of already-started requests.

All 33 LinkSys store tests pass, including the new preparation-failure/not-ready,
ordinary-update preservation, forced invalidation and recovery regression.
The Gateway binary builds successfully. Logs:
`/tmp/ws5-global-client-revocation-gateway-tests.log` and
`/tmp/ws5-global-client-revocation-gateway-build.log`.

Fresh native run `run-1790575124261` passes all seven harness checks. Its
`result.json` records verified `client-first` and `client-rotated` peers and
`localWebhookIdentityConfigured: false`. Both ReferenceGrant revocation and
Secret deletion converge to HTTP 401; grant restoration and Secret recreation
restore HTTP 200 with the expected peer identity. Log:
`/tmp/ws5-global-client-identity-runtime-20260928/run-revocation-fixed.log`.
The prior failed run remains regression evidence. The harness exited normally
and cleaned up its own processes. These checks use direct Controller mutations;
they do not add Center browser or federation transport coverage.

The Controller and Gateway repairs remain uncommitted in Edgion. Persistent
LinkSys providers use a separate lifecycle runner; their forced TLS invalidation
behavior is still pending assessment and is not covered by this Webhook result.
