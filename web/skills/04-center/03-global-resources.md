---
name: dashboard-global-resources
description: Dashboard navigation and page boundaries for GlobalResources inventory and GlobalResource synchronization.
---

# GlobalResources dashboard

Read the canonical backend architecture first:
`../../../skills/01-architecture/06-center/04-global-resource-management.md`.

## Navigation

The inventory tree is:

```text
GlobalResources
├── HTTPRoute
├── GRPCRoute
├── EdgionPlugins
├── EdgionConfigData
│   ├── KeyList
│   ├── IpList
│   ├── Selector
│   ├── RegionRouteOverride
│   └── Misc
└── ReferenceGrant
```

The sidebar currently supports only section, group, and leaf. Implement recursive menu nodes before adding the EdgionConfigData subtree. Recursive permission filtering must drop empty ancestors, active state must propagate to ancestors, and collapsed mode must preserve accessible labels.

## Page boundaries

- Inventory pages read Center aggregate APIs; they must not issue one browser request per Controller.
- The API returns per-cluster errors. Denied, unavailable, ambiguous, and malformed results are not empty lists.
- Per-cluster editing reuses the existing lossless resource adapters and sends mutations through Center's Controller proxy.
- `EdgionConfigData` filtering uses exact current `data.type` values.
- The separate GlobalResource desired-state and synchronization UI must not be mixed with non-durable inventory.
- Do not create placeholder resource data while a backend task is pending.

## Implementation order

Follow `tasks/pending/global-resource-management/03-subtasks.md`. GR-04 begins only after the aggregate API in GR-03 is complete; the synchronization UI begins only after GR-07.
