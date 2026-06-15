# Task DAC-05: Full-tier schema + permission-key bindings

**Profile:** feature
**Status:** done (commit 8858e91; 247 tests pass)
**Depends on:** DAC-01 (Store), DAC-04 (catalog keys)
**Plan:** `docs/history/superpowers/plans/2026-06-13-center-dual-access-control-plan.md` §Task 5

## Scope

DB tables and `Store` methods for full-tier users, roles, and bindings. No HTTP yet.

## Checklist

- [ ] `0003_users_roles.sql` (sqlite+mysql): `users`, `roles`, `user_roles`, `role_permissions`,
      optional `api_tokens`; UNIQUE on `username`/`name`; composite PKs on join tables.
- [ ] `src/store/users.rs`: `User`/`Role` + create/get/list/status/password/delete for users;
      create/list/delete + `set_role_permissions`/`role_permissions` for roles;
      `set_user_roles`; `permission_keys_for_user(username)` (JOIN).
- [ ] Test `user_role_permission_join` (sqlite) + MySQL-gated twin.
- [ ] Commit `feat: full-tier users/roles/bindings schema + store methods`.

## Acceptance

The JOIN resolves a user's effective permission keys on both backends.
