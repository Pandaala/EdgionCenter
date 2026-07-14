# Subtasks

| ID | Status | Deliverable |
|---|---|---|
| KN-01 | done | Cargo workspace, `center-core`, library seam, compatibility binary |
| KN-02 | done | Core domain types, narrow ports, capabilities, and contract tests |
| KN-03 | in-progress | Shared `center-runtime` extraction |
| KN-04 | pending | SQL adapter extraction and `edgion-center-standalone` binary |
| KN-05 | pending | Kubernetes CRDs and controller directory adapter |
| KN-06 | pending | Kubernetes SubjectAccessReview authorizer |
| KN-07 | pending | Kubernetes Lease coordination and ownership state machine |
| KN-08 | pending | `edgion-center-kubernetes` binary and manifests |
| KN-09 | pending | Capability-driven Admin API and dashboard behavior |
| KN-10 | pending | Owner-aware internal forwarding for command/proxy operations |
| KN-11 | pending | Standalone and Kubernetes integration matrices |
| KN-12 | pending | Separate images, CI, docs, skills, and compatibility-binary retirement |

## KN-01 acceptance

- Root Cargo invocation recognizes a workspace.
- `center-core` has no Axum, Tonic, SQLx, or Kube dependencies.
- Current application code is compiled as a library.
- `edgion-center` is a thin compatibility binary with unchanged CLI behavior.
- Existing tests and repository checks pass.

KN-01 completed on 2026-07-14. Validation: `cargo check --workspace
--all-targets`, `cargo test --workspace --all-targets`, `cargo clippy
--workspace --all-targets`, both repository guards, `git diff --check`, and a
scoped rustfmt check for all touched Rust files.

## KN-02 acceptance

- Platform ports do not expose Axum, Tonic, SQLx, or Kube types.
- Controller and session identifiers reject empty and control-character values.
- Authorization denial remains distinct from adapter failure.
- Standalone capabilities and Kubernetes capabilities are explicit.
- The existing SQL controller projection is covered by stale-session fencing,
  offline transition, and idempotent eviction contract tests.
- The existing allow-all and explicit-permission authorization behavior is
  covered through the new `Authorizer` contract.

KN-02 completed on 2026-07-14. Targeted core and compatibility contract tests,
workspace check, Clippy, and the full repository suite pass.

## KN-03 progress

- Added the `center-runtime` workspace crate with no SQLx or Kube dependency.
- Moved `metadata_store` and `watch_cache` into the runtime crate.
- Moved `aggregator` into the runtime crate and separated protobuf input from
  process-specific metrics emission.
- Moved the effective-state poller behind a `ControllerHttpClient` runtime port;
  the existing `ProxyForwarder` now acts as its compatibility adapter.
- Moved the federation protobuf source/build and session registry into the
  runtime crate, retaining production metrics through an injected hook.
- Moved command dispatch and Controller HTTP proxy forwarding into runtime;
  missing-controller tests verify fail-fast behavior and pending-map cleanup.
- Moved SPIFFE certificate parsing and Controller identity matching into the
  runtime federation module with all real-DER tests preserved.
- Moved federation metrics and bounded-label validation into runtime
  observability.
- Replaced the federation server's concrete SQL `Store` dependency with the
  `ControllerDirectory` port and ordered registration before offline projection.
- Moved the federation server and transport configuration into `center-runtime`;
  the compatibility crate now only re-exports the server module.
- Added a test-support feature so cross-crate federation tests can inspect cache
  state without exposing those helpers in production builds.
- Preserved the compatibility package through temporary module re-exports.
- Closed the first independent review findings: removed the runtime's transitive
  Kube dependency, added immediate takeover cancellation, persisted SQL session
  fencing/revisions, bounded projection latency, and reset stale standalone
  ownership during startup.
- Workspace total is 309 passing tests: 214 compatibility, 5 core, and 90
  runtime tests. Two compatibility-only clock helper tests were removed when the
  helper became private federation runtime behavior.
