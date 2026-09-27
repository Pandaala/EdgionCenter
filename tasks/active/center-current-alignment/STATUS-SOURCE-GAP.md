# Open: resource-list conditions omit native Controller runtime status

## Confirmed current behavior

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

This is the next implementation task within the authorized alignment goal.
The ReferenceGrant editor/traffic proof does not close this display gap.
