# Current frontend and native browser regression

Date: 2026-09-28. Overall alignment remains active.

Source: Center parent commit `296ea72` plus the BackendTLSPolicy target-column
presentation and two browser regression cases committed with this record.
No production source was changed while the suite ran. Existing current native
Center/Controller binaries from the prior combined regression were reused;
this pass changes no Rust code and is not a fresh backend build claim.

## Results

- Full frontend suite: **804 passed in 116 files**, 76.63 seconds.
- Production build, ESLint and E2E typecheck pass.
- Strict E2E inventory passes before runtime startup.
- Unfiltered standalone browser run: **173 passed, 2 skipped**, 9.7 minutes.
- Annotated case ledger: {'passed': 112}. The ledger covers annotated cases;
  it is not the total Playwright case count.
- Both skipped cases require Kubernetes: capability restrictions and dependency
  namespace scoping. Existing Kubernetes evidence remains separate.

This complete green run supersedes earlier standalone browser baselines,
including the combined run with a corrected ACME permission retry. The original
failed artifact remains intact. This runtime grants the actual Service/acme-trigger
permission and the unchanged ACME case passes in the full execution.

All 22 catalog CRUD workflows, resource menu actions, Center operations and
existing resource-specific flows pass. Two new browser cases import an optional
client certificate, clear it through Form, inspect YAML, submit, and read the
Controller API back. They verify omission of an empty options map and preservation
of an unrelated empty-valued option, target references and validation settings.
BackendTLSPolicy's target column now says Target backend and displays kind/name
and optional section, covering Service and EdgionBackend without conflating them.

## Artifacts and ownership

- Run: `alignment-latest-regression-20260928-1790562127063`.
- Root: `/tmp/ws5-center-latest-regression-20260928/` (run.log and private runtime).
- Frontend log: `/tmp/ws5-center-latest-frontend-full.log`.
- Gates: `/tmp/ws5-center-latest-build.log`, `-lint.log`, `-e2e-types.log`.
- The orchestrator exited zero and stops only its own four processes.
- 70 seed files remain; two changed as expected, with original deletion hashes
  retained. New mutation fixtures use exact cleanup. Other retained runtimes and
  unrelated working-tree changes were preserved.

This is native standalone UI/API evidence, not a fresh Kubernetes deployment,
provider mutation, or exhaustive data-plane conformance claim.
