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

Kubernetes preflight checks served API identities and compares every cataloged
Edgion custom-resource schema against the selected Controller checkout. Comparison
includes descriptive metadata and is independent of object key order. API discovery
alone is insufficient: an old schema can prune current fields or reject status
updates. The preflight is read-only and refuses drift; use an isolated cluster with
current CRDs instead of overwriting a shared cluster's contracts. Gateway API and
built-in resources currently receive discovery checks only.

The context guard accepts `orbstack` or exactly `kind-eruie2e-<hash>`, where hash
is the first eight hexadecimal characters of SHA-256 of `E2E_RUN_ID`. Other kind
clusters and names derived from a previous run are refused by seed, reset and
cleanup as well as preflight. For an isolated cluster, create it explicitly with
that name and a private kubeconfig (from `web/`):

```bash
umask 077
mkdir -p "$E2E_ARTIFACT_DIR"
prefix="eruie2e-$(printf %s "$E2E_RUN_ID" | shasum -a 256 | cut -c1-8)"
export KUBECONFIG="$E2E_ARTIFACT_DIR/kubeconfig"
export E2E_KUBE_CONTEXT="kind-$prefix"
kind create cluster --name "$prefix" --kubeconfig "$KUBECONFIG"
```

Install current Edgion/Gateway API and Center CRDs in that owned cluster before
running preflight. The runner imports locally built Center/Controller images with
`kind load docker-image`. It also requires free loopback ports 14180 and 5556;
the latter forwards Dex, while Chromium resolves only the run's Dex hostname to
loopback. The OIDC issuer, TLS hostname and in-cluster URLs remain unchanged.
The additional forwarding process is owned by the runner and stopped on exit;
the cluster and labeled resources are retained. See the
[kind quick start](https://kind.sigs.k8s.io/docs/user/quick-start/) for cluster and
image-loading commands.

The Kubernetes authorization suite also verifies native-RBAC/password-login
capabilities, hidden SQL administration menus, direct-route redirects, and rejected
user/role/audit API requests. Unsupported management scenarios must not be evidenced
solely by skipped standalone tests.

Standalone permission-denial coverage uses `E2E_RBAC=1` to enable database
password login and RBAC in the isolated runtime. Supply `EDGION_ADMIN_USERNAME`
and `EDGION_ADMIN_PASSWORD` matching `E2E_USERNAME` and `E2E_PASSWORD` so first-run
bootstrap grants the harness administrator its role. Use a new run ID/database.
The default remains single-admin authentication with `allow_all`; the restricted
user lifecycle case skips that configuration. The RBAC case creates a roleless
user, verifies management/proxy denials and password/status changes, and deletes
that exact user in cleanup. It does not test revocation of already-issued tokens.

Cleanup is always explicit:

```bash
E2E_MODE=kubernetes E2E_ALLOW_MUTATION=1 E2E_RUN_ID=... E2E_ARTIFACT_DIR=... \
  e2e/scripts/cleanup.sh
```

Cleanup refuses context, run-label, UID, static-inventory, or resource-plural mismatches. It
prints the ledger first, uses UID-preconditioned API deletes, deletes exact children before
Namespaces, and proves every identity is absent. It never uses namespace-wide or selector-wide
deletion.

The standalone cloud-account case is metadata-only: it stores an unresolved
credential reference, never inspects credentials or calls a DNS provider, and
checks browser edits against real generation preconditions. Provider-account
deletion is not exposed, so the uniquely named account remains only in the
run-owned artifact database alongside retained fixtures.

Standalone file seeds include an initial resourceVersion because direct file
installation bypasses Admin API creation. Controller assigns subsequent versions.
Unversioned manually installed files remain ineligible for Center CAS writes.
The retain check allows changed content with the exact run label and keeps the
original seed hashes; explicit file deletion still refuses modified fixtures.


State-case API checks understand both native conditions and the Kubernetes
status.controllers envelope, preserving each writer's native condition groups.
When metadata.generation is present, a condition only satisfies the expected
state if observedGeneration equals it. Missing or older observation versions
cannot prove current convergence. Generation-less standalone resources remain
supported. The pure observation helper has independent fixture tests.
