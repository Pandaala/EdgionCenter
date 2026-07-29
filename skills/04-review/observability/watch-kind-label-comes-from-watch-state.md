---
name: watch-kind-label-comes-from-watch-state
description: Use when reviewing `center-runtime/src/federation/server.rs` watch metrics, `EDGION_CONFIG_DATA`, `FedWatchState.kind`, or any `fed_metrics::record_watch_*` call — explains why the `kind` label is read from the watch state rather than a module constant, why that was changed while only one kind is watched, and which re-reports are rejected.
---

# The Watch `kind` Metric Label Comes From `FedWatchState`, Not a Module Constant — FIXED

## Conclusion

`federation/server.rs` used one module constant, `PLUGIN_METADATA_KIND`, for three unrelated
jobs: building the real subscription, control flow on re-watch, and the `kind` label on every
watch metric. The first two were correct; the third was correct only because exactly one kind
is watched.

Fixed 2026-07-28. Source: issue 18 in `tasks/new_work/00-index.md` — its own task file,
`18-watch-kind-constant-doubles-as-metric-label.md`, was deleted on close per that directory's
convention ("a task file that no longer exists in this directory is done"). The constant is now
`EDGION_CONFIG_DATA` and names only the subscription;
`FedWatchState` carries `kind: &'static str`, and all twelve metric emit sites read
`pm_watch.kind`.

## What was wrong

`fed_metrics::record_watch_event` / `record_watch_list` / `record_watch_error` each take a
`kind` label meaning *which kind this response belongs to*. Every call site passed the module
constant instead, meaning *which kind this module subscribes to*. Identical today; divergent
the moment a second kind is watched.

The failure mode is silent. A second watched kind's events, list results, and errors would all
land on `edgion_fed_watch_events_total{kind="EdgionConfigData"}` and its siblings: nothing
fails to compile, no test reds, no log complains, dashboards keep rendering. The new kind's
problems hide inside the old kind's baseline — the worst direction for the error to point.

Threading the value through twelve sites was mechanical while the value was uniform. With two
kinds live, the same edit first requires deciding which kind each site is *actually* handling,
and the sites inside the shared retry and re-watch paths are exactly where that is ambiguous.
That asymmetry is why this was done before the second kind, not after.

## How it works now

- In production code `EDGION_CONFIG_DATA` is used only by `initial_watch_request` and by the
  one `FedWatchState::new` call in the stream loop — the subscription, not a label. (Tests
  construct many more `FedWatchState`s with it; that is the constructor argument, not a label
  source.)
- `FedWatchState::new(kind, request_id, server_id)` stores the kind; `re_watch()` takes no
  `kind` argument and reads `self.kind`.
- `apply_watch_list` and `apply_watch_event` already hold `&mut FedWatchState`, so every metric
  site reads `pm_watch.kind` with no new plumbing.
- The field is `&'static str`, so the label stays inside the bounded vocabulary
  `fed_metrics` requires (`fed_metrics.rs`: "Never pass raw user input"). It can never be a
  value echoed back from a `FedWatchRequest`.

Emitted label values are byte-identical to before. This was a zero-behavior-delta refactor by
design — that is what made it cheap.

## Re-reports that are NOT accepted

**1. "One kind is watched, so this is churn."** That was the finding, and it was fixed anyway.
The whole argument is about cost asymmetry: mechanical now, judgment-laden later, and the
failure it prevents is invisible to every automated signal the project has.

**2. "Take the kind from `FedWatchRequest.kind` / `resp` so it always matches the wire."** No.
That is caller-supplied data reaching a Prometheus label — the cardinality hazard
`fed_metrics.rs` opens by forbidding. The kind must come from the bounded `&'static str`
vocabulary the state was constructed with.

**3. "Store `kind: String` on `FedWatchState` so it can be built dynamically."** No, for the
same reason. `&'static str` is what makes "bounded" checkable at the type level.

**4. "The `plugin_metadata` field on `CenterSyncClient` should be renamed too."** Out of scope
here and deliberately deferred — see
`docs/superpowers/plans/2026-07-27-cci-02-configdata-read-model.md`. This fix renamed the
metric-label constant only.

**5. "`cicd/checks/check_no_legacy_pm.sh` should have caught the stale name."** It could not,
and for a bigger reason than it looks. Two things are true:

- Its pattern is `PluginMetaData|pluginmetadata|plugin-metadata` and `rg` is case-sensitive by
  default, so neither `PLUGIN_METADATA_KIND` nor `plugin_metadata` would have matched anyway.
- More importantly, its globs are `--glob 'src/**' --glob 'web/src/**'`, and ripgrep anchors a
  glob containing `/` to the search root. This repository has **no top-level `src/`** — Rust
  code lives under `crates/*/src/`. So the gate scans only `web/src/**` and has never inspected
  a single line of Rust. `rg --glob 'src/**' -e EDGION_CONFIG_DATA .` finds nothing from the
  repo root; `--glob '**/src/**'` finds it immediately.

Proof it is not merely theoretical: `crates/center-runtime/proto/fed_sync.proto:9` still
carries a literal `"PluginMetaData"` in a comment and the gate reports OK.

Do not cite this gate as evidence that Rust sources are free of legacy names. Widening it is a
separate decision, not a consequence of this fix, and the two candidate widenings differ:
widening the globs to `**/src/**` (case-insensitive) currently surfaces **nothing** — measured:
`rg -i --glob '**/src/**' -e 'PluginMetaData|pluginmetadata|plugin-metadata' crates web/src`
returns zero hits. Dropping the globs entirely is what has fallout: the
`examples/test/conf/Center/{ctrl1,ctrl2}/PluginMetaData_*.yaml` fixtures (which declare
`kind: PluginMetaData`, so they may be stale *schema*, not just stale naming) and several
`tasks/pending/...` prose files.

## Related

- `crates/center-runtime/src/observe/fed_metrics.rs` — the label-cardinality contract this
  obeys.
- `docs/superpowers/plans/2026-07-26-cci-01-watch-hardening-command-removal.md` — adds
  `supported_kinds` to this same watch path; its code snippets still show the pre-rename name.
