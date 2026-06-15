# Task OAC-06: Docs for the orthogonal model

**Profile:** docs
**Status:** done (commit 896d917; config parses, docs accurate)
**Depends on:** OAC-01..05
**Plan:** `docs/history/superpowers/plans/2026-06-14-center-orthogonal-access-control-plan.md` §Task 6

## Checklist
- [ ] `config/edgion-center.yaml`: remove the `access:` block; document `authz.mode`, the three
      authn providers (`auth`/`local_auth`/`db_auth`), session-secret precedence, unified login,
      rbac fail-closed for unmapped identities, bootstrap env vars. Default stays allow_all + no
      db_auth. YAML valid.
- [ ] `skills/02-features/access-control.md`: rewrite around the three axes + a combination
      matrix; config keys; startup validation; bootstrap; menu visibility rules; known limitations.
- [ ] Mark the 2026-06-13 spec's `access.mode` portion superseded by the 2026-06-14 design.
- [ ] `cargo test --bin edgion-center config` + `cd web && npm run build` green.

## Acceptance
Any combination is configurable from the sample config + skills doc alone.
