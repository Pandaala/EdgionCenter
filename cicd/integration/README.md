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
| Multi-replica proxy routing | Owner locator and internal-forwarding tests | Deployment uses two replicas and the internal mTLS Service |
| Active-active directory reads | Capability/directory API tests | Fresh replica serves directory-backed reads without a local federation registry |

## Hermetic cloud matrix

The default workspace test gate is the cloud integration gate; it needs no cloud credentials and
must not contact the internet. Provider coverage is split deliberately:

| Boundary | Hermetic coverage |
|---|---|
| Cloudflare DNS HTTP | Wiremock status/body/header fixtures, bounded response streaming, throttling metadata, one-shot mutation dispatch, and lost-response ambiguity |
| Route 53 DNS and hosted-zone lifecycle | Deterministic SDK HTTP fixtures, revision and authority fencing, throttling, partial observations, mutation receipts, deletion guards, and post-dispatch failure injection |
| Admin API | Capability-gated route mounting, exact permission inventory, request body limits, sanitized errors, and provider-independent dashboard contracts |

Real-account tests remain optional verification only. They must use disposable resources and
explicit environment opt-in; their absence does not reduce or skip the hermetic gate above.

## Real kube-apiserver matrix

Use a disposable namespace in a test cluster. These adapter tests use your
kubeconfig identity directly; they do not require a Center Deployment or its
runtime ServiceAccount. The identity needs access to Controller and provider
account CRDs (including Controller status), and namespaced Leases.

Pin a test-cluster context in a private kubeconfig, then install only missing
CRDs. If either CRD already exists, inspect its schema before reusing it; do not
overwrite an existing cluster contract just to run a test.

```sh
set -e
umask 077
export CENTER_TEST_CONTEXT=orbstack # Replace with your test-cluster context.
fixture_dir=$(mktemp -d)
kubectl --context "$CENTER_TEST_CONTEXT" config view --minify --raw > "$fixture_dir/kubeconfig"
export KUBECONFIG="$fixture_dir/kubeconfig"
# On a test cluster where these CRDs are absent:
kubectl create -f cicd/deploy/center-kubernetes/crd.yaml
kubectl create -f cicd/deploy/center-kubernetes/provider-account-crd.yaml
kubectl wait --for=condition=Established --timeout=45s \
  crd/edgioncontrollers.center.edgion.io crd/edgionprovideraccounts.center.edgion.io
export EDGION_TEST_KUBERNETES=1
export EDGION_TEST_KUBERNETES_NAMESPACE="center-integration-$(date +%Y%m%d%H%M%S)"
kubectl create namespace "$EDGION_TEST_KUBERNETES_NAMESPACE"
cargo test -p edgion-center-adapter-kubernetes --test real_cluster -- --nocapture
```

The tests create uniquely named namespaced Controller, provider account, and
Lease objects. They verify provider account reconstruction and generation CAS,
Controller status/resourceVersion updates without spec-generation changes,
fresh-directory reconstruction, Lease expiry/takeover with two replica identities,
and rejection of a stale holder's release. They remove their own resources; the
namespace, CRDs, and private kubeconfig remain for inspection. The tests themselves
never install or delete cluster-scoped resources. If the opt-in variable is absent,
they print a clear skip message and perform no external mutation.

This proves adapter behavior against an API server, not deployed Center
authentication, replica forwarding, or runtime ServiceAccount permissions.

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
