---
name: center-global-resource-management
description: Architecture boundary for federated GlobalResources inventory and durable GlobalResource synchronization across managed clusters.
---

# Global resource management

Center has two intentionally separate global-resource capabilities.

## GlobalResources inventory

`GlobalResources` is a virtual, non-durable view over cluster-owned namespaced resources in configured platform namespaces. The default namespaces are `edgion-system` and `edgion-global`; this convention does not change Kubernetes resource scope.

The initial kinds are:

- `HTTPRoute`
- `GRPCRoute`
- `EdgionPlugins`
- `EdgionConfigData`
- `ReferenceGrant`

`EdgionConfigData` is filtered by the current `ConfigEntry` variants: `KeyList`, `IpList`, `Selector`, `RegionRouteOverride`, and `Misc`.

Center resolves exactly one eligible Controller per cluster, fans out bounded list/get requests through the existing federation HTTP proxy, and returns per-cluster observations and errors. It does not persist observed resource payloads or merge same-named cluster objects into one mutable object.

## GlobalResource desired state

`GlobalResource` is a durable Center-owned template, target selector, desired revision, and synchronization policy. Standalone persists it through the SQL adapter; Kubernetes mode uses a dedicated Center CRD and coordination boundary. Observed target payloads remain non-durable.

GR-06 is deliberately plan-only: the Admin API can create, list, read, replace, and plan desired intent, but cannot apply, adopt, prune, or delete. Replacement uses a strong `If-Match` generation. A later apply must carry the fresh Controller `metadata.resourceVersion` in the full PUT body because that is the Controller's current CAS contract.

Synchronization follows plan then apply:

1. Resolve the target cluster to exactly one eligible Controller.
2. Fresh-read the target object.
3. Normalize cluster-owned metadata and status for diff.
4. Produce a per-target plan.
5. Apply only the confirmed desired revision and target set.
6. Create when absent or update with the target's current `resourceVersion`.
7. Record each target result independently.

The first release does not adopt unowned objects, automatically retry ambiguous outcomes, or delete/prune.

## Responsibility boundary

Center owns orchestration, targeting, aggregation, drift, desired-state persistence, operation records, Center authorization, and audit.

Controller remains authoritative for federation identity authorization, resource CRUD, optimistic concurrency, schema validation, reference resolution, status, ConfCenter persistence, requeue, and Gateway synchronization. Gateway remains the runtime enforcement boundary.

The MVP uses the existing generic Controller Admin HTTP proxy. It adds no resource-specific Controller endpoint, ResourceKind, or federation protobuf message. The current protobuf apply/delete commands are not usable because the Controller handles only reload. Generic multi-kind reverse-watch multiplexing is an optional later optimization; the current Controller keeps one active reverse-watch task.

## Failure rules

- Zero or multiple eligible Controllers for a cluster: report unavailable/ambiguous and do not write.
- Missing Controller permission: report denied, never empty or synchronized.
- Platform namespace outside configuration: reject.
- Namespace excluded by Controller `watch_namespaces`: report unavailable.
- Existing object without matching Center ownership: conflict, do not overwrite.
- Stale resource version: conflict, do not retry without a precondition.
- Ambiguous transport outcome: record unknown outcome, do not replay automatically.
- `Secret`: never supported by inventory or desired-state synchronization.

Drift is a fresh comparison against the persisted desired revision, never a durable observed-object cache. Apply does not retry unknown writes; the operator must re-plan and confirm. Any future continuous reconciler must preserve all existing fences and use bounded exponential backoff.

## Authorization

Controller's built-in Center policy permits reads for the initial non-Secret kinds but permits writes only for `EdgionConfigData`. Synchronizing `HTTPRoute`, `GRPCRoute`, `EdgionPlugins`, or `ReferenceGrant` requires explicit concrete create/update grants in Controller configuration.

## Task handoff

The ordered implementation ledger is `tasks/pending/global-resource-management/03-subtasks.md`. Read the task's `01-design.md` and open issues before implementing any numbered task.

## Namespace-scoped inventory evolution

The initial five-kind catalog is a bootstrap inventory only. The next capability makes
the configured namespace set the inclusion rule: a GlobalResources inventory scans every
Controller-supported, inventory-safe namespaced kind in the Center-configured namespaces.
The target default namespace set is `edgion-system` and `edgion-global`.

The policy remains Center-owned. Controller must not know that a namespace is "global".
Center owns a reviewed, versioned descriptor set (kind, collection path, and display
category), resolves the authoritative Controller for a cluster, and uses the existing
authenticated namespaced `list`/`watch` paths for every descriptor in each configured
namespace. There is no separate Controller inventory-catalog protocol.

"All resources" means all inventory-safe Controller resource kinds, not arbitrary
Kubernetes objects. `Secret` is permanently absent from the Center descriptor set. Resources that may
contain resolved credentials or private key material are also excluded until they have an
explicit safe projection. A raw `ConfigMap` is not inventory-safe by default because its
data can contain credentials. Controller authorization remains concrete per kind; Center
must report a denied or unavailable kind/namespace observation rather than treating it as
an empty list.

An unsupported kind is reported as `not_supported`; a forbidden list/watch is `denied`; only
a successful empty list is `empty`. The first expanded-inventory release is inventory-only. Durable desired-state synchronization
continues to use its explicit safe-kind policy until per-kind create/update contracts,
normalization rules, ownership markers, and schema validation guarantees are reviewed.
The implementation ledger is
`tasks/pending/namespace-scoped-global-resources/03-subtasks.md`.
