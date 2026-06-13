# Task DAC-08: Frontend user mgmt + role matrix + menu gating

**Profile:** feature (frontend)
**Status:** todo (not started)
**Depends on:** DAC-07 (API), DAC-04 (permission context)
**Plan:** `docs/history/superpowers/plans/2026-06-13-center-dual-access-control-plan.md` §Task 8

## Scope

Dashboard pages for managing users/roles/permissions and permission-based menu/button gating.

## Checklist

- [ ] `web/src/api/users.ts`, `web/src/api/roles.ts` clients.
- [ ] `pages/Users/UserManagementPage.tsx`: list/create/disable/delete users, assign roles, reset
      password; render test.
- [ ] `pages/Roles/RoleManagementPage.tsx`: role list + permission **matrix** (checkbox grid from
      `permission-catalog`, grouped by page); render test.
- [ ] `menuConfig.tsx`: `requiredPermission` per item + `useCan` filter (Users/Roles → manage keys,
      Audit → `audit:read`).
- [ ] `/users`, `/roles` routes (gated) in `App.tsx`; `npm run test && npm run build` green.
- [ ] Commit `feat: user-management + role-matrix dashboard pages + menu gating`.

## Acceptance

Full-mode admin manages users/roles/permissions from the UI; lite shows everything; non-admins
don't see management menus and get 403 if they force the API.
