# Executable Playwright Plan

## Harness contract

Add `@playwright/test` and `tsx` as development dependencies and these package scripts:

```json
{
  "e2e:install": "playwright install chromium",
  "e2e": "playwright test",
  "e2e:standalone": "E2E_MODE=standalone playwright test",
  "e2e:kubernetes": "E2E_MODE=kubernetes playwright test",
  "e2e:inventory": "tsx e2e/scripts/check-inventory.ts"
}
```

Committed layout:

```text
web/playwright.config.ts
web/e2e/global-setup.ts
web/e2e/auth/standalone.setup.ts
web/e2e/auth/kubernetes.setup.ts
web/e2e/fixtures/{resources,policies}/
web/e2e/pages/
web/e2e/specs/{navigation,resources,operations,authorization,topology}.spec.ts
web/e2e/support/{api-oracle,k8s-oracle,case-ledger}.ts
web/e2e/scripts/{seed,reset,run,check-inventory}.{sh,ts}
web/e2e/action-inventory.json
web/e2e/case-ledger.schema.json
web/e2e/cleanup-ledger.schema.json
```

Required environment variables are `E2E_MODE`, `E2E_BASE_URL`,
`E2E_USERNAME`, `E2E_PASSWORD`, `E2E_CONTROLLER_A`, `E2E_CONTROLLER_B`,
`E2E_RUN_ID`, and `E2E_ARTIFACT_DIR`; Kubernetes additionally requires
`KUBECONFIG`. `E2E_RUN_ID` defaults to `resource-ui-<UTC timestamp>-<pid>` and
is used in labels, names, artifact paths, and the cleanup ledger.

## Start and authentication

- Standalone runs `cargo build -p edgion-center-standalone`, starts
  `target/debug/edgion-center-standalone --config-file e2e/runtime/standalone.yaml`,
  starts `npm run dev -- --host 127.0.0.1 --port 15173 --strictPort`, and polls
  `http://127.0.0.1:12201/api/v1/auth/status` plus
  `http://127.0.0.1:15173/login`. Its setup project
  fills the normal username/password form and saves storage state.
- Kubernetes runs `cicd/build-image.sh --mode kubernetes -t edgion-center-kubernetes:$E2E_RUN_ID`,
  then runs `../Edgion-resource-ui/cicd/build-image.sh --version "$E2E_RUN_ID"`
  and uses the resulting `pandaala/edgion-controller:$E2E_RUN_ID` image. It
  applies `e2e/runtime/kubernetes/*.yaml`, waits with
  `kubectl rollout status` for the exact task deployments, then runs
  `kubectl -n $E2E_NAMESPACE port-forward service/eruie2e-oauth2-proxy 14180:80`.
  It polls `http://127.0.0.1:14180/api/v1/auth/status`. Its setup project follows
  the Dex redirect, signs in
  with the E2E user, verifies the callback, and saves a separate storage state.
- Credentials are environment-only and are never written into fixtures, traces,
  snapshots, or the case ledger.

The committed Kubernetes fixture includes a task-local Dex static client,
oauth2-proxy callback `http://127.0.0.1:14180/oauth2/callback`, an E2E user loaded
from a run-owned Secret, Center ServiceAccount/Role/RoleBinding, and two distinct
Controller configs. Setup asserts the callback host and authenticated `/auth/me`
before saving storage state. Process IDs and port-forward PID files live only in
the run artifact directory; traps terminate and wait for those exact PIDs.

`e2e/scripts/run.sh standalone` owns the exact build/start/port checks and then
runs `npm run e2e:standalone`; `e2e/scripts/run.sh kubernetes` owns image build,
task manifest apply, oauth2 port-forward, readiness, and
`npm run e2e:kubernetes`. Both install signal traps that stop only processes they
started and leave the validated Kubernetes objects intact. CI calls the same two
entry points, so local and CI orchestration cannot drift.

## Seed, reset, and oracles

`seed.sh` creates namespaces and all resources from templates after substituting
the run ID. It writes every created object identity to `cleanup-ledger.json`.
`reset.sh` restores only objects in that ledger. Before each mutation case, the
API oracle reads the selected Controller and records generation/spec; after the
action it polls the API for the expected operator projection and condition.
Kubernetes mode additionally reads the exact GVK/name/namespace with kubectl and
checks status/protected metadata. Deadlines are bounded and fixed sleeps are not
used as correctness oracles.

## Complete action inventory

Every interactive control receives a stable `data-testid` and a row in
`action-inventory.json`. The inventory checker scans routes/components for test
IDs and fails on an undocumented or unimplemented action.

| Page/route | Required `data-testid` actions |
|---|---|
| `/login` | `login-username`, `login-password`, `login-submit`, `login-error` |
| global shell | `nav-toggle`, `user-menu`, `logout`, `controller-selector`, `controller-search`, `controller-A`, `controller-B`, `controller-empty` |
| `/dashboard` | `dashboard-refresh`, `dashboard-reload`, `count-<kind>` for all catalog kinds, `conflicts-link`, `unhealthy-link`, `acme-expiry-link` |
| `/controller/:id/user` and direct `/user` | `user-refresh`, `user-resource-summary`, `user-error-retry` |
| each `/resources/<kind>` | `<kind>-refresh`, `-search`, `-namespace`, `-age`, `-prev`, `-next`, `-create`, `-row-view`, `-row-edit`, `-row-delete`, `-select`, `-select-all`, `-batch-delete` when catalog-enabled |
| each create/edit route | `editor-form-tab`, `editor-yaml-tab`, `editor-submit`, `editor-cancel`, `editor-validation`, `editor-dirty-confirm`, and `<json-path>-add/-remove/-up/-down` for every repeated field declared by its adapter |
| Gateway/detail | `listener-expand`, `listener-condition-expand`, `gateway-related-route`, `gateway-related-tls` |
| Route/detail | `parent-expand`, `parent-condition-expand`, `route-related-gateway`, `route-related-service`, `route-ref-denied` |
| BackendTLSPolicy/EBTP detail | `ancestor-expand`, `target-service-link`, `policy-conflict-link` |
| EdgionAcme detail | `acme-trigger`, `acme-trigger-confirm`, `acme-trigger-cancel`, `acme-operation-status` |
| `/region-routes` | `region-refresh`, `region-override-link`, `region-failover`, `region-failover-confirm`, `region-failover-cancel` |
| `/controllers` and detail | `controllers-refresh`, `controller-row-open`, `controller-delete`, `controller-delete-confirm`, `controller-delete-cancel` |
| `/admin` | `admin-refresh`, `admin-controller-open`, `admin-controller-delete`, `admin-delete-confirm`, `admin-delete-cancel`, `admin-permission-denied` |
| `/audit` | `audit-refresh`, `audit-user-filter`, `audit-action-filter`, `audit-resource-filter`, `audit-time-filter`, `audit-prev`, `audit-next`, `audit-row-expand`, `audit-export` |
| `/users` | `users-refresh`, `user-create`, `user-edit`, `user-enable`, `user-disable`, `user-reset-password`, `user-delete`, and each confirmation/cancel control |
| `/roles` | `roles-refresh`, `role-create`, `role-edit`, `role-permission-toggle`, `role-delete`, `role-confirm`, `role-cancel` |
| `/global-connection-ip-restrictions` | `gcir-refresh`, `gcir-search`, `gcir-namespace`, `gcir-controller`, `gcir-row-open`, `gcir-prev`, `gcir-next` |
| `/global-connection-ip-restrictions/:namespace/:name/:controllerId` | `gcir-detail-refresh`, `gcir-profile-select`, `gcir-configdata-link`, `gcir-condition-expand`, `gcir-back` |
| `/topology` | `topology-refresh`, `topology-fit`, `topology-zoom-in`, `topology-zoom-out`, `topology-kind-filter`, `topology-namespace-filter`, `topology-node`, `topology-edge` |
| dependency dialogs | `configmap-create`, `configmap-replace`, `configmap-reference`, `secret-create`, `secret-replace`, `secret-reference`, `secret-redacted` |
| access/capability failure panel on every Controller page | `access-error`, `access-retry`, `access-readonly-explanation` |

All 21 catalog resources inherit the applicable resource-list/editor rows. The
catalog explicitly declares exceptions: read-only resources hide mutation and
batch controls; cluster-scoped resources omit namespace filtering; singleton or
restricted dependencies expose only their declared operations.

## Matrix and case ledger

The same generated cases run for Controller A and B in standalone and Kubernetes
modes. Authorization variants are default policy, explicit read-only, explicit
write, absent Center `proxy:access`, hot reload, and old Controller 404. Resource
states are accepted, malformed/rejected, unresolved, ReferenceGrant-denied,
ReferenceGrant-allowed, and conflict. Each result validates against
`case-ledger.schema.json` and records mode, controller, route, action, fixture,
HTTP status, API/Kubernetes oracle, condition, duration, and artifact paths.

`e2e/scripts/generate-cases.ts` takes the resource catalog, adapter path ledger,
and `action-inventory.json`, expands the two modes/two Controllers plus declared
authorization/state variants, validates the result, and writes
`expected-cases.json`. `case-ledger.schema.json` requires `schemaVersion`,
`runId`, `startedAt`, `finishedAt`, and `cases`; every case requires unique `id`,
`mode`, `controller`, `page`, `fixture`, `actionTestId`, `expectedHttp`,
`expectedOracle`, `status`, `durationMs`, and `artifacts`. The reporter performs
an exact ID-set comparison between expected and actual cases; duplicates,
missing cases, or unexpected skips fail the run.

Local and CI runs return non-zero for failed tests, missing inventory entries,
schema-invalid ledgers, missing expected cases, seed/reset failure, leaked
task-owned objects, console errors not allowlisted, or missing traces/screenshots
for failures. HTML report, traces, screenshots, network summaries, and JSON
ledger are stored beneath the run artifact directory.

## Cleanup ledger

Cleanup enumerates every namespaced and cluster-scoped identity created by seed;
it never relies on `kubectl get all`. The generated ledger includes apiVersion,
kind, resource plural, namespace, name, label, and ownership phase. Cleanup first
prints all entries, deletes exact entries, and proves each is absent. The CRD
plural for EdgionGatewayConfig is
`edgiongatewayclassconfigs.edgion.io`. Final successful environments remain until
the user authorizes cleanup.

The committed `cleanup-kind-map.json` is the authoritative 21-kind map:

| Kind | Fully qualified resource plural |
|---|---|
| GatewayClass | `gatewayclasses.gateway.networking.k8s.io` |
| EdgionGatewayConfig | `edgiongatewayclassconfigs.edgion.io` |
| Gateway | `gateways.gateway.networking.k8s.io` |
| HTTPRoute | `httproutes.gateway.networking.k8s.io` |
| GRPCRoute | `grpcroutes.gateway.networking.k8s.io` |
| TCPRoute | `tcproutes.gateway.networking.k8s.io` |
| UDPRoute | `udproutes.gateway.networking.k8s.io` |
| TLSRoute | `tlsroutes.gateway.networking.k8s.io` |
| Service | `services` |
| EndpointSlice | `endpointslices.discovery.k8s.io` |
| EdgionTls | `edgiontls.edgion.io` |
| ReferenceGrant | `referencegrants.gateway.networking.k8s.io` |
| BackendTLSPolicy | `backendtlspolicies.gateway.networking.k8s.io` |
| EdgionPlugins | `edgionplugins.edgion.io` |
| EdgionStreamPlugins | `edgionstreamplugins.edgion.io` |
| EdgionConfigData | `edgionconfigdata.edgion.io` |
| EdgionAcme | `edgionacmes.edgion.io` |
| LinkSys | `linksys.edgion.io` |
| EdgionBackendTrafficPolicy | `edgionbackendtrafficpolicies.edgion.io` |
| Secret | `secrets` |
| ConfigMap | `configmaps` |

`fixture-inventory.json` statically lists at least one fixture per row plus the
three Namespaces, Center/Controller Deployments, Services, ServiceAccounts,
Roles, RoleBindings, ClusterRoles, ClusterRoleBindings, ConfigMaps, Secrets,
Dex Deployment/Service, and
oauth2-proxy Deployment/Service. Seed may create only identities present in this
inventory and appends each server-returned UID to the run ledger.

`cleanup-ledger.schema.json` requires `schemaVersion`, `runId`, `context`,
`createdAt`, and non-empty `objects`; each object requires `apiVersion`, `kind`,
`resource`, `scope`, `name`, `runLabel`, `uid`, and `phase`, with `namespace`
required exactly when `scope=Namespaced`. Cleanup refuses a context mismatch,
run-label mismatch, missing or mismatched UID, unknown resource plural, or an
object absent from the static inventory. It deletes children before namespaces
and deletes cluster objects only by exact resource/name plus UID precondition.

Normal development runs fail on leaked ledger objects after an explicit cleanup
request. The final acceptance command uses `E2E_RETAIN_ENV=1`; it verifies every
ledger object still has the expected run label/UID, records `retained=true`, and
does not treat intentional retention as a leak. Seed/reset failure stops new
actions, writes the partial ledger atomically, and prints the exact recovery
command; it never performs broad best-effort deletion.
