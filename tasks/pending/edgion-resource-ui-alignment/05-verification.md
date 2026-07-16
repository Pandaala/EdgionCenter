# Verification and Evidence Plan

## Static and automated gates

| Gate | Command | Evidence artifact |
|---|---|---|
| TypeScript | `npx tsc --noEmit` | command log |
| Lint | `npm run lint` | command log |
| Unit/component | `npm test -- --run` | Vitest report |
| Production bundle | `npx vite build` | bundle log and size warnings |
| Preservation | resource adapter fixture suite | per-fixture operator projection diff |
| Center Rust | scoped workspace fmt/check/clippy/test | command log per crate |
| Edgion Rust | scoped fmt/check/clippy/test | command log per crate |
| Runtime integration | `cicd/integration/run-matrix.sh` plus scoped suites | matrix result |
| Repository policy | repository guard scripts | command log |
| Diff hygiene | `git diff --check` in both repositories | command log |

`accepted-alternate` fixtures mean only an API version the current Controller
explicitly accepts, such as BackendTLSPolicy v1alpha3 conversion. Removed
pre-release fields, annotations, and aliases are not treated as compatibility
requirements.

## Preservation fixture contract

Each resource adapter has stable fixture IDs:

- `P-<kind>-CURRENT`: every current operator field.
- `P-<kind>-ALT-<version>`: each accepted alternate API version.
- `P-<kind>-UNKNOWN`: a field accepted by the current Controller in an
  operator-owned preserve-unknown location but not modeled by the TypeScript
  form survives the frontend request.
- `P-<kind>-MULTI`: multiple rules/references/plugins survive a narrow edit.
- `P-<kind>-INTERNAL`: status, server metadata, resolved/parsed/redacted fields
  are absent from the mutation envelope.

The oracle compares operator-owned projections, not the full view response.

## Playwright browser suite

Add Playwright under `web/e2e`. Every case records:

```text
case ID, mode, Controller, page, fixture, action,
HTTP oracle, Controller/Kubernetes oracle, expected condition, artifact paths
```

Stable `data-testid` values identify every page-level action and every editor
submit/cancel/tab control. Per-resource case families are:

- `NAV-<kind>`: menu, route, controller switch, link targets.
- `LIST-<kind>`: load, search, namespace filter, age filter, refresh, pagination.
- `CRUD-<kind>`: create Form, create YAML, view, edit Form, edit YAML, delete.
- `BATCH-<kind>`: selection and batch action where supported.
- `STATUS-<kind>`: accepted, rejected, unresolved, conflict, parent/listener.
- `AUTH-<kind>`: advertised write, read-only policy, missing Center permission,
  older Controller with absent capability.
- `REL-<kind>`: topology and contextual navigation.
- `OP-<kind>`: resource-specific operations such as ACME trigger.

Pagination fixtures exceed the configured page size. Condition checks poll
`generation`/`observedGeneration` with a bounded deadline; fixed sleeps are
forbidden. Invalid cases distinguish:

1. malformed/YAML or deserialization rejection;
2. Kubernetes structural/admission rejection;
3. accepted storage followed by Controller `Accepted=False`.

Playwright retains HTML results, trace-on-first-retry, failure screenshots, and
the JSON case ledger under an ignored run-artifact directory. Final summarized
results are copied into the task changelog.

## Controller matrix

Both modes use two simultaneously connected Controllers with intentionally
different resource sets:

- Controller A watches `edgion-ui-e2e-a` and task-labeled cluster objects.
- Controller B watches `edgion-ui-e2e-b` and task-labeled cluster objects.
- Namespaced fixtures use the dynamic
  `metadata.labels.edgion.io/test-run=$E2E_RUN_ID`.
- Cluster-scoped fixtures use the same label and the `eruie2e-` name prefix.
- Policy fixtures include read-only default, explicit write grants, Secret deny,
  and a policy reload with revision change.

Applicable resources cover healthy, invalid, missing reference, denied
cross-namespace reference, permitted ReferenceGrant, oldest-wins conflict, and
read-only mutation denial.

## Runtime modes

### Standalone

- Build the standalone binary locally and copy it into dedicated OrbStack pods.
- Run two Controllers with isolated filesystem roots/resource sets.
- Seed the complete fixture ledger and run Playwright.

### Kubernetes

- Build local Center/Controller images and deploy only task-owned workloads.
- Use `edgion-resource-ui-e2e` for Center/runtime pods and the A/B namespaces
  for namespaced resource fixtures.
- Run the same Playwright case ledger against two connected Controllers.

## Cleanup safety

The seed tool writes a machine-readable cleanup ledger containing apiVersion,
kind, exact resource plural, namespace, name, run label, and creation phase for
every object. Inspection and cleanup enumerate that ledger, including all custom
resources; they never rely on `kubectl get all`. The cluster-scoped inventory
uses exact plurals, including `gatewayclasses.gateway.networking.k8s.io` and
`edgiongatewayclassconfigs.edgion.io`.

Namespaced cleanup deletes exact ledger objects, then the three task namespaces
only when the ledger proves they were created by the run. Cluster-scoped cleanup
requires both the exact ledger identity and matching run label/name prefix, then
proves absence. Never delete/reset a pre-existing namespace or use an unscoped
cluster-wide delete. Retain the final validated environment until the user
authorizes cleanup.

## Review gates

- Independent design review before formal implementation.
- Independent code review after each increment, with all Must Fix resolved.
- Coverage review states what is and is not covered.
- Final audit re-derives every objective requirement and maps it to current
  source, test, runtime, and browser evidence.

## Final evidence

Final closure rerun completed on 2026-07-16:

| Gate | Result |
|---|---|
| Frontend inventory | 21 kinds, 6 states, 240 actions, 216 cases, 240 implemented selectors; runtime action tests record actual click/input/change events |
| Frontend unit/component | 64 files, 260 tests passed |
| Frontend static/build | TypeScript, lint, and production build passed |
| Center workspace | fmt, all-target check, clippy with `-D warnings`, and all-target tests passed (33 + 1 + 33 + 163 + 6 + 12 + 119 + 45 tests) |
| Edgion workspace | fmt, all-target check/clippy, `cargo test --all`, and agent-doc validation passed; the six Controller documentation tests remain ignored by design |
| Standalone browser matrix | `resource-ui-standalone-final-r10`: 125 passed, 1 Kubernetes-only skip, 0 failed; 108/108 unique ledger cases passed against exactly two online Controllers |
| Kubernetes browser matrix | `resource-ui-kubernetes-final-r4`: 124 passed, 2 SQL-only skips, 0 failed; 108/108 unique ledger cases passed against exactly two online Controllers |
| Kubernetes least privilege | Center ClusterRole is SAR-only; runtime access is namespaced; default-namespace Lease, Pod, and EdgionController checks all deny |
| Kubernetes background services | ACME scans completed with one resource per Controller; DNS loops ran without `Forbidden` or tick failures |
| Independent reviews | Final frontend/coverage, conditional mutation security, and Kubernetes RBAC/ledger reviews approved with no blocker |
| Production dependency audit | `npm audit --omit=dev` reports 0 vulnerabilities |
| Development dependency audit | Baseline remains 2 moderate, 7 high, 1 critical; all are development-tool dependencies and were not changed in this feature |
| Diff hygiene | `git diff --check` passed independently in both repositories |

The final browser evidence is under
`web/test-results/resource-ui-kubernetes-final-r4/` and
`web/test-results/resource-ui-standalone-final-r10/`. Kubernetes `final-r4`
reuses the retained `resource-ui-kubernetes-final-r2` runtime identity, so its
case-ledger `runId` intentionally remains `resource-ui-kubernetes-final-r2`.
The cleanup ledger owns exactly 75 verified objects in the four
`eruie2e-86869049-*` namespaces. Standalone's 58 exact fixture files were
deleted after proof; the Kubernetes runtime is intentionally retained for user
inspection.
