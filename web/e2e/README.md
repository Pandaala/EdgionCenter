# Resource UI Playwright harness

Static validation is non-mutating and does not require a browser:

```bash
npm run e2e:inventory
npm run e2e:typecheck
E2E_MODE=mock npx playwright test e2e/specs/mock-static.spec.ts
```

`e2e:inventory` validates the 22-kind catalog/fixture/cleanup contract, six resource states,
two modes, two Controller slots, action inventory, and case expansion. Set
`E2E_INVENTORY_STRICT=1` once UI action test IDs have landed to fail on every missing selector.

Runtime entry points are `e2e/scripts/run.sh standalone` and `run.sh kubernetes`. They require
environment-only credentials and Controller IDs, create a unique run artifact directory, and
stop only PIDs they started. Kubernetes mutations require `E2E_ALLOW_MUTATION=1`, which
`run.sh` sets after establishing the run ID. Successful runs call the retain path: they verify
the exact run label and UID of every ledger object and leave the environment intact.

Both modes use the current sibling `Edgion/` checkout. Set `EDGION_DIR` to an absolute
checkout path to use another Controller source. The standalone runner builds and starts
that checkout's Controller binary and copies its current CRDs into each fixture directory.
For repeated frontend-only runs after building both native programs, set
`E2E_SKIP_BUILD=1` in standalone mode to reuse those binaries without acquiring Cargo
build locks. Rebuild whenever either program's source changes. The runner checks that
both executables exist before creating runtime configuration.

Cleanup is always explicit:

```bash
E2E_MODE=kubernetes E2E_ALLOW_MUTATION=1 E2E_RUN_ID=... E2E_ARTIFACT_DIR=... \
  e2e/scripts/cleanup.sh
```

Cleanup refuses context, run-label, UID, static-inventory, or resource-plural mismatches. It
prints the ledger first, uses UID-preconditioned API deletes, deletes exact children before
Namespaces, and proves every identity is absent. It never uses namespace-wide or selector-wide
deletion.
