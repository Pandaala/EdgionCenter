# Context

## Requirement summary

- Problem: Edgion evolved faster than the Center dashboard, leaving one missing
  first-class resource, incomplete forms, stale status/navigation, and editors
  that can destroy current fields.
- Expected behavior: complete resource management and diagnostics against at
  least two Controllers in standalone and Kubernetes Center modes.
- Out of scope: unrestricted Secret plaintext browsing, compatibility with
  fields or versions the current Controller no longer accepts, and cleanup of
  any pre-existing OrbStack environment.

## Repository baselines

| Repository | Base commit | Isolated branch | Worktree |
|---|---|---|---|
| Edgion | `39fbe4754520f63f7e7ba88ffcec0f17286cb8d1` | `feature/center-resource-ui-contract` | `/Volumes/ExtStore/ws5/Edgion-resource-ui` |
| EdgionCenter | `7084bcee84c4917aa817536a7b4de1eeb1d12f52` | `feature/edgion-resource-ui-alignment` | `/Volumes/ExtStore/ws5/EdgionCenter-resource-ui` |

The original Edgion worktree was clean. The original EdgionCenter worktree had
user-owned changes under `tasks/`; the isolated Center worktree was created from
the committed base and does not contain or modify those changes.

## Related skills

- Workspace `skills/edgion-workspace/SKILL.md`.
- Edgion resource feature and architecture skills.
- Edgion `skills/07-tasks/new_feature_work_flow.skill` and companions.
- EdgionCenter architecture, feature, review, and testing skills.
- Center web architecture, editor, type/utility, resource, and testing skills.

## Affected modules

- Edgion Controller authorization discovery and resource skills.
- Center selected-Controller HTTP proxy contract tests.
- Dashboard API types, adapters, editors, lists, shell, i18n, status,
  authorization, topology, dashboard, and E2E suite.

## Baseline evidence

Executed from the isolated Center worktree on 2026-07-15:

| Command | Result | Notes |
|---|---|---|
| `npm test -- --run` | pass | 10 files, 33 tests |
| `npm run lint` | pass | zero warnings |
| `npm run build` | pass | existing large-chunk warning |
| `npm ci` audit | warning | 10 dependency findings: 2 moderate, 7 high, 1 critical; no forced lockfile mutation |
| `git diff --check` | pass | task documents only at baseline |

The test suite emits existing Ant Design deprecation and JSDOM
`getComputedStyle` noise while still passing. These are baseline observations,
not evidence of new failures.

The npm audit findings predate this branch. This task will not force-upgrade
unrelated dependencies because that can change dashboard behavior outside the
resource work. The final report re-runs the audit and lists any dependency this
feature changes; remaining findings stay recorded as accepted baseline risk and
are not represented as fixed.

## Environment inventory

- Kubernetes context: `orbstack`.
- Pre-existing namespaces treated as user-owned: `default`, `edgion-system`,
  `edgion-test`, `kube-system`, `kube-public`, and `kube-node-lease`.
- Existing `edgion-test` contains `controller-1`, `controller-2`, and two Center
  replicas; these are read-only baseline references until the dedicated runtime
  matrix is deployed.
- Dedicated namespace created: `edgion-resource-ui-e2e`.
- Dedicated fixture namespaces planned: `edgion-ui-e2e-a` and
  `edgion-ui-e2e-b`.
- Task label: `edgion.io/test-run=$E2E_RUN_ID`, with a unique run ID generated
  for every execution and recorded in the cleanup ledger.
- Cluster-scoped fixture name prefix: `eruie2e-`.

## Initial risks

- Rust contains runtime fields that are serializable but are not operator CRD
  fields; blindly preserving the full API response would leak them into updates.
- Controller federation policy is not currently discoverable per verb/kind.
- Secret reads are denied by default and must not be inferred from page code.
- Cluster-scoped fixtures survive namespace deletion and require exact cleanup.
- The existing dashboard has no repeatable browser E2E framework.
