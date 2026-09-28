# ACME menu: trigger boundary and remaining lifecycle work

## Current Controller contract

The sibling Controller's `src/api/authz_classifier.rs` classifies manual issuance
as `AcmeTrigger` on synthetic `Service`, exposed as access operation
`acme.trigger`; ordinary EdgionAcme update permission is not sufficient.
`src/api/mod.rs::trigger_acme` acknowledges queue admission, not certificate
issuance. A missing service returns 503. The background service starts only in
the Kubernetes leader lifecycle; FS resource validation is not issuance.

## Repaired Center behavior

`AcmeTriggerButton` uses the shared permission-aware control with `acme.trigger`
and captures the page Controller for dispatch. `resourceApi.triggerAcme` bypasses
the mutable global proxy target and suppresses unsanitized interceptor messages.
A pending dispatch disables repeat clicks. Success explicitly says the check was
queued and issuance remains unconfirmed. Denial, unavailability and rejection
have separate messages; uncertain transport/server failures never report success
or automatically retry. The server remains the authorization authority.

## Native evidence

Artifacts: `/tmp/ws5-center-acme-trigger-20260928/` (`browser-proof.cjs`,
`result.json`, `denied-button.png`, `server-denial.png`). Four checkpoints passed:

1. Actual default Controller policy disables the trigger while displaying the row.
2. Browser-injected stale allowed access makes the button clickable, but the real
   Controller returns 403 and the UI reports sanitized denial. No server policy
   was changed to simulate stale authorization.
3. Direct admin dispatch against the actual FS Controller returns 503 because
   its issuance service is not running.
4. Removing the browser interception restores the disabled button. No CA was
   contacted. The owned `center-region-flow/center-acme-trigger-check` resource
   remains, labelled `alignment-run: acme-trigger-20260928`.

Focused API/component checks passed 16 tests, including pinned dispatch, double
click suppression, unsuccessful envelopes, denial/unavailability, and ambiguous
transport without replay. Build, lint and the unchanged 268-selector inventory
passed. Full frontend results are recorded in CURRENT-COVERAGE.md.

## Lifecycle and expiry display repair

`AcmeLifecycle` now renders source phase and certificateNotAfter, falling back
through `ResourceStatus` to an identity/version-matched processed observation.
Conditions and lifecycle share the same query rather than issuing independent
requests. A failed observation hides both cached phase and expiry. Missing or
invalid timestamps remain absent, while valid timestamps have explicit UTC
presentation and an ISO `time` attribute. Pending includes a service-availability
caveat; it does not imply the background issuer is running. The table scrolls
horizontally to preserve readable dates and access to actions on narrow screens.

Five lifecycle component tests cover source precedence, shared processed reads,
UTC conversion, missing/invalid fields, version mismatch and failed refresh.
Existing ResourceStatus tests also pass. After the presentation adjustment, the
five lifecycle tests, build and lint passed again. Logs use the prefix
`/tmp/ws5-center-acme-lifecycle-` (focused, layout, build-final and lint-final).

Four browser checkpoints passed against current Vite/Center:
actual native Pending, injected Ready/expiry rendering, injected read denial
clearing phase and expiry, and recovery to actual native Pending. The expiry
and denial responses are browser fixtures, not certificate issuance evidence.
Artifacts: `/tmp/ws5-center-acme-lifecycle-20260928/` (`browser-proof.cjs`,
`result.json`, `native-pending.png`, `injected-expiry.png`). A stale text locator
after the line-break adjustment failed once; it was corrected to inspect the
lifecycle cell, and the complete scenario passed. No resources or policies changed.

## Real Kubernetes issuance and Gateway certificate loading

Artifacts: `/tmp/ws5-center-acme-issuance-20260928/`. The owned namespace is
`ws5-center-acme-20260928`, Controller identity is `e2e-acme/alignment-acme`, and
resource/GatewayClass name is `center-acme-issuance`. This uses the retained
current-source Linux Controller artifact copied into an isolated container,
not the old shared-cluster Controller. Copied/source binary SHA-256 is
`b5defc40eef33a9768d014a7b3a816a51faf39f132f89f540cc0c9371c1069fd`.
The native Gateway uses the existing current-source debug artifact.

Center remains native on 12201 with the current Vite dashboard on 15173. The
Controller connects to it over mTLS and watches only the owned namespace, with
namespaced ServiceAccount permissions and limited cluster resource reads/status
writes. Its Center policy grants the concrete ACME resource and trigger operations,
without Secret reads or wildcard access. The Gateway's conf-sync test transport
is loopback plaintext; this run does not add conf-sync TLS evidence.

Pebble runs in a dedicated Docker network with real challenge validation
(`PEBBLE_VA_ALWAYS_VALID=0`). Its image ID is
`sha256:ddf230642b1a584f519f32e347de1b05a6e4c1f6c35c1863b33effeab5f78199`.
The isolated Controller trusts only the fixture CA via Linux SSL_CERT_FILE;
macOS system trust was not changed. The test CA contacts Gateway HTTP port 18300
for `acme-alignment.test`; certificates are served on HTTPS 18301. Configs,
credentials and private keys remain under /tmp and are not committed.

The resource was created through Center's federation proxy. Pebble logs prove
actual HTTP-01 validation and certificate issuance. Four subsequent checks pass
in `browser-result.json` / `browser-proof.cjs`:

1. Center's actual Ready, serial and expiry match the public certificate fetched
   from the owned Secret by the external test harness. No private key was read.
2. Gateway HTTPS presents the same certificate fingerprint, without restarting
   Gateway after Secret publication. The TLS fingerprint observation disables
   public trust validation and is not a public-PKI verification claim.
3. The actual Center menu displays Ready and the exact expiry with an enabled
   scoped trigger. `issued.png` was visually inspected.
4. The real menu trigger returns 200 and says only that the check was queued.

First certificate serial is `CE4A33CA15AC8C`, expiry
`2026-12-27T00:20:19.000Z`. Kubernetes Secret publication and initial Gateway
certificate hot loading are now established. This does not prove a replacement
certificate or renewal.

Retained handles: Docker `ws5-center-acme-controller`, `ws5-center-acme-pebble`,
network `ws5-center-acme-20260928`; native Gateway session 63587. Controller Admin
15923 and conf-sync 51003 bind host loopback. ServiceAccount token expires after
8 hours; revalidate runtime/auth before further work. Bootstrap initially exposed
missing read permissions for namespace metadata and the legacy config alias;
these were corrected only in the owned RBAC definitions. No existing CRDs or
unrelated workloads were changed.

## Remaining work

[ACME-SCHEDULER-CLEAR-GAP.md](ACME-SCHEDULER-CLEAR-GAP.md) records the upstream
422 failure clearing the scheduler checkpoint after successful publication.
Resolve that contract before claiming renewal completion. Continue certificate
replacement, renewal and recovery checks; this pass changed no production code
and does not rerun unchanged unit/build gates.
