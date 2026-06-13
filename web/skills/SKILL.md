---
name: edgion-center-dashboard-skills
description: Root navigation for the EdgionCenter web dashboard knowledge base. Read this first, then drill into the relevant subtree.
---

# EdgionCenter Dashboard Skills

> React 18 + TypeScript + Ant Design 5 + Vite 5 frontend for EdgionCenter.
> Talks to the Center Admin API (port 12201) and renders multi-cluster federation
> management plus the per-controller resource views proxied through Center.

## Navigation rules

1. **Progressive disclosure**: this file → category `SKILL.md` → specific files. Load only the smallest subtree the task needs.
2. **Three-layer locator**:
   - **Understand the architecture** (overall frontend design) → `01-architecture/`
   - **Component patterns** (how to build pages and editors) → `02-patterns/`
   - **Resource guides** (per-resource page notes) → `03-resources/`
3. **New resource page**: read `02-patterns/` (code patterns) and `03-resources/` (page notes) together; resource Schema is authoritative upstream (see External dependency).

## Quick locator

| What you want | Direct entry |
|---------------|--------------|
| **Frontend architecture** — structure, data flow, API layer | [01-architecture/SKILL.md](01-architecture/SKILL.md) |
| **List-page pattern** | [02-patterns/01-list-page.md](02-patterns/01-list-page.md) |
| **Editor-modal pattern** | [02-patterns/02-editor-modal.md](02-patterns/02-editor-modal.md) |
| **Types & utils pattern** | [02-patterns/03-types-and-utils.md](02-patterns/03-types-and-utils.md) |
| **i18n rules** | [02-patterns/04-i18n-rules.md](02-patterns/04-i18n-rules.md) |
| **API layer** | [01-architecture/02-api-layer.md](01-architecture/02-api-layer.md) |
| **Resource page guides** | [03-resources/SKILL.md](03-resources/SKILL.md) |
| **Center federation UI** — RegionRoute page, conflict detection, failover | [04-center/01-region-route-page.md](04-center/01-region-route-page.md) |
| **Testing** | [04-testing/SKILL.md](04-testing/SKILL.md) |

## Directory overview

| # | Directory | Purpose |
|---|-----------|---------|
| 01 | [architecture/](01-architecture/SKILL.md) | Frontend structure, data flow, state, API layer, routing |
| 02 | [patterns/](02-patterns/SKILL.md) | Dev patterns: list page, editor modal, types & utils, i18n |
| 03 | [resources/](03-resources/SKILL.md) | Per-resource page notes (Schema is upstream) |
| 04 | [testing/](04-testing/SKILL.md) | Backend bring-up, test data, dev verification |
| 04 | [center/](04-center/01-region-route-page.md) | Center federation: RegionRoute page, conflict detection, failover/sync controls |

## External dependency — resource Schema (Edgion)

Resource Schema (HTTPRoute/GRPCRoute/EdgionPlugins/EdgionTls/…) is **authoritative upstream**;
the frontend keeps page-development notes only:
https://github.com/Pandaala/Edgion/tree/main/skills/02-features/03-resources
