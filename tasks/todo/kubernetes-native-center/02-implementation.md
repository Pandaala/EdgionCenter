# Implementation and Test Strategy

## Increment strategy

Implement through independently verifiable increments. Keep the existing `edgion-center` binary as a compatibility path until the standalone replacement passes parity checks.

### Increment 1: Workspace and library seam

- Add a virtual workspace section while retaining the root package temporarily.
- Add `crates/center-core` and move the first platform-neutral domain type (`AuthzMode`) into it.
- Convert the current root package from binary-only to library plus thin compatibility binary.
- Preserve the current CLI, wire behavior, features, and tests.

Verification:

- `cargo fmt --all -- --check`
- `cargo check --workspace --all-targets`
- `cargo test --workspace --all-targets`
- `cargo clippy --workspace --all-targets`
- existing repository guards

Result (2026-07-14): complete. Workspace check, tests, Clippy, and repository
guards pass. The repository-wide formatting check still reports pre-existing
formatting differences outside this increment; every Rust file added or edited
by KN-01 passes a scoped `rustfmt --check` invocation.

### Increment 2: Core ports

- Add controller, audit, authorization, coordination, and capability domain contracts.
- Adapt existing types without changing runtime selection.
- Add pure unit tests for identifiers, capability resolution, and error semantics.

Result (2026-07-14): complete. The core crate now owns controller-directory,
audit, authorization, coordination, capability, identifier, and error contracts.
Compatibility adapters exercise the current SQL controller projection and
authorization resolver, including stale-session fencing and idempotent eviction.

### Increment 3: Shared runtime extraction

- Move federation, API, aggregation, watch, proxy, commander, common auth, and observability into `center-runtime`.
- Keep SQL implementation connected through the new ports.
- Prove the existing integration scripts still address the compatibility binary.

Progress (2026-07-14): created `center-runtime` and moved the metadata
aggregation store plus generic watch cache into it as the first vertical slice.
The compatibility crate temporarily re-exports these modules, preserving all
call sites while establishing the final dependency direction. Remaining
federation, API, aggregation, proxy, command, authentication, and observability
modules still need extraction before this increment is complete.

### Increment 4: Standalone composition

- Move SQL persistence and DB management into `center-adapter-sql`.
- Add the `edgion-center-standalone` package and standalone configuration.
- Run behavior parity and image tests.

### Increment 5: Kubernetes composition

- Implement CRDs, SubjectAccessReview authorization, Lease coordination, controller projection, and stdout runtime audit.
- Add the `edgion-center-kubernetes` package and Kubernetes deployment/RBAC manifests.
- Add capability-driven API and dashboard behavior.

### Increment 6: Multi-replica completion

- Implement owner-aware internal forwarding for commands and proxied requests.
- Verify takeover, Lease expiry, stale ownership, rolling restart, and active-active reads.

## Coverage requirements

- Preserve all existing unit tests throughout extraction.
- Add contract tests that run the same controller-directory and authorization behavior against each adapter where semantics overlap.
- Test unsupported capabilities explicitly; never rely on an `Option` panic or accidental 503.
- Add Kubernetes API tests for CRD name collision resistance, status conflicts, SAR deny/failure behavior, and Lease ownership.
- Keep external MySQL and real Kubernetes tests environment-gated with clear skip output.

## Rollback boundaries

Each increment must leave the compatibility binary working. Do not remove an old module until its replacement is compiled, tested, and wired. Avoid simultaneous module moves plus behavior changes when they can be separated.
