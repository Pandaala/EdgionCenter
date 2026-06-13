# Task DAC-02: Audit log backend (schema + sink + middleware)

**Profile:** feature
**Status:** todo (not started)
**Depends on:** DAC-01 (Store)
**Plan:** `docs/history/superpowers/plans/2026-06-13-center-dual-access-control-plan.md` §Task 2

## Scope

Record mutating admin actions with attribution into `audit_log`. Non-blocking (bounded mpsc +
background writer), fail-open (drop + metric on full channel). Used by BOTH tiers.

## Checklist

- [ ] `0002_audit_log.sql` (sqlite+mysql) with `idx_audit_log_ts`/`_actor`.
- [ ] `AuditConfig { enabled, log_reads, retention_days }` (default enabled=true, log_reads=false).
- [ ] `src/store/audit.rs`: `AuditRecord`, `AuditFilter`, `insert_audit/list_audit/prune_audit`.
- [ ] `src/common/audit/mod.rs`: `AuditSink::spawn/record` (cap 1024, drop→
      `edgion_center_audit_dropped_total`).
- [ ] `src/common/audit/middleware.rs`: read `UnifiedAuthClaims`, capture status, `source_ip` from
      `ConnectInfo` (NEVER X-Forwarded-For), decode proxy `controller_id` (`~`→`/`); skip GET unless
      `log_reads`; exclude the audit-read path.
- [ ] Wire into `compose_admin_routes` INSIDE `unified_auth` (claims present), outside business.
- [ ] `cli`: spawn sink when `database.enabled && audit.enabled`.
- [ ] Tests green; manual POST → DB row exists. Commit `feat: audit log backend`.

## Acceptance

Mutations recorded (actor/status/source_ip); reads excluded by default; full channel drops +
increments metric; no added request latency.
