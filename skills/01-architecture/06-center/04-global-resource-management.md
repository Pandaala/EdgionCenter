---
name: center-global-resource-management
description: Architecture boundary for federated GlobalResources inventory and durable GlobalResource synchronization across managed clusters.
---

# Global resource management

Center has two intentionally separate global-resource capabilities.

## GlobalResources inventory

`GlobalResources` is a virtual, non-durable view over cluster-owned namespaced resources in configured platform namespaces. The default namespaces are `edgion-system` and `edgion-global`; this convention does not change Kubernetes resource scope.

The live global kind is `EdgionConfigData`. `HTTPRoute`, `GRPCRoute`, `EdgionPlugins`, and
`ReferenceGrant` remain Controller-local resources: Center enters them through a selected
Controller instead of holding a cross-cluster inventory mirror.

The GlobalResources dashboard exposes `IpList`, `KeyList`, `Selector`, and `Misc` as direct
children, each backed by an exact `EdgionConfigData` type filter. `RegionRouteOverride`
remains an EdgionConfigData variant but is operated through the dedicated RegionRoute views.

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

The MVP uses the existing generic Controller Admin HTTP proxy. It adds no resource-specific Controller endpoint, ResourceKind, or federation protobuf message; the federation Command channel (apply/delete/reload) has been removed entirely, so this proxy is the only path. Generic multi-kind reverse-watch multiplexing is an optional later optimization; the current Controller keeps one active reverse-watch task.

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

## Namespace-scoped global configuration evolution

The initial five-kind catalog is being retired as a global inventory model. The configured
namespace set is the inclusion rule for global `EdgionConfigData` only. The target default
namespace set is `edgion-system` and `edgion-global`.

The policy remains Center-owned. Controller must not know that a namespace is "global".
Center owns the EdgionConfigData global namespace policy and type presentation, resolves the
authoritative Controller for a cluster, and maintains an in-memory real-time view through
bounded initial namespaced lists plus federated watch events for EdgionConfigData in each
configured namespace. There is no separate Controller inventory-catalog protocol. Controller
status reporting (identity, liveness, and resource counts) remains a separate compact
federation projection.

"Global" does not mean all Controller resources or arbitrary Kubernetes objects. `Secret` is
permanently absent. Resources such as routes, plugins, and grants retain Controller-local
navigation and authorization. A raw `ConfigMap` is excluded because its data can contain
credentials. Center must report a denied or unavailable ConfigData namespace observation rather
than treating it as an empty list.

An unsupported kind is reported as `not_supported`; a forbidden list/watch is `denied`; only
a successful empty list in a ready subscription is `empty`. Writes return only after their
Controller acknowledgement and corresponding watch convergence, never by optimistically
mutating Center's view. The first expanded-inventory release is inventory-only. Durable desired-state synchronization
continues to use its explicit safe-kind policy until per-kind create/update contracts,
normalization rules, ownership markers, and schema validation guarantees are reviewed.
The implementation ledger is
`tasks/pending/namespace-scoped-global-resources/03-subtasks.md`.
