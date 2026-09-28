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

## Remaining work

Real issuance/renewal needs a Kubernetes Controller leader and an isolated ACME
test CA. Native validation, denial and display proofs do not establish issuance,
Secret publication, renewal or Gateway certificate hot reload. Continue those
runtime checks without adding DNS-01 or expanding the supported product scope.
