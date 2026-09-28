# Combined native browser regression

## Scope and runtime

On 2026-09-28, rebuilt native Center and Controller from the current sibling
sources and ran every standalone Playwright case against Center production code
at `daf70d3`, plus the new filtered-batch regression. No Edgion history was used.
The Controller build includes the previously recorded, uncommitted ACME scheduler
repair. This pass changes no production code or Edgion files.

The isolated runtime uses Center HTTP 16201/federation 16251, Vite 16173 and two
filesystem Controllers (`e2e-a/combined-a`, `e2e-b/combined-b`) on Admin ports
16301/16401. It has its own SQLite database, generated credentials, mTLS material
and 70 labeled seed files. Center database RBAC is enabled. Controller grants
enumerate the 22 fixture resource kinds and concrete verbs; no wildcard policy
is used. These test grants are not the default production policy. Existing
native and OrbStack environments were not modified.

## Results and fixture correction

- Unfiltered execution: **155 passed, one failed, two Kubernetes-only skips**
  in 9.1 minutes. The original process exited nonzero and its report is retained.
- The only failure was `actions-edgionacme`. The private test policy incorrectly
  granted `acme-trigger` on `EdgionAcme`. Current Controller
  `src/api/authz_classifier.rs` and `src/authz/access.rs` classify this operation
  as **Service/acme-trigger**. The UI correctly disabled the trigger.
- Corrected only that private policy row, restarted only this run's processes,
  and reran the unchanged ACME case: **two passed**, including authentication,
  in 14.2 seconds. No product authorization change was needed.
- All 156 applicable cases therefore have passing evidence across the two
  executions. This is **not** a claim that the unfiltered run exited zero.
  Its annotated case ledger remains 111 passed / one failed; the ACME retry
  report supplies the separate corrective evidence.
- All 22 generic CRUD cases passed, along with Controller reload convergence,
  user/role operations, audit controls, two-Controller RegionRoute flows,
  resource action coverage and focused route/WAF/ConfigData/LinkSys/AI forms.
- E2E TypeScript, ESLint, strict inventory (22 kinds, 268 actions, 224 generated
  cases) and diff whitespace checks passed. The first TypeScript invocation
  used a nonexistent config path; the canonical `npm run e2e:typecheck` passed.
  No frontend production code changed since the recorded 751-test/build baseline.

The two skips cover Kubernetes capabilities and dependency namespace isolation.
Earlier separate Kubernetes evidence still applies only to its recorded runtime;
this native run does not refresh that deployment or prove Gateway traffic.

## New batch deletion regression

`web/e2e/specs/resource-mutations.spec.ts` now creates three run-owned HTTPRoutes
through the real proxy API, waits for stable versions, selects two in the browser,
then searches until one selected row is hidden. Confirming batch deletion sends
both DELETE requests. API reads confirm both objects are absent and the unselected
third object still carries this run's label. Exact cleanup covers only successfully
created fixture names. This passed in 3.6 seconds and extends the earlier
confirmation-only browser evidence to actual authorized deletion.

## Artifacts and retention

Root: `/tmp/ws5-center-current-browser-20260928/`.

- `run.cjs`, `run.log`: isolated orchestration and original full-run output.
- `retry-acme.cjs`, `retry-acme.log`: correction and passing focused execution.
- `alignment-combined-20260928-1790558975982/`: original HTML report, case ledger,
  failure screenshot/trace/video, private runtime configs, fixture ledger and
  binary/source hashes in `provenance.json`.
- Its `acme-retry/` child contains the passing retry report and `result.json`.
- Build logs: `/tmp/ws5-center-current-browser-center-build.log` and
  `/tmp/ws5-center-current-browser-controller-build.log`.
- Static checks: `/tmp/ws5-center-current-browser-e2e-types.log` and
  `/tmp/ws5-center-current-browser-e2e-lint.log`.

Retention verification passed for all 70 labeled seed files (two changed by the
suite; original deletion hashes preserved). Temporary CRUD fixtures were cleaned
by their exact tests. Only processes started by this run were stopped, matching
the repository runner's process lifecycle. Files and reports remain available.
The overall alignment audit is still active.
