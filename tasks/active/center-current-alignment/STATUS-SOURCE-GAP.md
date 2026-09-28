# Resolved: resource-list conditions observe native Controller runtime status

## Original confirmed behavior

The ReferenceGrant traffic run proves that the native FS Controller's raw
HTTPRoute list omits status while `/configserver/httproute?namespace=...&name=...`
has `status.parents[].conditions`, including grant denial/recovery. Center's
HTTPRoute row displays a dash. This is a live UI gap, not a speculative schema
mismatch. Evidence lives in `/tmp/ws5-center-grant-traffic-20260928/route-list.json`
and `route-list.png`; the processed endpoint was queried in `inspect.cjs`.

Relevant sources:

- `web/src/hooks/useResourceList.ts`: raw paginated resource retrieval.
- `web/src/api/resources.ts`: source resource APIs.
- `web/src/pages/Routes/HTTPRouteList.tsx`: renders only `record.status`.
- `web/src/components/resource/ResourceConditions.tsx`: shared condition rendering.
- Sibling `edgion-controller/src/api/configserver_handlers.rs`: existing processed
  get/list endpoints, separate from source CRUD.
- Sibling `edgion-controller/src/conf_mgr/conf_center/file_system/status.rs`:
  native status persistence separate from authored YAML.

## Required repair boundaries

1. Keep authored data and Controller runtime observations distinct. Preserve
   existing Kubernetes multi-writer status and do not replace editable spec with
   processed spec. Do not reintroduce runtime fields into mutation bodies.
2. Associate observations with the correct Controller, kind, namespace, name and
   version. An unavailable or stale observation must not become an apparent
   healthy result. Source and processed status freshness need explicit handling.
3. Keep reads bounded and authorized through the existing federation proxy.
   The processed bulk endpoint has no pagination; do not unconditionally fetch
   every resource merely to decorate one page. Permission or status-read failure
   must not silently erase readable source rows.
4. Reuse a shared mechanism across applicable resource lists. ReferenceGrant has
   no status and is not synchronized; never manufacture conditions for it.
5. Validate native grant denial/recovery in the actual route list, stale/missing
   observations, authorization failure, Controller switching, and mutation
   separation. Preserve the Kubernetes native status path.

## Implemented repair and evidence

Seventeen existing status columns now use `ResourceStatus`. Source status stays
primary, including Kubernetes multi-writer conditions. When absent, each visible
row reads its processed resource through the existing federation proxy, accepting
only the same Controller, kind, namespace, name and resourceVersion. Only status
enters the observation cache; processed spec never enters editors or source rows.
Four concurrent reads are allowed, with cancellation and 15-second polling.
Successful source refresh also refreshes observations when the source version is
unchanged. Read errors hide cached healthy conditions while preserving source
rows and actions. Missing versions and mismatched observations remain explicit.
Source list requests and caches are also bound to the captured Controller,
including callers that omit a custom scope. No status was invented for
ReferenceGrant or restricted dependency resources.

Validation:

- Full frontend suite: 690 tests in 104 files passed; the subsequently added
  Controller-switch test passed separately (one test in one file).
- TypeScript/Vite build, ESLint, E2E type checking and inventory passed. Inventory
  covers 22 kinds, 224 cases and 268 action/auxiliary selectors. Existing bundle
  size warnings remain.
- Six native browser checkpoints passed against the current dashboard and real
  Center federation: initial conditions, grant denial with HTTP 500, recovery
  with HTTP 200, failed status reads preserving rows, read recovery, and fixture
  restoration. The processed-read 403 was browser-injected; this is not evidence
  of real RBAC revocation. Grant denial/recovery changed the run-owned actual
  ReferenceGrant and was verified against actual Gateway traffic.

Artifacts: `/tmp/ws5-center-runtime-status-20260928/browser-result.json`,
`requests.json`, `denied.png`, `recovered.png`, and `browser-proof.cjs`.
Test logs: `/tmp/ws5-center-status-full-final.log`,
`/tmp/ws5-center-status-target.log`, `/tmp/ws5-center-status-build-scoped.log`,
and `/tmp/ws5-center-status-lint-scoped.log`.

This closes the observed native list display gap. It does not establish every
resource's runtime conditions or a deployed Kubernetes frontend upgrade. The
Kubernetes v5 image predates this change. The broader alignment goal stays active.
