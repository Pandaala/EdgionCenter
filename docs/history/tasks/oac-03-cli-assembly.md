# Task OAC-03: Provider-driven cli assembly + validation + bootstrap

**Profile:** feature (core)
**Status:** todo (not started)
**Depends on:** OAC-01, OAC-02
**Plan:** `docs/history/superpowers/plans/2026-06-14-center-orthogonal-access-control-plan.md` §Task 3

## Checklist
- [ ] Replace the `match config.access.mode {Lite|Full}` block in `src/cli/mod.rs` (~345-433)
      with provider-driven assembly: oidc_on / single_admin_on / db_auth_on / rbac flags.
- [ ] Session secret = local_auth.jwt_secret else db_auth.jwt_secret; required if any password
      login. Build UnifiedAuthState (OIDC passed through ONLY when oidc_on — no force-disable;
      local HS256 validator when any password login). AuthzStore: rbac→DbAuthz else AllowAllAuthz.
- [ ] Startup validation (prefer a pure `validate_access(config, store_present)` fn + test it):
      rbac→needs DB; db_auth→needs DB; password login→needs secret.
- [ ] Bootstrap retie: db_auth_on && users empty && EDGION_ADMIN_* set → bootstrap_admin (admin
      role with all_keys when rbac); WARN if env unset. Remove random-placeholder-only path /
      OIDC-disable / placeholder-password trick (keep placeholder pw only when db_auth-only & no
      single admin, as defense-in-depth).
- [ ] Mount routes via add_unified_auth_routes when any password login; compose with
      local_auth_intent = (any password login on).
- [ ] E2E: oidc_rbac_unmapped_sub_403 (+200 after provisioning), db_auth_allow_all_grants_everything,
      db_auth_rbac_enforces; keep center_no_auth_config...503 green; update AccessMode-referencing tests.
- [ ] `cargo build` + full `cargo test --bin edgion-center` green.

## Acceptance
Any axis combo assembles; startup validation fails closed; OIDC coexists with rbac; bootstrap works.
