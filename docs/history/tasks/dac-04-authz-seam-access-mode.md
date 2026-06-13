# Task DAC-04: Authz abstraction + `[access] mode` (lite tier complete)

**Profile:** feature (backend + frontend)
**Status:** todo (not started)
**Depends on:** DAC-02 (audit in compose); independent of DAC-03
**Plan:** `docs/history/superpowers/plans/2026-06-13-center-dual-access-control-plan.md` §Task 4

## Scope

Introduce the authz seam and the `access.mode` switch. **After this task lite mode is fully
deliverable** (Okta/local login + audit + `login=admin`).

## Checklist

- [ ] `AccessConfig { mode: AccessMode (lite|full) }` (default lite) on `CenterConfig`.
- [ ] `src/common/authz/mod.rs`: `Principal`, `PermissionSet` (with `all()`), `AuthzStore` trait.
- [ ] `src/common/authz/catalog.rs`: permission-key constants + `route_permission(method,path)`;
      test `every_business_route_has_a_key`.
- [ ] `src/common/authz/allow_all.rs`: `AllowAllAuthz` (returns `PermissionSet::all()`).
- [ ] `src/common/authz/middleware.rs`: 403 on missing required key; pass-through when `all` or no
      required key; tests for deny/allow.
- [ ] Wire into `compose_admin_routes`: business → audit → authz → unified_auth → cache-control.
- [ ] `/auth/me` returns `{ user, provider, permissions: [..] }`.
- [ ] `cli`: select `AllowAllAuthz` when `mode == lite`.
- [ ] Frontend: `auth.ts me()` returns permissions; `utils/permissions.ts` (`PermissionProvider`,
      `useCan`); wrap routes in `App.tsx`.
- [ ] Manual lite end-to-end. Commit `feat: authz seam + access.mode (lite tier complete)`.

## Acceptance

Lite mode works end-to-end; `/auth/me` returns the full key set; authz denies unknown keys when the
store is non-allow-all.
