# Center Skills System Skeleton — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a coherent, navigable skills knowledge base for EdgionCenter — repo-root entry (`AGENTS.md`), backend `skills/` skeleton with category indexes, and a de-staled English frontend `web/skills/` tree — so future center development has a solid foundation.

**Architecture:** Two skill trees (backend `skills/`, frontend `web/skills/`) joined by a single repo-root entry. Center-specific knowledge is authored locally; shared knowledge is referenced via remote GitHub URLs to `Pandaala/Edgion`, never copied or linked by local path.

**Tech Stack:** Markdown skill files (`SKILL.md` = directory entry, `*.md` = reference, `*.skill` = workflow). No code/compilation. "Tests" in this plan are verification commands (`grep`, `ls`, link checks).

**Conventions (apply to every file written):**
- All content **English**.
- External (Edgion) references use remote URLs: dirs → `https://github.com/Pandaala/Edgion/tree/main/skills/<path>`, files → `https://github.com/Pandaala/Edgion/blob/main/skills/<path>`. **Never** `../Edgion/...`.
- Center facts: single binary `edgion-center`; depends on `edgion-resources`; ports — `:12251` Federation gRPC, `:12201` Admin HTTP, `:12200` probe (`/health` `/ready`), `:12290` Prometheus metrics; optional `embed-dashboard` feature embeds `web/dist/`.
- Category `SKILL.md` template: frontmatter (`name`, `description`) → one-paragraph scope → routing/file table → `See also` / `External (Edgion)` section.
- Work happens on branch `skills-system-skeleton` (already created; design spec already committed there).

---

### Task 1: Backend root index `skills/SKILL.md`

**Files:**
- Create: `skills/SKILL.md`

- [ ] **Step 1: Write the file**

```markdown
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
```

- [ ] **Step 2: Verify links resolve to existing local files**

Run:
```bash
cd /Volumes/ExtStore/ws5/EdgionCenter
for f in skills/01-architecture/06-center/SKILL.md skills/01-architecture/SKILL.md skills/02-features/SKILL.md skills/04-review/SKILL.md skills/05-testing/SKILL.md skills/09-misc/SKILL.md web/skills/SKILL.md; do test -e "$f" && echo "OK $f" || echo "MISSING $f"; done
```
Expected: `01-architecture/06-center/SKILL.md` and `web/skills/SKILL.md` are OK; the other five are MISSING (created in Tasks 3–7). This is the expected intermediate state.

- [ ] **Step 3: Commit**

```bash
git add skills/SKILL.md
git commit -m "docs(skills): add backend root index skills/SKILL.md"
```

---

### Task 2: Repo-root entry `AGENTS.md` + `CLAUDE.md` symlink

**Files:**
- Create: `AGENTS.md`
- Create: `CLAUDE.md` (symlink → `AGENTS.md`)

- [ ] **Step 1: Write `AGENTS.md`**

```markdown
# EdgionCenter — AI Agent Project Guide

## Project Overview

EdgionCenter is the federation hub (Center) for multi-cluster Edgion. A single
`edgion-center` binary is the **server** side of `FederationSync`: one or more Controllers
dial in over a bidirectional gRPC stream, send a `RegisterRequest`, and act as data sources
for reverse Watches issued by Center. Center aggregates the published PluginMetaData,
persists a controller registry in SQLite, and exposes an HTTP Admin API plus an optional
embedded web dashboard.

- **Binary:** `edgion-center` (`src/main.rs`); startup in `src/cli/`.
- **Shared crate:** depends on `edgion-resources` (currently a local `path` dep on the
  Edgion checkout; a git-rev pin is planned per the Cargo.toml TODO before release).
- **Ports:** `:12251` Federation gRPC (Controller → Center), `:12201` Admin HTTP API,
  `:12200` probe (`/health`, `/ready` only), `:12290` Prometheus metrics.
- **Web dashboard:** `web/` (React + TypeScript + Vite). The `embed-dashboard` feature
  embeds `web/dist/` into the binary; off by default (dashboard can also be served from a
  filesystem dir via `web.dir` / `EDGION_WEB_DIR`).
- **Auth:** mandatory. With neither `[local_auth]` nor `[auth]` configured, business routes
  fail-close with 503; `/health`, `/ready`, `/metrics`, `/api/v1/auth/status` stay reachable.
  Center is authentication-only — no `(verb, kind)` RBAC engine (that is Controller-private).

## Knowledge System

When a task needs project context, start from `skills/SKILL.md` and **load progressively on
demand** — do not read everything at once.

- **Backend / global:** [`skills/SKILL.md`](skills/SKILL.md)
- **Frontend dashboard:** [`web/skills/SKILL.md`](web/skills/SKILL.md)

### Navigation rules

1. Progressive loading: `skills/SKILL.md` → category `SKILL.md` → specific files.
2. Three-layer lookup: architecture → `skills/01-architecture/`; feature/config →
   `skills/02-features/`; frontend → `web/skills/`.
3. `docs/` targets humans (specs/plans history); `skills/` targets AI agents and developers.

## External dependency — Edgion skills

Shared knowledge (resource Schema, coding rules, testing framework, task workflows) lives in
the Edgion repo and is **referenced, not copied**. Entry:
https://github.com/Pandaala/Edgion/blob/main/skills/SKILL.md

## Common workflows

- **Understand Center internals:** `skills/01-architecture/SKILL.md` → `06-center/SKILL.md` →
  the specific module file.
- **Configure / deploy Center:** `skills/02-features/SKILL.md`.
- **Add a frontend page:** `web/skills/SKILL.md` → `02-patterns/` → `04-center/`.
- **Look up a resource Schema:** upstream
  https://github.com/Pandaala/Edgion/tree/main/skills/02-features/03-resources
```

- [ ] **Step 2: Create the `CLAUDE.md` symlink**

Run:
```bash
cd /Volumes/ExtStore/ws5/EdgionCenter
ln -s AGENTS.md CLAUDE.md
```

- [ ] **Step 3: Verify symlink**

Run: `ls -l /Volumes/ExtStore/ws5/EdgionCenter/CLAUDE.md`
Expected: `CLAUDE.md -> AGENTS.md`

- [ ] **Step 4: Commit**

```bash
git add AGENTS.md CLAUDE.md
git commit -m "docs: add repo-root AGENTS.md entry and CLAUDE.md symlink"
```

---

### Task 3: `skills/01-architecture/SKILL.md` (src-module map)

**Files:**
- Create: `skills/01-architecture/SKILL.md`

- [ ] **Step 1: Write the file**

```markdown
---
name: center-architecture
description: Entry point for EdgionCenter internals. Routes to the 06-center docs and maps src/ modules to their documentation.
---

# 01 Architecture

EdgionCenter internals. The federation hub design is documented under
[`06-center/`](06-center/SKILL.md); this page additionally maps each `src/` module to its
doc (or marks it as not yet documented).

## File list

| Topic | Entry |
|-------|-------|
| Center overview, fed-sync, aggregator, watch cache, persistence | [06-center/SKILL.md](06-center/SKILL.md) |

## src module → doc map

| `src/` module | Responsibility | Doc |
|---------------|----------------|-----|
| `fed_sync/` | Federation gRPC server, Controller registry, bidirectional stream | [06-center/01-fed-sync-server.md](06-center/01-fed-sync-server.md) |
| `aggregator/` | `ResourceAggregator` — merges Controller-published metadata | [06-center/02-aggregator-and-watch-cache.md](06-center/02-aggregator-and-watch-cache.md) |
| `watch_cache/` | `CenterWatchCache`, registry, reverse-Watch data flow | [06-center/02-aggregator-and-watch-cache.md](06-center/02-aggregator-and-watch-cache.md) |
| `metadata_store/` | `CenterMetaDataStore` | [06-center/02-aggregator-and-watch-cache.md](06-center/02-aggregator-and-watch-cache.md) |
| `db/` | `CenterDb`, SQLite `controllers` table | [06-center/03-persistence.md](06-center/03-persistence.md) |
| `api/` | Admin API handlers (consistency, region-route, IP-restriction, web) | TODO: not yet documented — see `src/api/` |
| `proxy/` | Controller proxy | TODO: not yet documented — see `src/proxy/` |
| `commander/` | Command dispatch to Controllers | TODO: not yet documented — see `src/commander/` |
| `cli/` | Binary startup / argument parsing | TODO: not yet documented — see `src/cli/` |
| `config/` | Config schema & defaults (ports, sync intervals) | [../02-features/SKILL.md](../02-features/SKILL.md) |
| `common/` | Shared infra: auth, conf_sync, fed_sync proto, observe, startup | TODO: not yet documented — see `src/common/` |

## See also

- [02-features/SKILL.md](../02-features/SKILL.md) — config schema and Admin API endpoints
- [04-review/SKILL.md](../04-review/SKILL.md) — review findings touching these modules
```

- [ ] **Step 2: Verify the 06-center targets exist**

Run:
```bash
cd /Volumes/ExtStore/ws5/EdgionCenter/skills/01-architecture
for f in 06-center/SKILL.md 06-center/01-fed-sync-server.md 06-center/02-aggregator-and-watch-cache.md 06-center/03-persistence.md; do test -e "$f" && echo "OK $f" || echo "MISSING $f"; done
```
Expected: all OK.

- [ ] **Step 3: Commit**

```bash
git add skills/01-architecture/SKILL.md
git commit -m "docs(skills): add 01-architecture index with src-module map"
```

---

### Task 4: `skills/02-features/SKILL.md` (new directory skeleton)

**Files:**
- Create: `skills/02-features/SKILL.md`

- [ ] **Step 1: Write the file** (config facts are real, taken from `config/edgion-center.yaml`; unwritten sub-pages are explicit TODOs)

```markdown
---
name: center-features
description: How to run and configure EdgionCenter — config schema, ports, deployment, auth bootstrap, Admin API endpoints.
---

# 02 Features

Operator-facing reference for EdgionCenter. Authoritative config example:
[`config/edgion-center.yaml`](../../config/edgion-center.yaml).

## Ports

| Port | Listener | Serves |
|------|----------|--------|
| `12251` | `server.grpc_addr` | Federation gRPC (Controller → Center) |
| `12201` | `server.http_addr` | Admin HTTP API |
| `12200` | `server.probe_addr` | `/health`, `/ready` only |
| `12290` | `server.metrics_addr` | Prometheus metrics |

## Sync configuration (`sync:`)

| Key | Default | Meaning |
|-----|---------|---------|
| `list_interval_secs` | 300 | ListRequest cadence per controller |
| `list_timeout_secs` | 30 | ListResponse wait timeout |
| `command_timeout_secs` | 30 | CommandResponse wait timeout |
| `ping_interval_secs` | 30 | Heartbeat ping interval |

## Authentication

Mandatory. Configure a `local_auth` or `auth` (OIDC discovery) provider; omitting both leaves
business routes returning 503 until configured. `/health`, `/ready`, `/metrics`,
`/api/v1/auth/status` remain reachable. See
[01-architecture/06-center/SKILL.md](../01-architecture/06-center/SKILL.md) §Authentication.

## Admin API

TODO: enumerate Admin API endpoints. Handlers live in `src/api/`
(`consistency_handlers.rs`, `region_route_handlers.rs`,
`global_connection_ip_restriction_handlers.rs`, `web.rs`).

## Deployment

TODO: document deployment shapes. See `deploy/` and `docker/` in the repo root, and the
`embed-dashboard` feature in `Cargo.toml`.

## External dependency — Edgion features

Resource feature Schema (Route/Plugin/TLS/Backend/…) is defined upstream and not duplicated:
https://github.com/Pandaala/Edgion/tree/main/skills/02-features/03-resources
```

- [ ] **Step 2: Verify config reference exists**

Run: `test -e /Volumes/ExtStore/ws5/EdgionCenter/config/edgion-center.yaml && echo OK || echo MISSING`
Expected: OK.

- [ ] **Step 3: Commit**

```bash
git add skills/02-features/SKILL.md
git commit -m "docs(skills): add 02-features index (config, ports, auth, admin-api skeleton)"
```

---

### Task 5: `skills/04-review/SKILL.md` (index over existing findings)

**Files:**
- Create: `skills/04-review/SKILL.md`

- [ ] **Step 1: Write the file** (routing table built from the existing finding files)

```markdown
---
name: center-review
description: EdgionCenter review findings, one file per finding, grouped by topic.
---

# 04 Review

Recorded review findings for EdgionCenter. One file per finding; grep/ls to discover.

## Topic groups

| Group | Findings |
|-------|----------|
| `architecture/` | [fed-proxy-header-forwarding.md](architecture/fed-proxy-header-forwarding.md), [register-validation-registry-caps.md](architecture/register-validation-registry-caps.md) |
| `cpu-memory/cases/` | [admin-api-controller-summaries-multi-call.md](cpu-memory/cases/admin-api-controller-summaries-multi-call.md), [centerdb-single-mutex-connection-not-blocking.md](cpu-memory/cases/centerdb-single-mutex-connection-not-blocking.md), [offline-controller-data-retention-business-requirement.md](cpu-memory/cases/offline-controller-data-retention-business-requirement.md), [watch-cache-registry-get-or-create-slow-path.md](cpu-memory/cases/watch-cache-registry-get-or-create-slow-path.md) |
| `h2-grpc/` | [fed-sync-keepalive.md](h2-grpc/fed-sync-keepalive.md), [heartbeat-timeout-pong-tracking.md](h2-grpc/heartbeat-timeout-pong-tracking.md) |

## See also

- [01-architecture/SKILL.md](../01-architecture/SKILL.md) — the modules these findings touch
- Edgion review conventions: https://github.com/Pandaala/Edgion/blob/main/skills/04-review/SKILL.md
```

- [ ] **Step 2: Verify every linked finding exists**

Run:
```bash
cd /Volumes/ExtStore/ws5/EdgionCenter/skills/04-review
for f in architecture/fed-proxy-header-forwarding.md architecture/register-validation-registry-caps.md cpu-memory/cases/admin-api-controller-summaries-multi-call.md cpu-memory/cases/centerdb-single-mutex-connection-not-blocking.md cpu-memory/cases/offline-controller-data-retention-business-requirement.md cpu-memory/cases/watch-cache-registry-get-or-create-slow-path.md h2-grpc/fed-sync-keepalive.md h2-grpc/heartbeat-timeout-pong-tracking.md; do test -e "$f" && echo "OK $f" || echo "MISSING $f"; done
```
Expected: all OK.

- [ ] **Step 3: Commit**

```bash
git add skills/04-review/SKILL.md
git commit -m "docs(skills): add 04-review index over existing findings"
```

---

### Task 6: `skills/05-testing/SKILL.md` (skeleton)

**Files:**
- Create: `skills/05-testing/SKILL.md`

- [ ] **Step 1: Write the file**

```markdown
---
name: center-testing
description: Test guidance for EdgionCenter. Center-specific notes here; the shared framework lives upstream.
---

# 05 Testing

Test guidance for EdgionCenter.

## Running tests

TODO: document Center's test entry points (`cargo test -p edgion-center`, integration
harness location). Capture commands as they stabilize.

## Center-specific scenarios

TODO: federation sync (register → reverse Watch), aggregator merge correctness, controller
offline/online transitions, CenterDb persistence.

## External dependency — Edgion testing framework

Unit/integration testing patterns are shared and defined upstream:
https://github.com/Pandaala/Edgion/tree/main/skills/05-testing
```

- [ ] **Step 2: Verify file created**

Run: `test -e /Volumes/ExtStore/ws5/EdgionCenter/skills/05-testing/SKILL.md && echo OK || echo MISSING`
Expected: OK.

- [ ] **Step 3: Commit**

```bash
git add skills/05-testing/SKILL.md
git commit -m "docs(skills): add 05-testing skeleton"
```

---

### Task 7: `skills/09-misc/SKILL.md` (index)

**Files:**
- Create: `skills/09-misc/SKILL.md`

Note: `skills/09-misc/center-db.md` is already a redirect stub pointing to
`01-architecture/06-center/03-persistence.md`. The index records that.

- [ ] **Step 1: Write the file**

```markdown
---
name: center-misc
description: Miscellaneous EdgionCenter notes and moved-file redirects.
---

# 09 Misc

Miscellaneous notes and redirects.

## Contents

| File | Topic |
|------|-------|
| [center-db.md](center-db.md) | Redirect → [01-architecture/06-center/03-persistence.md](../01-architecture/06-center/03-persistence.md) |
```

- [ ] **Step 2: Verify redirect target exists**

Run: `test -e /Volumes/ExtStore/ws5/EdgionCenter/skills/01-architecture/06-center/03-persistence.md && echo OK || echo MISSING`
Expected: OK.

- [ ] **Step 3: Commit**

```bash
git add skills/09-misc/SKILL.md
git commit -m "docs(skills): add 09-misc index"
```

---

### Task 8: Retarget `web/skills/SKILL.md` to Center, English

**Files:**
- Overwrite: `web/skills/SKILL.md`

The current file describes the Edgion **Controller** dashboard (port 12101, "20 CRUD pages",
dead refs `../../skills/02-dashboard/`, `ws2/skills/`). Replace wholesale with the Center,
English version below. Keep the generic pattern/architecture/resource routing (those files
stay) but fix framing, port, language, and external links.

- [ ] **Step 1: Overwrite the file**

```markdown
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
```

- [ ] **Step 2: Verify no stale references remain in this file**

Run:
```bash
grep -nE "12101|Edgion Controller|02-dashboard|ws2/skills|20 个 CRUD|已完成的功能" /Volumes/ExtStore/ws5/EdgionCenter/web/skills/SKILL.md || echo "CLEAN"
```
Expected: `CLEAN`.

- [ ] **Step 3: Commit**

```bash
git add web/skills/SKILL.md
git commit -m "docs(web/skills): retarget root index to Center, English"
```

---

### Task 9: Retarget `web/CLAUDE.md` to Center, English

**Files:**
- Modify: `web/CLAUDE.md`

This is a large file. Translate to English and fix every Controller-framing item. Apply these
concrete edits; keep the genuinely reusable pattern/i18n/coding sections (translated).

- [ ] **Step 1: Replace the header block**

Replace the opening (title + `## 项目概要` paragraph + tech stack + dev-server line) with:

```markdown
# EdgionCenter Dashboard — AI Agent Project Guide

## Project Overview

The EdgionCenter dashboard is the web management UI for EdgionCenter, built on React 18 +
TypeScript + Ant Design 5 + Vite 5. It talks to the Center Admin API (port 12201) over REST
to manage multi-cluster federation and the per-controller resources proxied through Center.

**Stack:** React 18, TypeScript 5, Ant Design 5, Vite 5, React Router 6, React Query 5,
Monaco Editor, Zod, Axios.

**Dev server:** `npm run dev` (port 5173, proxies `/api` to `localhost:12201`).
```

- [ ] **Step 2: Fix the backend bring-up commands block**

Replace the `### Controller API 端点` framing and the `## 常用命令` backend block so all
references point at Center: change `Controller Admin API: http://localhost:12101` to
`Center Admin API: http://localhost:12201`, and replace the `cd ../edgion` Controller/Gateway
bring-up with the Center bring-up:

```bash
# Backend (run from the EdgionCenter repo root)
cargo run --bin edgion-center -- --config-file config/edgion-center.yaml
# Center Admin API: http://localhost:12201
```

Rename the `### Controller API 端点` heading to `### Center API endpoints` and keep the
endpoint list (translate comments to English); update the probe note to `:12200` and Admin to
`:12201`.

- [ ] **Step 3: Translate remaining Chinese prose to English**

Translate section headings and prose (`知识体系`→`Knowledge System`, `核心开发模式`→`Core
development patterns`, `验证约定`→`Validation conventions`, `多语言（i18n）规范`→`i18n rules`,
`编码规范`→`Coding conventions`, etc.) and all inline Chinese sentences. Keep code blocks,
identifiers, and the resource Scope table as-is (they are already English/technical).

- [ ] **Step 4: Fix the i18n cross-reference**

Replace the dead reference `ws2/skills/02-dashboard/04-i18n.md` with the local
`skills/02-patterns/04-i18n-rules.md`.

- [ ] **Step 5: Verify no stale references remain**

Run:
```bash
grep -nE "12101|Edgion Controller|ws2/skills|cd \.\./edgion" /Volumes/ExtStore/ws5/EdgionCenter/web/CLAUDE.md || echo "CLEAN"
```
Expected: `CLEAN`.

- [ ] **Step 6: Verify no Chinese characters remain**

Run:
```bash
grep -nP "[\x{4e00}-\x{9fff}]" /Volumes/ExtStore/ws5/EdgionCenter/web/CLAUDE.md || echo "ALL-ENGLISH"
```
Expected: `ALL-ENGLISH`.

- [ ] **Step 7: Commit**

```bash
git add web/CLAUDE.md
git commit -m "docs(web): retarget CLAUDE.md to Center, English"
```

---

### Task 10: Delete `web/skills/PLAN.md`; mark resource tables source=Edgion

**Files:**
- Delete: `web/skills/PLAN.md`
- Modify: `web/skills/03-resources/SKILL.md` (add source banner)

- [ ] **Step 1: Delete the stale plan**

Run:
```bash
cd /Volumes/ExtStore/ws5/EdgionCenter
git rm web/skills/PLAN.md
```

- [ ] **Step 2: Add an authoritative-source banner to the resources index**

At the top of `web/skills/03-resources/SKILL.md` (immediately after the frontmatter), insert:

```markdown
> **Resource Schema is authoritative upstream (Edgion), not here.** This tree keeps
> frontend page-development notes only. Field definitions, types, and defaults:
> https://github.com/Pandaala/Edgion/tree/main/skills/02-features/03-resources
```

- [ ] **Step 3: Translate any Chinese prose in the 03-resources index to English**

Run to detect:
```bash
grep -nP "[\x{4e00}-\x{9fff}]" /Volumes/ExtStore/ws5/EdgionCenter/web/skills/03-resources/SKILL.md || echo "ALL-ENGLISH"
```
If Chinese is found, translate those lines to English; otherwise no change. Expected after fix: `ALL-ENGLISH`.

- [ ] **Step 4: Commit**

```bash
git add -A web/skills/
git commit -m "docs(web/skills): drop stale PLAN.md; mark resource Schema source as Edgion"
```

---

### Task 11: Final whole-tree verification

**Files:** none (verification only)

- [ ] **Step 1: Every category directory has a SKILL.md**

Run:
```bash
cd /Volumes/ExtStore/ws5/EdgionCenter
for d in skills skills/01-architecture skills/02-features skills/04-review skills/05-testing skills/09-misc web/skills web/skills/01-architecture web/skills/02-patterns web/skills/03-resources web/skills/04-testing; do test -e "$d/SKILL.md" && echo "OK $d" || echo "MISSING $d"; done
```
Expected: all OK.

- [ ] **Step 2: No local sibling-path references to Edgion anywhere in skills**

Run:
```bash
cd /Volumes/ExtStore/ws5/EdgionCenter
grep -rnE "\.\./Edgion|\.\./edgion|ws2/skills|02-dashboard" skills web/skills AGENTS.md web/CLAUDE.md || echo "CLEAN"
```
Expected: `CLEAN`.

- [ ] **Step 3: No Controller/12101 framing left in skills entry files**

Run:
```bash
cd /Volumes/ExtStore/ws5/EdgionCenter
grep -rnE "12101|Edgion Controller" skills web/skills/SKILL.md web/CLAUDE.md AGENTS.md || echo "CLEAN"
```
Expected: `CLEAN`.

- [ ] **Step 4: Backend skills are all English**

Run:
```bash
cd /Volumes/ExtStore/ws5/EdgionCenter
grep -rlP "[\x{4e00}-\x{9fff}]" skills AGENTS.md || echo "ALL-ENGLISH"
```
Expected: `ALL-ENGLISH`.

- [ ] **Step 5: Cold-start navigation smoke check**

Manually open `AGENTS.md` → follow to `skills/SKILL.md` → follow each Quick-locator link →
confirm each resolves to an existing file. Note any broken link and fix the source file.

- [ ] **Step 6: Final commit (if Step 5 produced fixes)**

```bash
git add -A
git commit -m "docs(skills): fix links found in final navigation check"
```

---

## Self-review notes (author)

- **Spec coverage:** A=Tasks 1–2; B=Tasks 3–7; C=Tasks 8–10; D (out of scope) respected
  (only stubs, no `.skill` files, no schema copies, no git-rev migration). Deliverables
  checklist items all map to a task; success criteria map to Task 11 verifications.
- **Intentional TODOs:** The `TODO:` markers inside Tasks 3/4/6 are *content* the skeleton
  deliberately leaves for later (per spec D1), not plan placeholders. Each names the exact
  `src/` location to document.
- **Link consistency:** root index (Task 1) links to the five category indexes created in
  Tasks 3–7; Task 1 Step 2 expects them MISSING at that point — ordering is intentional and
  Task 11 re-verifies the whole graph.
```
