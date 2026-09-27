---
name: dashboard-global-resources
description: Dashboard navigation and page boundaries for the read-only GlobalResources inventory served from the federation watch cache.
---

# GlobalResources dashboard

Read the canonical backend architecture first:
`../../../skills/01-architecture/06-center/04-global-resource-management.md`.

## Navigation

The inventory tree is:

```text
GlobalResources
├── IpList
├── KeyList
├── Selector
├── RequestAccessUrlAllowList
├── ProxyProtocolTrust
├── WafRuleBundle
├── WafPolicy
└── Misc
```

Every leaf is an `EdgionConfigData` inventory filtered by its exact `data.type`. Do not add
an intermediate EdgionConfigData menu node or repeat the type selection as content-area tabs.
Use the leaf type alone as the page title; do not prefix it with `EdgionConfigData`.
`RegionRouteOverride` is operated through the RegionRoute pages. Routes, plugins, and grants
remain available only in the selected Controller context.

## Page boundaries

- Inventory pages read Center aggregate APIs; they must not issue one browser request per Controller.
- The wire shape still carries a per-cluster `errors` array, but the watch-backed
  path always emits it empty; cluster trouble surfaces as `state`/`syncState`
  (for example `offline`, stale) rather than as error rows.
- Keep cluster state in the cluster filter and request failure handling; do not render a
  separate cluster-coverage summary card above every inventory table.
- Per-cluster editing reuses the existing lossless resource adapters and sends mutations through Center's Controller proxy.
- Every current ConfigData type except RegionRouteOverride has a direct inventory leaf.
- WafRuleBundle, RequestAccessUrlAllowList, and Misc payloads are hidden in the global
  view. The comparison table reports content comparison unavailable when any
  member has a hidden payload; the drawer explains how to inspect the resource
  through its Controller. ProxyProtocolTrust and WafPolicy contain only trust
  configuration and bundle references and pass through the global read model.
- Do not expose a separate desired-state and synchronization page. A future manual sync starts
  from one inventory row, compares that object across all clusters, then presents target
  selection, a fresh plan, and explicit apply confirmation in a modal.
- Do not create placeholder resource data while a backend task is pending.

## Implementation status

The read-only inventory (list + comparison drawer) is shipped and served entirely
from the federation watch read model. The durable desired-state / plan-and-apply
program (`tasks/pending/global-resource-management/`, GR-03 … GR-09) was rejected
on 2026-07-26 and must not be resumed from this file; any future write UI starts
from the convergence spec, not from those subtasks.
