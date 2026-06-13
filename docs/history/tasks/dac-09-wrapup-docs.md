# Task DAC-09: Wrap-up — configs, skills docs, migration notes

**Profile:** docs
**Status:** todo (not started)
**Depends on:** DAC-01..08
**Plan:** `docs/history/superpowers/plans/2026-06-13-center-dual-access-control-plan.md` §Task 9

## Scope

Make both tiers configurable from the sample config + skills doc alone; retire old drafts.

## Checklist

- [ ] `config/edgion-center.yaml`: commented example blocks for both modes (`access.mode`,
      `database.backend` sqlite/mysql, `audit`).
- [ ] `skills/02-features/access-control.md` (lite vs full, config keys, permission catalog,
      bootstrap) + link from `skills/02-features/SKILL.md`.
- [ ] Mark `center-auth-rbac-design.md` and `center-audit-log.md` as superseded by this plan.
- [ ] Final `cargo test --lib && cd web && npm run build`. Commit `docs: access-control config
      examples + skills`.

## Acceptance

An operator can configure either tier from the sample config + skills doc alone.
