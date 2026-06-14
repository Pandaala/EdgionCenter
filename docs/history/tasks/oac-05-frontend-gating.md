# Task OAC-05: Frontend menu gating on authzMode + dbAuthEnabled

**Profile:** feature (frontend)
**Status:** done (commit 62ae092; frontend tests pass)
**Depends on:** OAC-04
**Plan:** `docs/history/superpowers/plans/2026-06-14-center-orthogonal-access-control-plan.md` §Task 5

## Checklist
- [ ] `client.ts`: serverInfo type — replace `accessMode?` with `authzMode?: 'allow_all'|'rbac'`
      + `dbAuthEnabled?: boolean`.
- [ ] `menuConfig.tsx`: replace `requiredMode:'full'` with gates so **Users** shows when
      `authzMode==='rbac' || dbAuthEnabled` and **Roles** shows only when `authzMode==='rbac'`
      (both keep their `users:manage`/`roles:manage` permission gate). `Sidebar.tsx`/
      `useServerInfo.ts` feed the new fields into the filter ctx.
- [ ] `menuConfig.test.tsx`: allow_all+no-dbAuth → Users+Roles hidden; db_auth(+allow_all) →
      Users shown, Roles hidden; rbac → both shown.
- [ ] `npm run test && npm run build` green.

## Acceptance
Users page shows when rbac OR db_auth; Roles only when rbac; permission keys still honored.
