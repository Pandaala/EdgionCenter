# Task OAC-01: Config refactor (drop access.mode; add authz + db_auth)

**Profile:** refactor
**Status:** done (commit aa46551 + c93004b; part of OAC core)
**Depends on:** none (but compiles only with OAC-02 + OAC-03)
**Plan:** `docs/history/superpowers/plans/2026-06-14-center-orthogonal-access-control-plan.md` §Task 1

## Checklist
- [ ] Remove `AccessConfig`/`AccessMode` + the `access` field from `CenterConfig` (+ Default).
- [ ] Add `AuthzMode {AllowAll(default)|Rbac}`, `AuthzConfig{mode}`, `DbAuthConfig{enabled,
      jwt_secret?, jwt_expiry_hours?, cookie_secure?}`; wire `authz` + `db_auth` onto CenterConfig.
- [ ] Parse tests (via the production singleton_map_recursive path): authz default allow_all,
      authz rbac parses, db_auth default disabled, db_auth enabled parses.
- [ ] (Tree compiles only after OAC-03; commit standalone only if it builds, else fold into OAC-03.)

## Acceptance
New config types parse; `access` gone.
