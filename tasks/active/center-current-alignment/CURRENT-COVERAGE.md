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

The latest complete resource run `alignment-full-current-20260928-v3` passed
155 tests with two Kubernetes-only skips (capabilities and dependency namespace
scope). Session 20509 exited zero; all 112 annotated ledger cases passed,
including all 22 generic resource CRUD cases, plus the focused routes/WAF/
ConfigData/LinkSys scenarios. Both native binaries were freshly built from
current source; the frontend corresponds to commit 712ed0f (later commits before
this run completed only changed backend test code and evidence documentation).
Log: `/tmp/ws5-center-full-current-native-v3.log`.
Artifacts: `web/test-results/alignment-full-current-20260928-v3/`.
The older 147-test run remains historical evidence, superseded by this run.
The subsequent authentication repair has separate current browser/component
evidence below; the full resource run predates that repair.

## Controller resource menus

All 22 rows have the native CRUD baseline above. Secret/ConfigMap deliberately
use the restricted dependency workflow rather than a general secret inventory.
No row implies complete nested-field coverage or data-plane conformance.

| Resource | Menu suffix | Focused alignment evidence beyond baseline |
|---|---|---|
| GatewayClass | infrastructure/gatewayclasses | Cluster scope; parameter reference; shared status handling |
| EdgionGatewayConfig | system/config | Current load-balancing controls, body/plugin policy, outbound TLS stripping |
| Gateway | infrastructure/gateways | Native and shared Controller status, listener counts/kinds/conditions; current listener TLS runtime exclusions; protocol-specific TLS mode selection and Form/YAML submission guard |
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
| EdgionPlugins | plugins | 48 stage-plugin catalog entries; independent WAF form/count/references; mutation stripping; edited/referenced access-policy body capability |
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
| Center dashboard and Controllers | Registration, counts, proxy CRUD/CAS, watches, reload, offline/reconnect/resync; component read-failure/recovery states; deployed Kubernetes owner/non-owner reads, writes, CAS, connection migration and old-fence revocation | Pod crash takeover, post-dispatch transport faults and additional freshness transitions |
| RegionRoute | Failover/restore, source-data sync, enable preservation, missing-target recovery; explicit write outcomes | Actual routed traffic and wider concurrent outcome scenarios |
| Global ConfigData inventory | Eight menu leaves; per-type visibility/redaction; native inventory checks; catalog/list recovery and expired-cursor reset components | Additional multi-cluster unavailable/stale transitions |
| Provider accounts | Native and real Kubernetes browser create/edit, label retention and exact-generation conflict; 52-account pagination; staged SAR permissions, denied-read recovery and credential-value rejection | Metadata-only proof; external credential inspection is separate |
| Cloudflare DNS and Route53 DNS | API DTO/form tests; sanitized read failure/recovery; lost-response uncertainty; 195 hermetic backend tests | New browser error states have component evidence, not native provider mutation evidence |
| Login, audit, users, roles | Native password auth/logout, administration/restricted permissions; real Dex login/logout; mixed-provider password-cookie cleanup; explicit logout failure/unavailable feedback; deployed Kubernetes SAR, disabled administration routes and proxy logout | Dependency namespace workflow and further permission transitions |

Native federation evidence: 27 lifecycle checks in
`/tmp/ws5-center-federation-native-v2.log` and 9 mTLS checks in
`/tmp/ws5-center-mtls-native-v3.log`. Cloud gate:
`/tmp/ws5-center-cloud-contract-tests.log` (195 passed; one real-account opt-in ignored).
Real cloud accounts are optional per `cicd/integration/README.md`.

## Current verification and environment

- Full frontend baseline: 590 tests in 100 files passed in
  `/tmp/ws5-center-frontend-full-current.log`, before the latest cloud fixes.
  Superseded for frontend unit/component coverage by the 625-test run below.
  The complete native browser regression is recorded above (155 passed).
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

The current backend matrix passed fmt, clippy, workspace tests, app tests with
no default features (242), dependency isolation and kustomize. Its final exit was
1 at the pre-existing English-only violation in root
`fix-issue-workflow-generic.zh.md`; the subsequent legacy guard was run separately
and passed. See `/tmp/ws5-center-current-backend-matrix-v2.log`. External MySQL,
Kubernetes and federation stages were not opted into that matrix. A 10ms wall-clock
Cloudflare test race was fixed and passed both focused and matrix runs.

## Latest complete frontend suite

At Center commit `4c774c2`, session 81433 completed successfully: 101 test files,
625 tests, including the recent Gateway/HTTP/plugin mutation boundaries, access
policy body edits, Controller read-state fixes and GlobalResources recovery.
Retained log: `/tmp/ws5-center-frontend-final-20260928.log`.
The earlier 622-test run remains at
`/tmp/ws5-center-frontend-full-20260928-current.log`.
This supersedes the earlier 590-test frontend baseline for these changes. It is
not native browser, backend compilation or deployed Kubernetes evidence.

The dedicated kind node was rechecked in the same pass: its API still returns
EOF, `crictl ps -a` has no control-plane containers, and containerd's main thread
is blocked in `wait_for_partner -> fifo_open -> openat` while repeated container
creation requests report reserved names. This is stronger evidence than the
previous assumption of slow image unpacking. The node services remain running;
no unrelated cluster was modified and no runtime restart was attempted.

## Source-to-coverage reconciliation

The current source audit confirmed exact equality between Edgion ResourceKind
(excluding Unspecified) and the 22 Center catalog kinds. All 22 have passing
native CRUD ledger entries; all 20 first-class catalog routes appear in both the
Controller menu and this index. Secret/ConfigMap share the restricted dependency
menu. The 48 HTTP plugin names match the current Edgion enum exactly. All nine
ConfigData variants and all eight LinkSys variants have operator form support;
stream catalogs match the four connection-stage and one TLSRoute-stage variants.

The backend matrix covers OIDC token/identity validation, SAR identity scoping,
lease fencing and owner forwarding/no-ambiguous-replay behavior. These checks do
not prove a real browser OAuth redirect/callback flow or deployed cross-replica
operation. The original work log also retains open cross-resource traffic,
attachment, probe and failure scenarios. Consequently this reconciliation proves
catalog/menu completeness but does not close the overall task.

The real OAuth browser path now passes against an isolated native Center plus
the same pinned Dex and oauth2-proxy versions used by the Kubernetes fixture.
The original six login/session checks led to a reproduced logout defect, now
fixed with configured same-origin proxy logout and explicit failure handling.
Current browser checks prove proxy-cookie deletion and 401 after logout, plus
cleanup of a coexisting Center password cookie. The existing repository shell
test passes through both password and OIDC entry points. Login bootstrap also
accepts an existing OIDC session when password login is enabled. These native
checks do not establish Kubernetes SAR or replica behavior. See
[OIDC browser evidence](OIDC-BROWSER-EVIDENCE.md).

Authentication repair gates: 635 frontend tests in 102 files; focused auth tests,
build/lint, E2E types/inventory; backend workspace and no-default-feature tests,
Clippy, formatting, dependency purity and manifest rendering. The full backend
matrix still exits at the unrelated tracked `fix-issue-workflow-generic.zh.md`
English-only guard; the later no-legacy guard passes when run separately.

Current provider-account menu evidence is in
[PROVIDER-ACCOUNT-KUBERNETES-EVIDENCE.md](PROVIDER-ACCOUNT-KUBERNETES-EVIDENCE.md).
The current Vite dashboard against the deployed v5 backend passed real Kubernetes
CRUD/CAS, pagination beyond 50 accounts, and permission-revocation recovery.
Frontend checks now pass 642 tests, build/lint, E2E types and inventory. All
provider grants were removed after verification; 52 metadata-only CRDs remain
in the owned namespace. The embedded v5 image predates these frontend changes.

## Next audit actions

Current deployed Kubernetes authentication proof is recorded in
[KUBERNETES-AUTH-EVIDENCE.md](KUBERNETES-AUTH-EVIDENCE.md). Two OrbStack Center
replicas were validated with v4, then upgraded to v5, with canonical OAuth sidecars and a private Dex
fixture. Nine live RBAC checks, five browser scenarios, and all three selected
repository E2E cases passed. The discovered disabled-route blank page is fixed;
635 frontend tests, build, lint and E2E type checks pass. The isolated kind API
remains unavailable; existing shared Gateway CRDs were not changed.
[Replica forwarding evidence](KUBERNETES-FORWARDING-EVIDENCE.md) now proves real
owner/non-owner reads and CAS writes, default Controller RBAC across the hop,
connection migration and old-fence revocation. It exposed and fixed normal
session cancellation retaining a valid ownership flag after Lease release.
The v5 deployment passes 22 proxy/fencing checks, two permission-restoration
checks and all three selected browser cases. Backend validation passed 873
workspace tests, 244 no-default-feature app tests, format/Clippy/check and
adapter/manifest gates; the existing English-only baseline failure remains.
The temporary proxy grants were removed. Pod crash takeover and transport
failures after dispatch are not established by connection migration alone.

1. Continue the exact operator/runtime-field audit against all current resource
   structs. Catalog coverage alone is insufficient: the current pass found the
   missing ExtensionRef.resolvedNamespace, Gateway TLS resolution and HTTP rule
   admission/provenance exclusions (now fixed). Shared plugin conditions now
   exclude SecretMatch.resolvedValues and IP-match resolvedIps in all four
   stage trees. Continue nested configuration and operator editor coverage;
   serialization checks do not establish placement-specific runtime acceptance.
2. Finish capability/permission and asynchronous state transitions in Center
   menus, including OIDC and owner forwarding evidence. Keep native, component,
   hermetic transport and unavailable environment evidence distinct.
3. Current frontend/backend gates and the complete native browser regression
   are recorded above. Rerun affected checks after further fixes; retain the
   explicit pre-existing English-only guard limitation.
4. Reconcile every row with the original objective before claiming completion.
   Passing editor CRUD never proves Gateway traffic, real DNS propagation or
   a Kubernetes deployment that has not run.
