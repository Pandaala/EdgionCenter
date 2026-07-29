---
name: reload-outcome-not-just-dispatch
description: Use when reviewing `center-app/src/api/reload_ops.rs`, `reload_controller`, or the dashboard reload button — explains why a reload reports a terminal outcome instead of dispatch success, why the budget is 20 s and not the write path's 10 s or the Controller's 30 s hint, why the Controller was deliberately not changed, and why 409/503 are carried on extra fields rather than new `OutcomeState` variants.
---

# Reload Reports a Terminal Outcome, Not Dispatch Success — FIXED

## Conclusion

`POST /api/v1/controllers/{id}/reload` on Center used to map any 2xx from the Controller to
`200 {"ok"}`, and the dashboard rendered that as a green toast. That was wrong: the
Controller's 200 only ever means *queued*.

Fixed 2026-07-28 (source: `tasks/new_work/17-reload-reports-initiation-not-convergence.md`,
closed; registry entry CCA-I05 in `../../../tasks/center-controller-alignment/05-issues.md`).

## What was wrong

`edgion-controller/src/api/mod.rs` (`reload_all_resources`) applies the `[center]`/`cli_tokens`
policy synchronously and then calls `conf_mgr.request_reload()`, which is **fire-and-forget**.
Its 200 body says so verbatim: `"Reload initiated - controllers will restart with new
server_id"`.

Center discarded that qualifier. A reload that was dispatched and then wedged produced no
signal anywhere — no timeout, no failure callback, no state transition, only a success toast.
Meanwhile the `EdgionConfigData` write path (`config_data_ops.rs`) already modelled exactly
this problem with six outcomes and a 10 s budget. The heavier, rarer, more destructive
operation had none of it.

## How it works now

`crates/center-app/src/api/reload_ops.rs`:

1. Capture the Controller's `server_id` from the **local watch cache**
   (`CenterWatchCacheRegistry::cached_server_id`) *before* dispatch — no extra round trip.
2. Dispatch the proxied `POST /api/v1/reload`.
3. Poll the local cache, 500 ms cadence, until that id changes.
4. Report `Converged` | `Accepted` | `Unknown` | `Conflict` | `Failed`.

The `server_id` works as the completion signal because a Controller mints a fresh one every
time it rebuilds its ConfigSyncServer, and the cache only ever refreshes it through a
successfully applied list/event batch — so a *change* means the Controller genuinely restarted
and Center re-listed against the new instance. A wedged reload leaves it untouched.

## Re-reports that are NOT accepted

**1. "The budget should be 30 s to match the Controller's `Retry-After: 30`."** Rejected. The
budget is bounded from above by the *client*, not by how long a reload takes:
`web/src/api/client.ts` sets a 30 s axios timeout and the proxy tunnel's own per-request
deadline (`command_timeout_secs`) defaults to 25 s. A budget at or near 30 s converts a real
outcome into a browser-side abort — reintroducing the "no signal anywhere" failure this fix
removes. 20 s leaves 10 s of headroom. `reload_ops::budget_is_bounded_by_the_client_timeout`
guards the constant; change the axios timeout first if you want to change the budget.

**2. "A reload slower than 20 s reports `Unknown`, so the budget is too short."** Not a defect.
`Unknown` is a real terminal state meaning "dispatched, completion not observed within this
request", and it is honest. Making large-cluster reloads reach `Converged` requires an
asynchronous 202 + a polling endpoint + server-side reload state — a materially larger design,
not a budget tweak. Propose it as a task, not as a review fix.

**3. "Add `RetryAfter` / `NotLeader` states to `OutcomeState`."** Rejected. That enum is shared
with the `EdgionConfigData` write path and is frozen; widening it would force every existing
consumer (`WriteOutcomeTag.tsx`, `region_route_handlers::OutcomeClass`) to grow arms for states
they cannot produce. A transient `503` and `409 not leader` both mean "nothing was applied", so
they map to `Failed`/`Conflict` and carry their *actionable* part on `retry_after_secs` /
`leader`. That is what keeps them distinguishable in the UI.

**3b. "`retry_after_secs` should mean 'a reload is already in progress'."** No — and do not
reintroduce that wording. `request_reload()` fails transiently for more than one reason
(already queued, or the config center not started/ready), and the Controller answers `503` +
`Retry-After: 5` for all of them. The retry action is the same, so they share one state, but
only the Controller's own body says which; `reason` carries it verbatim and the UI shows it
(`center.reloadOutcome.retry` interpolates `{reason}`). Asserting a cause Center cannot know is
the same class of error as the original finding.

**4. "The Controller should return a completion signal / gain a `/reload-status` endpoint."**
Rejected, and the original acceptance criteria forbade it. The Controller is not the defect: a
queue push can honestly promise nothing beyond "queued". `server-info` and the watch stream
were already sufficient. Its doc comment now states the initiation-only semantics explicitly —
verify that comment before re-reporting the Controller side as a gap.

**5. "`reload_controller` should reuse the generic `proxy_handler`."** Rejected. `proxy_handler`
serves every `ANY /api/v1/proxy/{controller_id}/{*rest}` request; reload-specific convergence
semantics there would apply to unrelated requests. (The original finding cited
`api/mod.rs:951-982`, which is `proxy_handler` — the reload handler was at 819.)

**6. "Return 200 for every outcome so the frontend has one code path."** Rejected. `Conflict`
and `Failed` mean nothing was applied; answering 200 would re-create the original dishonesty
for any non-browser client. They are returned as 409 / 503 / 502, and the frontend lifts the
outcome out of the rejection (`reloadOutcomeFromError`). Note the reload request must keep
`_silent: true` — the shared axios interceptor otherwise pops its own hard-coded error toast
over the reload-specific wording.

**7. "Poll `GET /api/v1/server-info` instead of the local cache."** Rejected for the same
reason the write path does not re-GET: the federation watch stream already keeps the cache
current, so convergence is observable for free. A per-tick proxied GET would add a round trip
through a Controller that is, by hypothesis, mid-restart.

**8. "The budget is 20 s, so 20 s + the 25 s tunnel deadline could exceed the 30 s axios
timeout."** Half right, and already documented in the module docs. The dispatch is *inside* the
budget, so the worst case is `max(tunnel_deadline, budget) + one poll interval`, not their sum
— ~25.5 s at the default. But `sync.command_timeout_secs` is operator-configurable and is not
test-guarded, so raising it above 30 s does re-open the hole. Treat that field as part of this
contract; a fix would guard it at startup, not shrink the budget.

**9. "Center should probe `GET /api/v1/server-info` instead of / in addition to the cache."**
The steady-state poll: rejected, see 7. A *one-shot* probe at budget exhaustion, to upgrade
some `Unknown`s and `Accepted`s into real verdicts, is a legitimate follow-up — but it is new
behavior on a new channel, so it belongs in a task, not in a review fix.

## Edge cases already handled (do not re-report)

- **No baseline** (`cached_server_id` returns `None`): reports `Accepted`, not a guess. Adopting
  the first observed id as "converged" would report success for a reload that never ran.
- **Non-owning replica**: reports `Accepted` immediately rather than waiting out the budget —
  20 s later it would say the same thing.
- **Cache entry goes absent mid-wait**: not treated as convergence. Only a re-list repopulates
  it, and that is what carries the new id.
- **`Retry-After` absent or in HTTP-date form**: falls back to the Controller's documented 5 s.
  The condition is transient either way; dropping the field would render as a permanent failure.
- **A dispatch slower than the whole budget**: the poll loop observes the cache *before* testing
  the budget and before its first sleep, so a congested tunnel cannot produce `Unknown` while the
  new `server_id` already sits in the cache. Pinned by
  `a_dispatch_that_outlasts_the_budget_still_observes_the_cache`.
- **Upstream non-2xx other than 409/503**: reported as `502` with the upstream status and body in
  `reason`, deliberately *not* echoed as Center's own status — echoing an upstream `403` made a
  Center-side authorization failure indistinguishable from a Controller-side one. Pinned by
  `other_upstream_rejections_collapse_to_502_with_the_detail_in_reason`.
- **A transport failure's message on the dashboard**: `_silent` suppresses the interceptor's
  per-status toast, so `reloadErrorMessage` prefers the server's own `error` over axios's
  "Request failed with status code 404". The 401 branch runs *before* the `_silent` check in
  `client.ts`, so session expiry still redirects to login.
- **`409` body unparseable or `leader: null`** (mid-election): still `Conflict`, just without an
  address.
- **Transport failures** (404 unknown controller, 504 tunnel timeout): returned as `Err` with
  the proxy's own status, never flattened into a `Failed` outcome. `api/mod.rs`'s
  `reload_uses_proxy_tunnel_and_maps_unknown_controller_to_404` pins this.

## Related

- `config_data_ops.rs` — the write-path convergence core this mirrors, and the owner of
  `OutcomeState`.
- [center-writes-use-unfenced-proxy-forward.md](center-writes-use-unfenced-proxy-forward.md) —
  why the dispatch transport is unfenced.
