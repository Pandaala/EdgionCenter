---
name: dashboard-global-resources
description: Dashboard navigation and page boundaries for GlobalResources inventory and row-scoped synchronization.
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
└── Misc
```

Every leaf is an `EdgionConfigData` inventory filtered by its exact `data.type`. Do not add
an intermediate EdgionConfigData menu node or repeat the type selection as content-area tabs.
Use the leaf type alone as the page title; do not prefix it with `EdgionConfigData`.
`RegionRouteOverride` is operated through the RegionRoute pages. Routes, plugins, and grants
remain available only in the selected Controller context.

## Page boundaries

- Inventory pages read Center aggregate APIs; they must not issue one browser request per Controller.
- The API returns per-cluster errors. Denied, unavailable, ambiguous, and malformed results are not empty lists.
- Keep cluster state in the cluster filter and request failure handling; do not render a
  separate cluster-coverage summary card above every inventory table.
- Per-cluster editing reuses the existing lossless resource adapters and sends mutations through Center's Controller proxy.
- The four visible `EdgionConfigData` filters are `IpList`, `KeyList`, `Selector`, and `Misc`.
- Do not expose a separate desired-state and synchronization page. A future manual sync starts
  from one inventory row, compares that object across all clusters, then presents target
  selection, a fresh plan, and explicit apply confirmation in a modal.
- Do not create placeholder resource data while a backend task is pending.

## Implementation order

Follow `tasks/pending/global-resource-management/03-subtasks.md`. GR-04 begins only after the aggregate API in GR-03 is complete; the synchronization UI begins only after GR-07.
