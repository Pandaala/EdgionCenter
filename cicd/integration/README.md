# Integration matrix

Run the hermetic matrix from the repository root:

```sh
cicd/integration/run-matrix.sh
```

It covers both binaries, all workspace targets, the Kubernetes-free standalone
dependency boundary, the SQL-free Kubernetes dependency boundary, strict
Clippy/formatting, the no-default-feature application build, rendered manifests,
frontend tests/build, repository guards, and diff hygiene.

`kubectl` is required for the offline Kustomize render. Minimal local
environments may opt out explicitly with `EDGION_SKIP_KUBECTL=1`; the runner
prints the skipped gate instead of silently reducing coverage. The client
dry-run performs API discovery, so it runs only with the explicit real-cluster
opt-in and never touches the user's current context during the default matrix.

## Scenario ownership

| Scenario | Hermetic coverage | Real-system coverage |
|---|---|---|
| Standalone SQLite persistence, restart fencing, auth, and Admin API | SQL adapter, app, and standalone suites | Workspace matrix |
| Standalone MySQL ordering and joins | Environment-gated adapter tests | `EDGION_TEST_MYSQL_URL` |
| Kubernetes restart reconstruction | Mock API projection tests | Fresh adapter reads the real CRD after projection |
| Invalid CRD, Lease, and SAR RBAC | Kubernetes binary preflight tests | Deployment readiness must remain false when the runtime Role is reduced |
| Kubernetes audit boundary | Structured stdout and SQL audit contract tests | Runtime logs plus kube-apiserver audit policy |
| Lease expiry, fencing, same-holder reconnect, and takeover | Lease/registry/runtime tests | Two coordinators take over one real Lease after expiry |
| Multi-replica command/proxy routing | Owner locator and internal-forwarding tests | Deployment uses two replicas and the internal mTLS Service |
| Active-active global reads | Capability/directory API tests | Fresh replica reconstructs reads without a local federation registry |

## Real kube-apiserver matrix

Use a disposable namespace in a test cluster. Install the checked-in CRD and
RBAC first, then opt in explicitly:

```sh
kubectl apply -k cicd/deploy/center-kubernetes
export EDGION_TEST_KUBERNETES=1
export EDGION_TEST_KUBERNETES_NAMESPACE=edgion-system
cargo test -p edgion-center-adapter-kubernetes --test real_cluster -- --nocapture
```

The test creates uniquely named namespaced Controller and Lease objects, proves
that a fresh directory instance reconstructs state, exercises expiry/takeover
with two replica identities, verifies the stale holder cannot release the new
fence, and removes its resources. It never installs or deletes cluster-scoped
resources. If the opt-in variable is absent, it prints a clear skip message and
performs no external mutation.

To exercise RBAC denial in a deployed cluster, remove one permission at a time
(`edgioncontrollers`, `edgioncontrollers/status`, `leases`, `pods`, or
`subjectaccessreviews`) from a disposable deployment. The process must fail its
startup preflight or remain unready; restore the checked-in Role before running
the takeover matrix.

## External MySQL matrix

The SQL adapter owns environment-gated MySQL round trips for controller fencing,
audit persistence, and user/role joins:

```sh
export EDGION_TEST_MYSQL_URL='mysql://user:password@127.0.0.1:3306/edgion_center_test'
cargo test -p edgion-center-adapter-sql -- --nocapture
```

Use a disposable database. The hermetic SQLite suite always runs, so an absent
MySQL URL is a documented skip rather than a silent loss of all SQL coverage.
