---
name: center-testing
description: Hermetic and external integration matrices for both EdgionCenter modes.
---

# Testing

Run the default, non-destructive matrix from the repository root:

```sh
cicd/integration/run-matrix.sh
```

It runs formatting, workspace Clippy with warnings denied, all Rust tests, the app's
no-default-feature build, dependency-purity checks, offline manifest rendering, frontend
lint/tests/build, policy guards, and `git diff --check`.

External fixtures are opt-in:

- `EDGION_TEST_MYSQL_URL=...` enables SQL-adapter MySQL tests.
- `EDGION_TEST_KUBERNETES=1` plus a disposable
  `EDGION_TEST_KUBERNETES_NAMESPACE` enables real API-server CRD/Lease tests.
- `EDGION_FEDERATION_E2E=1` runs the two-controller federation e2e
  (`examples/test/scripts/integration/run_center_test.sh`): real Center +
  Controller binaries over mTLS, covering registration/StatsReport counts,
  watch-cache reads, proxied CRUD with CAS, the default-RBAC surface, failover
  fan-out and row sync, terminal reload outcomes, disconnect/reconnect/eviction,
  a >4 MiB proxied list, and the terminal watch denial. It needs the sibling
  Edgion repo (override the path with `EDGION_DIR`). The companion
  `run_center_mtls_test.sh` covers SPIFFE identity rejection and the plaintext
  fail-close; both scripts document, in their headers, which scenarios are
  deliberately left to unit tests and why.
  The federation runner refuses occupied ports, stops only its child processes,
  and retains its private temporary directory for inspection on success or
  failure. It never calls the broad kill_all utility. File-system Controllers
  set conf_center.controller_name explicitly, as current startup requires.

Never point the Kubernetes matrix at a shared or production namespace. See
`cicd/integration/README.md` for setup, assertions, and cleanup guarantees.
