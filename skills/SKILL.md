---
name: edgion-center-skills
description: Root navigation for the EdgionCenter knowledge base. Read this first, then drill into the relevant subtree.
---

# EdgionCenter Skills

> Federation hub (Center) for multi-cluster Edgion. A single `edgion-center` binary
> accepts bidirectional gRPC streams from one or more Controllers, aggregates the
> PluginMetaData they publish, persists a controller registry, and exposes an HTTP
> Admin API plus an optional embedded web dashboard.

## Navigation rules

1. **Progressive disclosure**: this file → category `SKILL.md` → specific files. Load only the smallest subtree the task needs.
2. **Three-layer locator**:
   - **Understand the architecture** (how Center is implemented) → `01-architecture/`
   - **Look up a feature / config** (how to run / configure) → `02-features/`
   - **Frontend (web dashboard)** → [`web/skills/SKILL.md`](../web/skills/SKILL.md)
3. **Shared knowledge lives upstream**: resource Schema, coding rules, the testing framework, and task workflows are defined in the Edgion repo, not duplicated here. See **External dependency** below.

## Quick locator

| What you want | Direct entry |
|---------------|--------------|
| **Center architecture** — fed-sync server, aggregator, watch cache, persistence | [01-architecture/06-center/SKILL.md](01-architecture/06-center/SKILL.md) |
| **src module → doc map** | [01-architecture/SKILL.md](01-architecture/SKILL.md) |
| **Config & ports / deploy / auth bootstrap / Admin API** | [02-features/SKILL.md](02-features/SKILL.md) |
| **Review findings** (federation / CPU-memory / h2-grpc) | [04-review/SKILL.md](04-review/SKILL.md) |
| **Testing** | [05-testing/SKILL.md](05-testing/SKILL.md) |
| **Misc / redirects** | [09-misc/SKILL.md](09-misc/SKILL.md) |
| **Frontend dashboard** | [web/skills/SKILL.md](../web/skills/SKILL.md) |

## Directory overview

| # | Directory | Purpose |
|---|-----------|---------|
| 01 | [architecture/](01-architecture/SKILL.md) | Center internals: fed-sync, aggregator, watch cache, persistence; src-module map |
| 02 | [features/](02-features/SKILL.md) | Config schema, ports, deployment, auth bootstrap, Admin API endpoints |
| 04 | [review/](04-review/SKILL.md) | Review findings, one file per finding |
| 05 | [testing/](05-testing/SKILL.md) | Center test guidance |
| 09 | [misc/](09-misc/SKILL.md) | Miscellaneous and moved-file redirects |

## External dependency — Edgion skills

Center reuses the Edgion `edgion-resources` schema crate; the corresponding knowledge is
**not duplicated here**. Read it upstream:

| Topic | Upstream location |
|-------|-------------------|
| Resource feature Schema (Route/Plugin/TLS/Backend/…) | https://github.com/Pandaala/Edgion/tree/main/skills/02-features/03-resources |
| Coding conventions (log IDs, log safety, observability) | https://github.com/Pandaala/Edgion/tree/main/skills/03-coding |
| Testing framework (unit / integration) | https://github.com/Pandaala/Edgion/tree/main/skills/05-testing |
| Task lifecycle & workflow templates | https://github.com/Pandaala/Edgion/tree/main/skills/07-tasks |
| Root knowledge base | https://github.com/Pandaala/Edgion/blob/main/skills/SKILL.md |
