# Current native browser regression

Source: Center `ba012f3`, with no tracked implementation changes during the run.
Date: 2026-09-28.

## Runtime and provenance

Both native programs were rebuilt from the current worktrees before startup:

- Center: `cargo build -p edgion-center-standalone`, passed;
  `/tmp/ws5-center-current-browser-build.log`.
- Controller: `cargo build -p edgion-controller`, passed;
  `/tmp/ws5-controller-current-browser-build.log`. The local ACME and global
  outbound client identity repairs remain uncommitted in Edgion.

The private runner `/tmp/ws5-current-browser-run.py` uses the repository's TLS
generation, runtime rendering, standalone fixture seeding, Playwright suite and
exact-ledger retain verification. It remaps listener ports in the generated
configs and starts Vite programmatically with matching API/probe proxies, so
the earlier retained runtime is left intact.

The run ID is `alignment-current-20260928-final`; artifacts are under
`/tmp/ws5-center-browser-current-20260928`. Generated credentials/configuration
are private and are not copied into this document. The isolated composition has
SQL RBAC/password login, two filesystem Controllers, current copied CRDs and
real mTLS federation. Center listens on Admin `17201`, probe `17200`, federation
`17251` and metrics `17290`; the browser uses Vite `17573`. Controller A uses
`17300/17301/17351/17390`, and B uses `17400/17401/17451/17490`.

The harness waits for authenticated enumeration of exactly the two expected
online Controllers before starting Playwright. It stops only the child
processes it created, retaining the database, fixtures, logs and browser
artifacts. No Gateway is started by this browser harness.

## Validation

- Full frontend: **848 passed / 117 files**, 84.70 seconds;
  `/tmp/ws5-center-current-final-frontend.log`.
- Strict inventory: **22 kinds, 6 states, 268 actions, 224 cases and 268
  implemented selectors**, passed in the native run log.
- E2E TypeScript: passed; `/tmp/ws5-center-current-browser-types.log`.
- Browser: **174 passed, 2 skipped**, 9.7 minutes;
  `/tmp/ws5-center-browser-current-20260928.log`. Both skips are Kubernetes-only
  capability/namespace cases. The standalone restricted-user case ran and passed.
- Retain verification: **70 exact standalone files verified**, including two
  intentionally changed files with original deletion hashes preserved. The runner
  exited zero; all four owned processes exited and their Admin/Vite ports were free.

## Evidence boundary

The complete browser suite exercises actual Center-to-Controller reads and
mutations, including API readback, both Controller slots, resource editors,
restricted dependencies, RBAC, reload, global resources, RegionRoute controls,
topology and metadata-only cloud accounts. Some fault cases deliberately inject
HTTP/browser failures; those assertions retain their narrower scope.

This is standalone browser/API evidence. It does not prove Gateway traffic,
external DNS provider mutation, Kubernetes replica crash takeover or a fresh
Kubernetes deployment. Those remaining requirements stay in COMPLETION-AUDIT.md.
