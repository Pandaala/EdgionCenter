# Context

## Workspace routing

- `/Volumes/ExtStore/ws5/AGENTS.md`
- `/Volumes/ExtStore/ws5/skills/edgion-workspace/SKILL.md`
- `/Volumes/ExtStore/ws5/EdgionCenter/AGENTS.md`
- `/Volumes/ExtStore/ws5/EdgionCenter/skills/SKILL.md`

## Task workflow

- `/Volumes/ExtStore/ws5/Edgion/skills/07-tasks/SKILL.md`
- `/Volumes/ExtStore/ws5/EdgionCenter/tasks/00-roadmap-review.md`
- `/Volumes/ExtStore/ws5/EdgionCenter/tasks/01-audit-log-stdout.md`
- `/Volumes/ExtStore/ws5/EdgionCenter/tasks/02-decouple-controller-registry.md`
- `/Volumes/ExtStore/ws5/EdgionCenter/tasks/03-declarative-rbac.md`
- `/Volumes/ExtStore/ws5/EdgionCenter/tasks/04-stateless-jwt-oidc.md`
- `/Volumes/ExtStore/ws5/EdgionCenter/tasks/05-verification-cleanup.md`

## Architecture and feature knowledge

- `/Volumes/ExtStore/ws5/EdgionCenter/skills/01-architecture/SKILL.md`
- `/Volumes/ExtStore/ws5/EdgionCenter/skills/01-architecture/06-center/SKILL.md`
- `/Volumes/ExtStore/ws5/EdgionCenter/skills/02-features/access-control.md`
- `/Volumes/ExtStore/ws5/EdgionCenter/skills/05-testing/SKILL.md`
- `/Volumes/ExtStore/ws5/EdgionCenter/web/skills/SKILL.md`

## Current source boundaries

- `src/cli/mod.rs`: startup and composition; currently branches on optional SQL state.
- `src/api/mod.rs`: Admin API state; currently carries `Option<Arc<Store>>` and platform flags.
- `src/common/authz/`: existing `AuthzStore` seam with AllowAll and DB implementations.
- `src/common/unified_auth/`: common OIDC/local JWT authentication and full claims capture.
- `src/store/`: SQL persistence for controllers, audit, users, roles, and permission bindings.
- `src/fed_sync/`, `src/aggregator/`, `src/watch_cache/`, `src/metadata_store/`: shared runtime state and federation processing.

## Baseline

- `cargo test --all-targets`: 296 passed on 2026-07-14.
- `cicd/checks/check_english_only.sh`: passed.
- `cicd/checks/check_no_legacy_pm.sh`: passed for guarded paths.
