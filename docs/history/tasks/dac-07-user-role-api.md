# Task DAC-07: User/role admin CRUD API

**Profile:** feature
**Status:** todo (not started)
**Depends on:** DAC-05 (store), DAC-06 (authz enforcement)
**Plan:** `docs/history/superpowers/plans/2026-06-13-center-dual-access-control-plan.md` §Task 7

## Scope

HTTP CRUD for users, roles, bindings, and the permission catalog (all gated).

## Checklist

- [ ] `src/api/users.rs`: `GET/POST /api/v1/center/admin/users`, `PATCH/DELETE /.../users/{id}`
      (status / password / roles) — gated `users:manage`; bcrypt-hash on create/reset.
- [ ] `src/api/roles.rs`: `GET/POST /.../roles`, `PUT /.../roles/{id}/permissions`,
      `DELETE /.../roles/{id}` — gated `roles:manage`.
- [ ] `GET /api/v1/center/admin/permission-catalog` → grouped key catalog.
- [ ] Mount routes (tagged in `catalog.rs`); per-endpoint handler tests.
- [ ] Commit `feat: user/role admin CRUD API`.

## Acceptance

Full CRUD over users/roles/bindings; catalog endpoint returns grouped keys.
