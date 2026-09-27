# Current alignment coverage

Checkpoint: 2026-09-28. The overall goal is **active, not complete**.
This is a current index; [README.md](README.md) retains the chronological evidence.
Older process handles and "next" notes there are historical unless revalidated.

## Scope and authority

The user requested the complete Center backend/dashboard review against current
sibling Edgion, resource by resource and menu by menu, without Edgion history.
Center commits are authorized; no push. Preserve unrelated work in both repos.
Current menu authority is `web/src/components/shell/menuConfig.tsx`; resource
authority is the sibling resource crate, mirrored by `web/src/config/resourceCatalog.ts`.
Center cloud integration is independent of Controller federation.

## Evidence levels

- **Catalog/contract**: source and adapter checks; not proof of runtime behavior.
- **Native CRUD**: browser actions and actual Controller/API readback.
- **Focused flow**: resource-specific browser or backend scenario described below.
- **Open**: missing or narrower evidence; never count it as completion.

The full standalone run `alignment-full-rbac-20260928-v2` passed 147 tests,
with one Kubernetes-only skip, including all 22 resource CRUD cases and 112
generated action cases. Its source predates the later fixes below; it is a
baseline, not proof that the final current tree passed the complete browser suite.
Log: `/tmp/ws5-center-full-rbac-native-v2.log`.

## Controller resource menus

All 22 rows have the native CRUD baseline above. Secret/ConfigMap deliberately
use the restricted dependency workflow rather than a general secret inventory.
No row implies complete nested-field coverage or data-plane conformance.

| Resource | Menu suffix | Focused alignment evidence beyond baseline |
|---|---|---|
| GatewayClass | infrastructure/gatewayclasses | Cluster scope; parameter reference; shared status handling |
| EdgionGatewayConfig | system/config | Current load-balancing controls, body/plugin policy, outbound TLS stripping |
| Gateway | infrastructure/gateways | Native and shared Controller status, listener counts/kinds/conditions; current listener TLS runtime exclusions |
| ReferenceGrant | infrastructure/referencegrants | Version boundary and topology authorization projection |
| HTTPRoute | routes/http | Backend AI references, mirror annotations, retry bounds, optional policy clearing; rule admission/provenance exclusions |
| GRPCRoute | routes/grpc | Method-only/service-only/header-only edits, last-match removal, policy clearing |
| TCPRoute | routes/tcp | v1 plus accepted alternate; stream-plugin/keepalive native edits |
| UDPRoute | routes/udp | v1 plus accepted alternate; stream-plugin native edits and TCP-control exclusion |
| TLSRoute | routes/tls | v1 plus accepted alternate; v2 Proxy Protocol, retries and keepalive native edits |
| Service | services/list | Immutable-field handling, zero weights, single/batch delete workflows |
| EndpointSlice | services/endpointslices | Native top-level addressType/endpoints/ports envelope retained |
| EdgionBackend | services/ai-backends | New resource, provider/credential/model editor, AI route and topology references |
| EdgionBackendTrafficPolicy | services/backend-traffic-policies | HTTPS probe editing, supported AI targets, unsupported AI controls, feature summary |
| EdgionTls | security/tls | Typed mTLS SANs and resolved-secret mutation boundary |
| BackendTLSPolicy | security/backendtls | Current identity/target restrictions and lossless form edits |
| Secret | security/dependencies | Metadata-only listing; explicit write controls; no global secret read model |
| ConfigMap | security/dependencies | Restricted dependency operations and exact replacement/readback |
| EdgionPlugins | plugins | 48 stage-plugin catalog entries; independent WAF form/count/references; mutation stripping |
| EdgionStreamPlugins | plugins/stream | Current connection/TLS-stage catalogs and stage-specific controls |
| EdgionConfigData | plugins/metadata | Nine typed variants; four new variants have dedicated native CRUD |
| LinkSys | system/linksys | All eight variants have dedicated native browser CRUD |
| EdgionAcme | system/acme | HTTP-01 scope; current renewal and notification boundaries |

Recent native logs supplement the baseline: `/tmp/ws5-center-waf-ui-native-v1.log`,
`/tmp/ws5-center-http-retry-native-v1.log`, `/tmp/ws5-center-grpc-match-native-v1.log`,
`/tmp/ws5-center-stream-annotations-native-v1.log` and `-v2.log`, and
`/tmp/ws5-center-route-policy-clear-native-v2.log`. The stream v1 run had one
test-locator failure; TLS passed its focused v2 rerun. Do not describe v1 as green.

## Center and shared menus

| Menu group | Verified behavior | Remaining evidence boundary |
|---|---|---|
| Controller dashboard, operations, topology | Native controls; AI nodes; freshness, polarity, stale/partial/conflict states; grant boundaries | Cross-resource runtime changes beyond the recorded scenarios |
| Center dashboard and Controllers | Registration, counts, proxy CRUD/CAS, watches, reload, offline/reconnect/resync | Deployed Kubernetes ownership/forwarding and additional freshness transitions |
| RegionRoute | Failover/restore, source-data sync, enable preservation, missing-target recovery; explicit write outcomes | Actual routed traffic and wider concurrent outcome scenarios |
| Global ConfigData inventory | Eight menu leaves; per-type visibility/redaction; native inventory checks | Additional multi-cluster unavailable/stale transitions |
| Provider accounts | Native create/edit, label retention, exact-generation conflict | Kubernetes dashboard capability/identity workflow |
| Cloudflare DNS and Route53 DNS | API DTO/form tests; sanitized read failure/recovery; lost-response uncertainty; 195 hermetic backend tests | New browser error states have component evidence, not native provider mutation evidence |
| Login, audit, users, roles | Native standalone password auth/logout, audit controls, administration, restricted permissions | OIDC and deployed Kubernetes capability workflows |

Native federation evidence: 27 lifecycle checks in
`/tmp/ws5-center-federation-native-v2.log` and 9 mTLS checks in
`/tmp/ws5-center-mtls-native-v3.log`. Cloud gate:
`/tmp/ws5-center-cloud-contract-tests.log` (195 passed; one real-account opt-in ignored).
Real cloud accounts are optional per `cicd/integration/README.md`.

## Current verification and environment

- Full frontend baseline: 590 tests in 100 files passed in
  `/tmp/ws5-center-frontend-full-current.log`, before the latest cloud fixes.
  Subsequent focused suites/build/lint cover those edits; no final-tree full
  frontend/browser regression is claimed yet.
- Two real Kubernetes adapter scenarios passed earlier: reconstruction/CAS and
  Lease takeover/fencing. This is not deployed OIDC, ServiceAccount RBAC or
  cross-replica forwarding proof.
- Center Kubernetes linux/arm64 image built with Rust 1.96.1 and locked deps;
  that staged image predates the newest frontend work. Its version smoke passed.
- The isolated kind API returned EOF at the last readiness check. Kubelet was
  restored after the bounded prewarm timed out. Current runtime work must first
  inspect the node, not assume an old handle is still running.
- Edgion image compilation finished, but packaging failed to find its configured
  release directory. No Edgion source change or commit was made for that failure.
- Existing OrbStack workloads and the user's other kind cluster remain untouched.

## Next audit actions

1. Continue the exact operator/runtime-field audit against all current resource
   structs. Catalog coverage alone is insufficient: the current pass found the
   missing ExtensionRef.resolvedNamespace, Gateway TLS resolution and HTTP rule
   admission/provenance exclusions (now fixed). Next inspect shared plugin
   conditions: SecretMatch.resolvedValues and IP-match resolvedIps are serialized
   by current Edgion but absent from the mutation exclusions. Verify every owning
   condition placement before choosing scoped filtering paths.
2. Finish capability/permission and asynchronous state transitions in Center
   menus, including OIDC and owner forwarding evidence. Keep native, component,
   hermetic transport and unavailable environment evidence distinct.
3. Run final current-tree frontend/backend gates and the complete native browser
   regression after the remaining fixes. Account for pre-existing repository
   guard failures explicitly; never hide them or delete unrelated files.
4. Reconcile every row with the original objective before claiming completion.
   Passing editor CRUD never proves Gateway traffic, real DNS propagation or
   a Kubernetes deployment that has not run.
