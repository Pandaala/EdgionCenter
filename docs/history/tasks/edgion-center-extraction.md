# Extract Center into the EdgionCenter repo + merge edgion-dashboard

## Meta
| Key | Value |
|-----|-------|
| Created | 2026-06-11 |
| Status | todo (UNBLOCKED — `edgion-resources` crate is extracted) |
| Type | refactor / new-repo |
| Priority | P1 |

## AI guide summary
> Move `src/core/center/` (20 files, ~6.3k lines — the federation Hub) OUT of the Edgion monorepo
> into the standalone **EdgionCenter** repo (`/Volumes/ExtStore/ws5/EdgionCenter`, currently empty +
> `.git`), and merge the **edgion-dashboard** React app (`/Volumes/ExtStore/ws5/edgion-dashboard`)
> into it (embedded via rust-embed, served same-origin). EdgionCenter becomes its own cargo binary.
> The hard prerequisite (a clean shared `edgion-resources` crate) is DONE — Center reuses it. The
> federation wire contract (`fed_sync.proto`) is COPIED + regenerated (not shared source). Auth/common
> infra is re-implemented (3rd-party) or copied as a minimal subset, per the original decision.

## Scope
### In scope
- New standalone `EdgionCenter` cargo project (the federation Hub binary).
- Resolve Center's dependency surface (share / copy-proto / re-implement — table below).
- Merge edgion-dashboard into EdgionCenter (build + rust-embed).
- Remove Center (`src/core/center/`, `src/bin/edgion_center.rs`, the `edgion-center` bin, `EdgionCenterCli`)
  from Edgion once EdgionCenter works.
### Out of scope (future)
- The lean-Hub vs optional Management-Plane split (`center-split-hub-management-plane.md`) — extract
  the **whole current Center as-is first**; refactor into Hub/MP later.
- RBAC / audit / multi-tenant (`center-auth-rbac-design.md`, `center-audit-log.md`) — Management-Plane,
  future.

## Center's dependency surface (audited 2026-06-11) → resolution
Center = 20 files / 6286 lines. Deps: `edgion_resources` (15), `crate::core::center` (30, internal),
`crate::core::common` (30):

| Center dep | Count | Resolution in EdgionCenter |
|---|---|---|
| `edgion_resources` | 15 | **SHARE** — git dependency on the Edgion repo pinned to a rev (`edgion-resources = { git = "…", rev = "…" }`), or vendor/submodule. The whole reason we extracted the crate. |
| `core::common::fed_sync` | 13 | **COPY the `.proto` (`src/core/common/fed_sync/proto/fed_sync.proto`) + regenerate** bindings in EdgionCenter (tonic/prost) — wire contract, not shared source. The fed-sync SERVER logic lives in `core::center::fed_sync/{server,registry}` → moves WITH Center. Shared fed_sync types (spiffe, proto wrappers) → copy the small surface. |
| `core::common::api` | 11 | Admin-API compose/HTTP layer. **Copy a minimal subset** (the router/compose Center actually uses) OR re-implement on axum. Audit `compose.rs` / `compose_admin_routes`. |
| `core::common::observe` | 9 | metrics/tracing. Re-implement with the same crates (metrics/prometheus/tracing) — copy the thin handles Center uses. |
| `core::common::{local_auth, unified_auth, auth}` | 10 | **RE-IMPLEMENT with 3rd-party** (OIDC/Okta crate + local users) per the original decision. Center's "login = admin" personal mode + fronted-by-MP mode (see `center-split-hub-management-plane.md` dual-mode). |
| `core::common::config` | 3 | Center config loader — copy the small Center-specific config + a minimal config helper. |
| `core::common::conf_sync` | 2 | conf_sync types Center uses — copy the small surface. |
| `core::common::{startup, grpc_tls, metadata_conf_handler}` | 3 | startup (panic hook / crypto init), gRPC mTLS, metadata handler — copy the tiny pieces, or re-derive. |

> Rule of thumb: **share `edgion_resources`; copy the `.proto`; for the rest, copy the MINIMAL subset
> Center uses, or re-implement with 3rd-party.** Do NOT try to share `core::common` wholesale — it
> drags controller/gateway coupling. Audit the exact symbols Center imports per module and pull only those.

## Phases
1. **Scaffold** EdgionCenter: `cargo init` (binary) in `/Volumes/ExtStore/ws5/EdgionCenter`; add the
   `edgion-resources` git dependency; confirm it compiles against the shared crate.
2. **fed_sync contract**: copy `fed_sync.proto` into EdgionCenter; set up `build.rs` (tonic-build);
   regenerate. Pin the proto version so Center (server) and Edgion controllers (clients) stay compatible.
3. **Move Center code**: bring `src/core/center/*` into `EdgionCenter/src/` (fed_sync server, aggregator,
   proxy, api, watch_cache, commander, metadata_store, db, cli, config). Rewrite `crate::core::center::`
   → `crate::` and `edgion_resources::` stays.
4. **Resolve the common deps** (the table) — per module: copy the minimal subset or re-implement
   (auth = 3rd-party). Iterate with `cargo check` until EdgionCenter builds standalone.
5. **Dashboard merge**: move `edgion-dashboard/` source into EdgionCenter (e.g. `EdgionCenter/web/`);
   wire `vite build` → `dist/`; embed via rust-embed + SPA history fallback on Center's Admin API
   listener (same-origin, no CORS) — follow `center-build-in-frontend.md` (hybrid: embed by default +
   filesystem override).
6. **Verify EdgionCenter standalone**: `cargo build`; boots; a real Edgion controller fed-syncs in
   (mTLS); aggregation + admin proxy work; dashboard served same-origin; auth (local + OIDC) works.
7. **Cut from Edgion**: delete `src/core/center/`, `src/bin/edgion_center.rs`, the `[[bin]] edgion-center`,
   `EdgionCenterCli`, and any Center-only `core::common` code now unused. `cargo check --all-targets`
   + integration tests green. The Edgion repo now has 3 binaries (gateway/controller/cli).

## Decisions (settled 2026-06-11)
1. **edgion-resources sharing = git dependency pinned to a rev** (`edgion-resources = { git = "<Edgion repo>",
   rev = "…" }`). Reproducible, two-repo standard, fits Center's separate release cadence.
2. **`core::common` resolution:**
   - **OIDC / OpenID Connect → 3rd-party crate** (e.g. `openidconnect`); local-users auth re-implemented
     on top. This replaces `core::common::{unified_auth, local_auth, auth}` for Center.
   - **Everything else reusable → copy the minimal subset Center actually uses** (executor's judgment per
     module: `api` compose/router, `observe` handles, `config`, `conf_sync` types, `startup`, `grpc_tls`,
     `metadata_conf_handler`). Pull only the symbols Center imports; don't drag the whole module. Prefer
     copy-the-subset over re-implement unless the subset is trivial.
3. **Dashboard lives IN EdgionCenter going forward** — move `edgion-dashboard/` source into the repo
   (e.g. `EdgionCenter/web/`), built + embedded per `center-build-in-frontend.md` (rust-embed, same-origin,
   SPA fallback; hybrid embed-by-default + filesystem override). Build wiring = executor's call (vite in
   CI/build vs committed `dist/`).

## References
- `center-split-hub-management-plane.md` — Hub vs Management-Plane boundary + dual-mode auth (future split).
- `center-build-in-frontend.md` — dashboard embedding (rust-embed, same-origin, SPA fallback).
- `center-auth-rbac-design.md`, `center-audit-log.md` — Management-Plane concerns (future).
- Shared crate: `edgion-resources/` (top-level). fed_sync proto: `src/core/common/fed_sync/proto/fed_sync.proto`.
- Center internals: `src/core/center/` (fed_sync, aggregator, proxy, api, watch_cache, commander, metadata_store, db).
