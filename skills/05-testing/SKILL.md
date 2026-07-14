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

Never point the Kubernetes matrix at a shared or production namespace. See
`cicd/integration/README.md` for setup, assertions, and cleanup guarantees.
