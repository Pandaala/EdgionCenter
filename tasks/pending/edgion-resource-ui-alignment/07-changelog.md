# Changelog

## 2026-07-15

- Created isolated feature worktrees and branches for both repositories.
- Recorded the 21-resource baseline and complete delivery objective.
- Ran the clean frontend baseline: 33 tests, lint, and production build passed.
- Created the dedicated `edgion-resource-ui-e2e` namespace without touching
  existing namespaces.
- Added an initial lossless document helper, resource catalog, conditions
  component, permission-aware action foundation, and 13 focused passing tests.
- Paused UI-01 after independent review found missing data-channel, mutation,
  permission-discovery, cleanup, ledger, and E2E design details.
- Revised the design and workflow documents after two reviews; the third review
  accepted mutation/version/workflow design and requested final precision for
  field paths, self-access mounting, cleanup, and executable browser inventory.
- Closed those final findings; the third-review follow-up approved formal
  implementation with no remaining Must Fix. UI-00 is complete and UI-01 began.
- Implemented authenticated Controller `/api/v1/access`, bounded capability
  snapshots, Center proxy/action intersection, the 21-kind catalog, lossless
  mutation boundaries, recursive internal stripping, and shared conditions.
- Added safe update semantics: Kubernetes resourceVersion replace, filesystem
  same-resource serialization/unique temp writes, and etcd mod_revision CAS;
  all preserve server metadata/status without restoring runtime spec fields.
- Added ReferenceGrant single-resolution v1/v1beta1 URL/GVK projection and
  explicit discovery-error propagation.
- UI-01 review found and closed five concurrency/data-loss/stale-auth findings.
  Final gate: Controller 89 storage + 12 access tests, web 72 tests, Rust check,
  TypeScript, lint, production build, and diff checks passed. Independent
  follow-up approved with no Must Fix; UI-02 started.
- Completed UI-02 lossless adapters and mutation paths for ConfigData, ACME,
  Gateway/GatewayClass, all Route families, EdgionTls, BackendTLSPolicy,
  GatewayConfig, LinkSys, StreamPlugins, Secret, and ReferenceGrant.
- Removed every frontend `removeEmpty` caller, preserved multi-entry arrays and
  unknown operator fields, rejected Secret redaction placeholders, and added
  canonical plus explicit compatibility-version validation at the common
  mutation boundary.
- UI-02 final gate: full frontend 28 files/105 tests plus typecheck, lint, and
  build passed before the parallel next-phase edits; final boundary fixes passed
  4 files/26 focused tests. Independent follow-up approved with no Must Fix;
  UI-03 started.
- Implemented complete EdgionBackendTrafficPolicy types, lossless adapter,
  validation, Form/YAML editor, list CRUD/batch actions, access gating,
  conditions, navigation, and bilingual copy.
- UI-03 review findings were closed: disabling active health checks now
  preserves unknown siblings, health-check port accepts the Rust/CRD `0..65535`
  range, and invalid HTTP status drafts cannot be silently discarded across
  protocol switches. Final focused gate: 4 files/14 tests with real Form/YAML
  mutation payload coverage; UI-04 started.
- Completed HTTPRoute/GRPCRoute structured matches, all current filters,
  backend filters, policies, delegation, and ExternalAuth; completed TCP/UDP/TLS
  multi-rule/backends and the runtime-verified annotation matrix.
- UI-04 review findings closed narrow filter/path switching, redirect/rewrite
  conflicts, protocol retry/delegation rules, typed gRPC backends, real Editor
  Form/YAML submit tests, URI SAN preservation, and arbitrary JSON success
  predicates. Final follow-up approved with no Must Fix; UI-05 started.
- Completed Gateway v1.5 addresses/listeners/allowedRoutes/global TLS, complete
  GatewayClass parameters/status UX, and all current EdgionGatewayConfig Rust
  sections with lossless Form/YAML editing and shared submit validation.
- Fixed the companion Edgion CRD's ReferenceGrant default from true to false so
  Kubernetes matches standalone/etcd serde semantics, with a cross-mode Rust
  regression test. UI-05 review follow-up approved with no Must Fix; UI-06
  started.
- Completed all 39 current HTTP plugin variants with the exact four-stage
  eligibility/cardinality matrix, both flattened StreamPlugin stages, and all
  five ConfigData variants. Added lossless Form/YAML mutation boundaries,
  current RequestMirror fields, permissions, Conditions, and real submission
  coverage. Independent UI-06 review approved with no Must Fix; the full gate
  passed 46 files/178 tests plus typecheck, lint, build, and diff checks. UI-07
  and the non-overlapping topology/dashboard portion of UI-08 started in
  parallel.
- Completed UI-07 across LinkSys, BackendTLSPolicy, Service, EndpointSlice, and
  restricted Secret/ConfigMap dependencies. Independent review closed runtime
  secret write-back, same-namespace BackendTLS, safe type switching, access and
  batch gaps, and real request-test coverage. Restricted dependencies are a
  documented safe exception with metadata-key listing and write-only replace
  only. Final review approved with no Must Fix; 60 files/232 tests and all
  static/build gates passed.
- Completed UI-08 Conditions rollout, current-contract topology, fleet health
  summaries, diagnostics conflict projection, and multi-Controller drift
  comparison. Review corrections separated unavailable/unknown/missing states,
  fixed condition truth and ReferenceGrant tri-state semantics, completed
  explicit resource-reference edges, and hardened EndpointSlice fingerprints.
  Final review approved with no Must Fix; 60 files/234 tests and all gates
  passed. UI-09 runtime/harness corrections continue before OrbStack mutation.
- Completed UI-09 with the generated inventory fully implemented: 21 kinds,
  six state families, 213 actions, 132 cases, and 213 stable action selectors.
  The final standalone matrix passed 104/104 against exactly two Controllers;
  the final Kubernetes matrix passed 103/105 with only the two documented
  SQL-admin-only skips and no failures.
- Hardened Kubernetes namespace isolation end to end. Storage list/pagination,
  resource watchers, API discovery, DNS resolver Service/EndpointSlice scans,
  and ACME startup scans now fan out only across configured watch namespaces;
  AllNamespaces behavior remains explicit only for an empty namespace list.
  Regression tests pin namespace-specific Kubernetes request paths.
- Rebuilt and rolled both Controller deployments to
  `resource-ui-kubernetes-r6fix5_arm64`. With minimal RoleBindings, both remain
  Ready and online, ACME scans complete, DNS loops run without 403 errors, and
  neither service account can list Secrets cluster-wide.
- Completed UI-10. Full frontend, Center integration, Edgion Rust, repository
  policy, runtime, and diff gates passed, and all independent reviews approved
  with no unresolved blocker. Production dependency audit is clean; the
  pre-existing development-tool audit remains recorded separately.
- Retained the final OrbStack environment at `http://127.0.0.1:14180` with
  exactly two Controllers and 73 ledger-owned objects across four task
  namespaces. No commit or push was made.
- Re-ran the final closure after concurrency, RBAC, and coverage hardening.
  Conditional deletes now carry resourceVersion through the browser, Center,
  filesystem, etcd, and Kubernetes backends; filesystem rollback uses atomic
  no-clobber restoration and 409 guidance distinguishes creates from actions.
- Split Center Kubernetes privileges into a SAR-only ClusterRole plus a
  system-namespace runtime Role. The exact runtime inventory now owns and
  verifies 75 objects, with default-namespace Lease, Pod, and
  EdgionController access denied.
- Added runtime action-event evidence, all-kind Form/YAML/Conditions
  round-trips, direct Form→Conditions→YAML and YAML→Conditions→Form safety,
  UDP/TLS shared controls, and ACME stale-writer preservation oracles.
- Final current-source proof: Kubernetes `final-r4` passed 124 tests with two
  SQL-only skips; standalone `final-r10` passed 125 tests with one
  Kubernetes-only skip. Both case ledgers contain 108/108 unique passed cases.
  The 58 standalone files were cleaned exactly; the 75-object Kubernetes
  environment remains available at `http://127.0.0.1:14180`.
