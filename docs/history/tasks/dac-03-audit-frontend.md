# Task DAC-03: Audit read API + dashboard page

**Profile:** feature (backend + frontend)
**Status:** done (commit 1916f2e; backend+frontend tests pass)
**Depends on:** DAC-02
**Plan:** `docs/history/superpowers/plans/2026-06-13-center-dual-access-control-plan.md` §Task 3

## Scope

Expose the audit trail and render it in the dashboard.

## Checklist

- [ ] `src/api/audit.rs`: `GET /api/v1/center/admin/audit-logs?limit&offset&actor&controller&since&until`
      → `ListResponse<AuditRecordDto>`; excluded from `log_reads`.
- [ ] Mount route in `src/api/mod.rs`; handler test (filters + pagination).
- [ ] `web/src/api/audit.ts`: `auditApi.list(params)`.
- [ ] `web/src/pages/Audit/AuditLogPage.tsx`: table + filters + pager; Vitest render test.
- [ ] Route `/audit` (RequireAuth) in `App.tsx`; menu item in `menuConfig.tsx` (gate `audit:read`
      wired in DAC-08; visible for now).
- [ ] `npm run build` type-check green. Commit `feat: audit log read API + dashboard page`.

## Acceptance

Dashboard shows the audit trail with working filters and pagination.
