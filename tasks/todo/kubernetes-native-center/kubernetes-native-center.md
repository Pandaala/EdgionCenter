# Kubernetes-Native and Standalone Center Runtimes

## Meta

| Key | Value |
|---|---|
| Created | 2026-07-14 |
| Status | in-progress |
| Type | feature / architecture refactor |
| Priority | P1 |
| Issue | N/A |

## AI guide summary

Refactor EdgionCenter into a Cargo workspace with a platform-neutral core and runtime, then compose two explicit binaries: a Kubernetes-native binary that delegates persistence, RBAC, audit, and coordination to Kubernetes APIs, and a standalone binary that retains SQL/local management. The core difficulty is keeping federation and API behavior shared without creating a monolithic `ControlStore` trait or scattering platform conditionals through handlers.

## Requirements

- Keep one implementation of federation, aggregation, Admin API contracts, OIDC validation, observability, and business rules.
- Build Kubernetes and standalone distributions as separate binaries with isolated platform dependencies.
- Use Kubernetes CRDs/status, native RBAC authorization checks, API-server audit, and Lease-based coordination in Kubernetes mode.
- Retain SQLite/MySQL users, roles, controller history, and audit history in standalone mode.
- Do not add file-backed users or file RBAC.
- Preserve a clear capability contract so the dashboard can adapt to platform-specific management surfaces.
- Migrate incrementally while keeping the existing test baseline green.

## Scope

### In scope

- Cargo workspace and crate boundaries.
- Platform capability traits and composition roots.
- Two binaries and separate deployment artifacts.
- Kubernetes-native controller directory, authorization, coordination, and audit strategy.
- SQL-backed standalone adapters.
- Migration of the current single-package code without behavior loss.
- Reconciliation of Tasks 01–05 with the new architecture.

### Out of scope

- Declarative E2E probing implementation (Task 06 remains a separate initiative).
- Replacing Kubernetes etcd or SQL internals.
- Implementing a third file-backed management mode.
- Creating a new service-token product before its requirements are approved.

## Document index

| Document | Status | Notes |
|---|---|---|
| `00-context.md` | done | Current code and skill entry points |
| `01-design.md` | done | Approved target workspace, ports/adapters, runtime composition, and migration |
| `02-implementation.md` | in-progress | Increment plan and verification strategy |
| `03-subtasks.md` | in-progress | KN-01 and KN-02 complete; KN-03 started |
| `05-issues.md` | — | Create when an implementation blocker is found |
| `06-decisions.md` | done | Initial architecture decisions recorded |

## Affected modules

| Module | Impact |
|---|---|
| Root `Cargo.toml` | Convert the package to a workspace and centralize dependencies/lints |
| `src/` | Move current shared behavior into core/runtime crates |
| `src/store/` | Become the SQL adapter |
| `src/common/authz/` | Split platform-neutral policy contract from SQL/Kubernetes implementations |
| `src/cli/` | Replace the monolithic composition path with two thin binary roots |
| `src/api/` | Depend on capabilities/ports rather than `Option<Store>` |
| `src/fed_sync/` | Remain shared runtime behavior; persist compact status through a port |
| `config/` | Split shared, Kubernetes, and standalone examples |
| `cicd/` | Build and package two binaries; add Kubernetes CRDs/RBAC/Lease permissions |
| `web/` | Render features from server capability metadata |
| `skills/` and `docs/` | Document both deployment models and remove stale architecture text |

## Decision log

| Date | Decision | Reason |
|---|---|---|
| 2026-07-14 | Prefer Kubernetes-native management over file RBAC | Reuse Kubernetes persistence, identity, RBAC, audit, and HA primitives instead of maintaining a second user/role system |
| 2026-07-14 | Retain SQL for standalone deployments | VM/Docker environments do not have a Kubernetes API and already have a working management implementation |
| 2026-07-14 | Use two explicit binaries | Compile-time composition keeps platform dependencies and invalid configuration combinations out of each artifact |
