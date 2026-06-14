# Task OAC-04: server-info exposes authzMode + dbAuthEnabled

**Profile:** feature
**Status:** done (commit 0cef4bc; 283 tests pass)
**Depends on:** OAC-01, OAC-03
**Plan:** `docs/history/superpowers/plans/2026-06-14-center-orthogonal-access-control-plan.md` §Task 4

## Checklist
- [ ] Replace `ApiState.access_mode: AccessMode` with `authz_mode: AuthzMode` + `db_auth_enabled:
      bool`; update all ApiState constructions (cli + state_with_db helpers in api/mod, audit,
      users, roles).
- [ ] server-info: drop `accessMode`; add `authzMode: "allow_all"|"rbac"` + `dbAuthEnabled: bool`
      (camelCase).
- [ ] Update test → `server_info_reports_authz_and_db_auth` (rbac+db_auth case and
      allow_all+no-db_auth case).

## Acceptance
server-info returns the two new fields; accessMode gone.
