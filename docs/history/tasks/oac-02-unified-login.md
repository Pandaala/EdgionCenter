# Task OAC-02: Unified password login (DB users then single-admin)

**Profile:** feature
**Status:** todo (not started)
**Depends on:** OAC-01 (DbAuthConfig); compiles with OAC-03
**Plan:** `docs/history/superpowers/plans/2026-06-14-center-orthogonal-access-control-plan.md` §Task 2

## Checklist
- [ ] Factor `LocalAuthState::verify_single_admin(username,password) -> bool` (timing-safe) out of
      the existing `login_handler`; keep the existing endpoint working via it.
- [ ] `UnifiedLoginState { store: Option<Arc<Store>>, local: Arc<LocalAuthState>,
      single_admin_enabled: bool }` + `unified_login_handler`: DB user (active+bcrypt) → else
      single-admin → else uniform 401; always run a dummy bcrypt for constant time; reuse
      `issue_login_response`.
- [ ] `add_unified_auth_routes(business, login_state, local_state)` mounts POST /auth/login +
      reuses local logout/me (replaces add_db_auth_routes).
- [ ] Tests: db_user_wins, falls_back_to_admin, db_inactive_rejected, uniform_401, no_store_admin_only.

## Acceptance
One login endpoint authenticates DB users + single admin; uniform 401; timing-safe; inactive rejected.
