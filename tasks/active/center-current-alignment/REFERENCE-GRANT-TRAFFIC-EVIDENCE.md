# ReferenceGrant form and cross-namespace traffic

Checkpoint: 2026-09-28. The overall alignment goal remains active.

The current native Center/Vite forwards to Controller `e2e-a/alignment-a`.
Its retained Gateway on 18200 serves a new HTTPRoute in `center-region-flow`,
with Host `grant-alignment.example.com`. The backend Service and EndpointSlice
are in `center-grant-target`, targeting the synthetic west backend on 18202.
A ReferenceGrant in the target namespace permits only HTTPRoute references
from the source namespace and only the named Service. All four added resources
are named `center-grant-flow` and labeled `grant-traffic-20260928`.

Five checks pass:

1. The initial grant names `wrong-target`. Processed HTTPRoute status is
   `ResolvedRefs=False/RefNotPermitted`, and actual Gateway traffic returns 500.
2. The Center grant form changes `to.name` to `center-grant-flow`. A successful
   PUT advances resourceVersion, processed references resolve and traffic is 200.
3. The form changes `from.namespace` to `wrong-source`. Status returns to
   `RefNotPermitted` and actual traffic returns 500.
4. Restoring `center-region-flow` resolves references and restores 200 with body
   `region-west`. Each form edit has exact field and resourceVersion readback.
5. Exact original Controller configuration bytes are restored and reloaded.
   Access discovery confirms the temporary ReferenceGrant update permission is
   absent; the retained valid cross-namespace route remains healthy.

Only ReferenceGrant `update` was added temporarily to concrete default Controller
rules. No wildcard or Secret permission was introduced. Existing RegionRoute
resources and the second Controller remained untouched. No Gateway restart was
needed for grant edits: Controller revalidation updated the denied backend ref.

Artifacts: `/tmp/ws5-center-grant-traffic-20260928/`, including fixtures,
`browser-proof.cjs`, passing `browser-result.json`, `recovered.png`, and private
configuration snapshots. This establishes HTTP backend grant name/source matching
and revocation by form edit, not every grant consumer or deletion path.

## Status display finding

A separate inspection of the actual HTTPRoute list found a Center gap. The FS
Controller's raw list returns this route without `status` or `spec.currentStatus`;
the processed `/configserver/httproute` response contains the correct conditions.
Center's list reads raw resources and passes only `record.status` to
`ResourceConditions`, so the row displays a dash even when runtime status is known.
`route-list.json` and `route-list.png` record the live row; `inspect.cjs` captures
the processed response. The passing grant flow checks processed status directly
and must not be presented as passing list-status UI evidence.

See [STATUS-SOURCE-GAP.md](STATUS-SOURCE-GAP.md) for the next scoped repair.
No production source change was made in this grant verification pass.
