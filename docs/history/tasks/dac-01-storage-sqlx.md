# Task DAC-01: Storage foundation on sqlx (replace rusqlite)

**Profile:** feature / refactor
**Status:** todo (not started)
**Depends on:** none — this is the base
**Plan:** `docs/history/superpowers/plans/2026-06-13-center-dual-access-control-plan.md` §Task 1

## Scope

Replace `rusqlite`/`src/db/mod.rs` with a `sqlx`-backed `Store` supporting SQLite **and** MySQL
behind one async API. Preserve `controllers` behavior exactly. No new tables here.

## Checklist

- [ ] Add `sqlx` (sqlite+mysql+macros+migrate) to `Cargo.toml`; remove `rusqlite`.
- [ ] Extend `DatabaseConfig`: `backend: DbBackend (sqlite|mysql)`, `mysql_url: Option<String>`.
- [ ] Migrations `0001_controllers.sql` for sqlite + mysql (port CREATE + legacy DROPs).
- [ ] `src/store/{mod.rs,controllers.rs}`: `Store::connect/migrate/open_in_memory` + 4 controller
      methods (upsert/mark_offline/delete/list), dialect-branched upsert.
- [ ] Port the 3 existing `src/db` tests to async; add MySQL-gated round-trip
      (`EDGION_TEST_MYSQL_URL`).
- [ ] Migrate callers (`cli`, `api`, `fed_sync/server`); delete `src/db/mod.rs`.
- [ ] `cargo build && cargo test --lib` green; manual controllers API regression.
- [ ] Commit `refactor: replace rusqlite CenterDb with sqlx Store (sqlite+mysql)`.

## Acceptance

controllers CRUD identical on SQLite; MySQL round-trip passes when configured; `rusqlite` gone.
