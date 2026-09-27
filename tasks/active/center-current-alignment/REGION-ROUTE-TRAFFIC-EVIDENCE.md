# RegionRoute menu to actual Gateway traffic

Checkpoint: 2026-09-28. Overall alignment remains active.

## Failure found and repaired

The initial browser flow appeared to pass east → west → east. Access logs showed
that restoration actually reported `overlay_invalid` and silently used the base
configuration. Center wrote `failoverTo: ""`; current Edgion interprets every
present string as a region name, so the empty target invalidates the overlay.
Center's predicate also treated that string as cleared and could report an
idempotent `converged` without correcting it.

A stronger fixture makes base behavior different: the base east region fails
over to west, while the valid overlay leaves east local. The old native Center
then reproducibly reports `converged` for clear while actual traffic reaches
west. `/tmp/ws5-center-region-traffic-20260928/before-fix.json` records this.
The first apparent pass is retained as `initial-weak-oracle-result.json`, not
counted as passing proof of restoration.

The repair removes the optional target when the request clears failover and
requires an absent/null target for the clear predicate. Existing invalid empty
strings now trigger a repair write. The same live flow exposed cached
`spec.currentStatus` being persisted as configuration, producing an unknown-field
condition. The shared ConfigData writer now excludes exactly top-level `status`
and `spec.currentStatus`; CAS resourceVersion and nested operator data remain.

## Passing live proof

Current Vite source on 15173 uses the retained native standalone Center on 12201.
An isolated FS Controller `e2e-a/alignment-a` connects over existing test mTLS to
12251, serves Admin on 15921 and conf-sync on 51001. Its freshly started Gateway
listens on 18200; synthetic east and west HTTP backends use the host private
interface on 18201 and 18202. This proof does not alter the Kubernetes Center or
its restricted namespace-test Controllers.

Eight owned resources form the path: GatewayConfig, GatewayClass, Gateway,
Service, EndpointSlice, EdgionPlugins, RegionRouteOverride ConfigData and
HTTPRoute. Names and namespace are `center-region-flow`; labels identify
`region-traffic-20260928`. The base plugin retains routing logic; only its overlay
is writable through the default Center policy.

After rebuilding/restarting the native Center, the stronger browser proof passes:

1. Clearing the pre-existing empty target performs a new CAS write; the target
   field disappears, cached status is absent from persisted spec, and traffic
   goes to east rather than the base configuration's west.
2. The RegionRoute page selects west. Center reports one `converged` outcome,
   Controller resourceVersion advances, and actual traffic goes to west.
3. The page selects No failover. The optional target is absent, the watch outcome
   is `converged`, and actual traffic returns to east using the valid overlay.

The base plugin spec stays identical through both menu operations. Final three
access logs show ports 18201, 18202 and 18201 with no `overlay_invalid` message.
Both backend counters advance. The inspected screenshot shows the consistent
row and restored region tags. A further exact-body plugin PUT through Center
returns 403 and leaves the resource identical: no Controller RBAC expansion
was needed. The final overlay has no failover and the Controller config still
omits explicit Center RBAC, using the default policy.

## Artifacts and setup limits

`/tmp/ws5-center-region-traffic-20260928/` retains proof scripts, fixtures,
`browser-result.json`, `verified.json`, `recovered.png`, backend counters,
access logs and private runtime configuration. Credentials remain outside Git.
Controller session 77944, Gateway session 79699, backend session 73054 and
rebuilt native Center session 9013 were live at this checkpoint; revalidate
before reuse. Original Center session 47666 was stopped before replacement.

Initial setup failed because the isolated work directory lacked CRD schemas;
current schemas were copied locally. Initial fixture filenames were not valid
FS storage names, so resources were created through Admin API and the ignored
files removed. Gateway started before those resources and required one normal
restart to acquire its listener. These are setup corrections, not product fixes;
no Gateway restart occurred during the final menu transitions.

This first run establishes single-Controller actual traffic. The subsequent
two-Controller run below extends it to fan-out and partial failure. Kubernetes
deployment of the repair remains separate; the existing v5 image does not
include this repair.


## Validation

Three new Rust regressions cover clear semantics, repair of existing invalid
empty targets, and cached-status exclusion with CAS/operator-data preservation.
The app's focused full library run passes 286 tests. The subsequent backend
matrix passes 876 workspace tests and 247 app tests without default features
(1,123 total), plus formatting, Clippy, dependency isolation and offline
Kubernetes manifest rendering. It exits 1 at the pre-existing English-only
violation in `fix-issue-workflow-generic.zh.md`; that unrelated file is unchanged.
The later no-legacy guard passes separately, as does diff whitespace validation.

The matrix used `EDGION_SKIP_WEB=1` for this backend repair to preserve the live
Vite installation and avoid reinstalling unchanged frontend dependencies.
E2E TypeScript, inventory (22 kinds, 224 cases, 266 actions) and lint pass
separately after the added browser assertions. Frontend component code is
unchanged; its retained 679-test baseline was not rerun. The single-Controller
browser/traffic proof above runs against the rebuilt native backend.

Logs: `app-tests.log`, `build.log`, `matrix.log`, `e2e-types.log`,
`e2e-inventory.log`, and `lint.log` in the artifact directory above.


## Two Controllers: fan-out, partial failure and source synchronization

The follow-up adds a separate FS Controller `e2e-b/alignment-b`, with its own
configuration directory, mTLS client identity, Admin port 15922 and conf-sync
51002. Its independent Gateway listens on 18210. Both Gateways use the same
synthetic east/west backends; separate access logs and destination ports identify
each data-plane path. Only run-owned fixtures were copied, using canonical FS
filenames and excluding computed status. Both base plugins route east to west;
both valid overlays keep east local, retaining the stronger fallback oracle.

Five initial assertions pass: both Gateways initially reach east; one menu
failover reports two `converged` outcomes and moves both Gateways to west; one
menu clear reports two `converged` outcomes and returns both to east. Every
Controller's resourceVersion advances. Cleared targets and persisted
`spec.currentStatus` are absent, and both base plugin documents stay identical.

A second run temporarily removes only ConfigData `update` from Controller B's
otherwise equivalent default policy, preserving its read/watch behavior:

1. Menu failover returns HTTP 207 with one converged and one failed outcome.
   The open editor retains both results after the row becomes inconsistent,
   and its apply action is disabled. Gateway A reaches west; Gateway B stays east.
2. Restoring B's exact original config re-enables its default update permission.
   The page synchronizes A's overlay data to B. The result reports B converged,
   both actual Gateways reach west, and the row permits failover edits again.
3. A finally step clears failover on both and verifies both return east. B's
   configuration bytes match the pre-test file exactly; no temporary policy
   remains. B's seven logged Gateway requests contain no invalid-overlay fallback.

The first partial-run attempt incorrectly expected HTTP 200. The API correctly
returned 207, so the script failed that assertion and successfully restored both
policy and traffic in its finally block. The corrected run passed all three
partial/recovery checkpoints. That first attempt is not a product defect.

Artifacts: `/tmp/ws5-center-region-dual-20260928/`, including passing
`browser-result.json`, `partial-result.json`, inspected `recovered.png` and
`partial.png`, scripts, access logs and private configuration. Retained Controller
B session is 41313 and Gateway B session is 2207; revalidate before reuse.
Controller A, Gateway A, native Center and backend handles remain as above.
No production source change or permission expansion was needed in this follow-up.
The backend/frontend gate results above therefore remain the applicable baseline.

This proves two-Controller actual traffic and a real authorization-induced mixed
outcome followed by menu synchronization. It does not establish simultaneous
operator races, every terminal outcome, or deployment of the repair into the
Kubernetes image. The overall resource/menu audit remains active.
