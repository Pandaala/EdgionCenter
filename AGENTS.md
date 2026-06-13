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
