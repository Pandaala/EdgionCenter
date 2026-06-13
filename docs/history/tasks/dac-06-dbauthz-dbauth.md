# Task DAC-06: DbAuthz + DB-user authn (full login + enforcement)

**Profile:** feature
**Status:** todo (not started)
**Depends on:** DAC-05 (users store), DAC-04 (authz seam)
**Plan:** `docs/history/superpowers/plans/2026-06-13-center-dual-access-control-plan.md` §Task 6

## Scope

Full-mode login against DB users (bcrypt) and RBAC enforcement via `DbAuthz`.

## Checklist

- [ ] `src/common/db_auth/`: login/me/logout mirroring `local_auth` shape; bcrypt verify; reject
      `status != active`; reuse local_auth JWT/cookie helpers (extract shared helper if needed).
- [ ] `src/common/authz/db_authz.rs`: `DbAuthz` over `Store::permission_keys_for_user` with a 30s
      TTL cache (moka OR hand-rolled `RwLock<HashMap>+Instant`).
- [ ] `cli`/`compose`: in full mode mount db-auth login + select `DbAuthz`; full mode + DB disabled
      → startup error.
- [ ] First-run bootstrap: zero users → create `admin` from `EDGION_ADMIN_USERNAME/PASSWORD` bound
      to a built-in `admin` role with `all_keys()`; WARN if env unset.
- [ ] End-to-end test: `controllers:read`-only user → 200 GET /controllers, 403 POST .../reload.
- [ ] Commit `feat: DbAuthz + DB-user login + RBAC enforcement (full tier)`.

## Acceptance

Full-mode login works; RBAC enforced (403 on missing key); inactive users rejected; admin bootstrap
works.
