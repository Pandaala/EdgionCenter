---
name: center-fleet-observability
description: Multi-Controller resource inventory, consistency, condition health, and topology behavior.
---

# Fleet observability

## Federation diagnostics

The Center-only `/federation-diagnostics` page consumes the read-only
`/api/v1/center/admin/watch-status` and `/api/v1/center/admin/metadata-store` endpoints. It shows
watch ownership/sync versions plus effective RegionRoute and GlobalConnectionIpRestriction key
coverage. The navigation entry requires `server:read` and is available in both Center deployment
modes; it does not depend on SQL persistence or Kubernetes-native RBAC.

## Data boundaries

The Center controller list reports connection and aggregate-count metadata. Detailed
fleet health is read through each online Controller's existing HTTP proxy. The web
loads every first-class catalog resource with the Controller id encoded in the proxy
path and `_skipControllerProxy` set, so comparison never depends on the currently
selected Controller.

Failures are partial: one denied or old resource endpoint is recorded as unavailable
for that Controller/kind and must not hide successful snapshots. Secret and ConfigMap
contents are not loaded by fleet observability.

## Consistency semantics

The comparison identity is `kind/namespace/name` (`_cluster` for cluster scope). A row
is consistent only when every online Controller has the resource and its operator
document is equal. Status, resourceVersion, uid, generation, managed fields, creation
timestamps, and catalog-declared runtime fields do not participate in the fingerprint.
Conditions remain visible beside the configuration result.

Fleet summaries include counts by kind, cluster, and Controller; missing/divergent
resources; rejected and unresolved conditions; explicit conflict diagnostics; and
ACME certificates expiring within 30 days.

## Topology semantics

The selected-Controller topology loads all first-class relationship resources and
metadata-only keys for restricted dependencies. It displays these main chains:

```text
GatewayClass -> Gateway -> Route -> Service -> EndpointSlice -> Backend address
Route -> EdgionPlugins / EdgionStreamPlugins -> ConfigData / LinkSys / Secret
Gateway -> EdgionTls / EdgionAcme -> Secret
Service -> BackendTLSPolicy / EdgionBackendTrafficPolicy
```

Reference-like fields are resolved using their declared kind and namespace. A missing
target becomes a red unresolved placeholder instead of silently dropping the edge.
Rejected/conflicting conditions are projected onto nodes and edges. Namespace filters
retain connected cluster-scoped parents and cross-namespace dependencies.

## Conditions

Every catalog entry with `hasConditions` uses `ResourceConditions` in its list and
read-only detail. The component collects resource, parent, ancestor, and listener
condition locations and displays type, status, reason, message, observed generation,
transition time, and context. Pages must never synthesize an `Active` status.
