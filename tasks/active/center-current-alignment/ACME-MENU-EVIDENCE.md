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

## Remaining work

The list's separate Lifecycle column still reads source `status.phase` only;
it does not share the processed-status fallback used by Conditions. Certificate
expiry is not displayed despite the old resource guide claiming otherwise.
Continue these display fields against current EdgionAcmeStatus, preserving
source/runtime separation and acknowledging that FS Pending is not a running
issuance service. Real issuance/renewal needs a Kubernetes Controller leader and
an isolated ACME test CA; this native denial proof does not establish either.
