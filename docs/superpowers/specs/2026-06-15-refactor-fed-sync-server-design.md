# Refactor `sync()` into testable watch handlers — Design

**Date:** 2026-06-15
**Task:** `tasks/01-refactor-fed-sync-server.md`
**Type:** Maintainability + test coverage · **Risk:** Medium · **Priority:** High

## Problem

`FederationGrpcServer::sync()` in `src/fed_sync/server/mod.rs` is a ~1000-line
method. Its main message loop (`mod.rs:547-816`) is a ~270-line `tokio::select!`
with a `match` nested up to six levels deep, all inside a `tokio::spawn` closure.

The most important business logic — parsing and applying `WatchListResponse` and
`WatchEventResponse` into the watch cache — has **no unit tests**, because it is
unreachable except by driving a full gRPC stream. Existing tests only cover
`validate_register_req`, capacity gating, and the registry.

## Goals

- Extract the watch-payload logic into standalone, synchronously unit-testable
  functions.
- Shrink the `select!` loop body to < ~120 lines.
- Shrink `sync()` itself by extracting the register/SPIFFE prologue.
- **Zero behavior change:** stale-session guard, `update_last_seen`, last-pong
  update, heartbeat, `mark_offline_all` fan-out, backoff timing, and every
  `fed_metrics::record_*` call are preserved.

## Architecture / file layout

Add `src/fed_sync/server/watch.rs` to hold the testable watch core; `mod.rs`
keeps wiring and side-effect execution.

**Moved to `watch.rs`:**
- `WatchEventRaw`
- `FedWatchState` (incl. `re_watch`)
- new `WatchOutcome` enum
- `apply_watch_list`, `apply_watch_event`
- their `#[cfg(test)]` unit tests

**Stays in `mod.rs`:**
- the `select!` main loop and spawn wiring
- a new private method `FederationGrpcServer::authorize_register(&self,
  peer_certs, &register_req) -> Result<(), Status>` that absorbs the current
  validate → capacity → SPIFFE prologue (current lines 252-311), shrinking
  `sync()` itself. **All three checks' `tracing::warn!` calls and the
  `fed_metrics::record_peer_identity_check` emission (both OK and failure paths)
  stay INSIDE `authorize_register`** — it returns only the bare `Status` (already
  the distinct `invalid_argument` / `resource_exhausted` / `permission_denied` /
  `internal` codes), and `sync()` just `?`-propagates it. Boundary is exactly
  252-311: `let controller_id = …clone()` (313), the "Controller registered" info
  log (316-322), and `registry.register(…)` (325+) all stay in `sync()`.

`FedWatchState`, `WatchEventRaw`, and `WatchOutcome` move to `watch.rs` and become
`pub` with `pub` fields (mod.rs reads `pm_watch.request_id` / `.server_id` /
`.consecutive_errors` directly); `re_watch` / `new` become `pub`.

## Core components: decision vs. execution

```rust
enum WatchOutcome { Applied, Skip, ReWatch, BackoffThenReWatch }

fn apply_watch_list(
    cid: &str,
    cache: &CenterWatchCache<PluginMetaData>,
    watch: &mut FedWatchState,
    resp: WatchListResponse,
) -> WatchOutcome;

fn apply_watch_event(
    cid: &str,
    cache: &CenterWatchCache<PluginMetaData>,
    watch: &mut FedWatchState,
    resp: WatchEventResponse,
) -> WatchOutcome;
```

Handlers are **fully synchronous**: they do the `request_id` comparison, JSON
parsing, cache application (`replace_all` / `apply_events`), `server_id` /
`consecutive_errors` updates, metric emission, and logging — then return a
`WatchOutcome`. They touch **no async, no channel, and do not call `re_watch`**.

`cid: &str` is passed in so the handlers' `tracing::*` calls keep the
`controller_id = %cid` field they emit today (loop closure captures `cid`; the
extracted functions need it explicitly). Tests pass any string.

## Data flow (loop side)

The loop matches the payload, calls the handler, then acts on the outcome:

| Outcome | Loop action |
|---|---|
| `Skip` | `continue` (stale `request_id`) |
| `Applied` | nothing |
| `ReWatch` | `watch.re_watch(KIND)` → `inner_tx.send` (server_id changed) |
| `BackoffThenReWatch` | `select!{ sleep(3s) / inner_tx.closed() => break }`, then `re_watch` + `send` (recv error) |

`re_watch` (rotates `request_id`, resets `consecutive_errors`) is invoked by the
loop, not the handler. This keeps all async/timing/channel concerns in the loop
and leaves the handlers pure and synchronously testable. The `BackoffThenReWatch`
arm keeps the existing nested `select!` (its `inner_tx.closed()` branch `break`s
the outer loop) — this is the one intentional bit of nesting that remains.

## Error handling / preserved invariants

- stale-session guard (`is_current_session`) — unchanged, stays in loop.
- `update_last_seen`, last-pong store, heartbeat task — unchanged.
- `mark_offline_all` four-way fan-out and reason-gated metrics — unchanged.
- 3s backoff timing and session-close detection during backoff — unchanged,
  stays in loop (driven by `BackoffThenReWatch`).
- **Metric emission stays inside the handlers** (confirmed): they are global
  counters, harmless to call from unit tests, and keeping them in the handler is
  the most faithful preservation of current behavior.
- **Parse error does not trigger a re-watch** (confirmed): list/event parse
  failures only log + emit a metric, exactly as today (handler returns `Applied`
  for a failed list parse; for events, the batch is parsed once and a failure
  there is logged + metric, no re-watch).

## Testing

`watch.rs` unit tests construct the cache directly via
`CenterWatchCache::<PluginMetaData>::new(id, handler)`. The existing `MockHandler`
in `watch_cache/cache.rs` is private to that file's `#[cfg(test)]` module, so
`watch.rs` tests define **their own** no-op `CenterConfHandler<PluginMetaData>`
impl (the trait is `pub` in `watch_cache/traits.rs`). Proto response types are
plain prost structs with public fields — constructed by field literal. Cases:

- list happy path → cache populated, `server_id` updated, returns `Applied`
- stale `request_id` → returns `Skip`, cache untouched
- `server_id` change → returns `ReWatch`
- list parse error → cache untouched, no re-watch
- event-type classification → add / update / delete applied, unknown type skipped
- event `error` non-empty → returns `BackoffThenReWatch`, `consecutive_errors`
  incremented

## Acceptance criteria

- [ ] `apply_watch_list` and `apply_watch_event` are standalone with unit tests
      covering the cases above.
- [ ] `select!` loop body < ~120 lines.
- [ ] No behavior change (invariants above all preserved).
- [ ] `cargo test` and `cargo clippy` pass.

## Out of scope

- The simple arms (Pong / CommandResponse / HttpProxyResponse / StatsReport,
  ~4-8 lines each, ~20 lines total) are **not** extracted — extraction would be
  churn for churn's sake.

## Loop-size note

Today's `select!` body is ~269 lines (547-815). Extracting the two watch arms
into ~4-line handler-call + outcome-match blocks removes the bulk (~195 lines),
landing the body around ~110-115 lines. The < ~120 goal is met but with thin
margin; if it overshoots, the disconnect-cleanup arms (`Err` / `Ok(None)`, each a
log + `mark_offline_all` + `break`) are the next extraction candidate — but that
is a fallback, not planned work.
