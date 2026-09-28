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
The dependency namespace skip now has a separate passing Kubernetes execution:
`alignment-namespace-proxy-20260928` ran the strengthened repository case through
two mTLS Controllers and deployed Center, with both Secret and ConfigMap key
lists and explicit out-of-scope denial. Six additional browser checks cover both
menus/controllers and metadata-only permissions. Artifacts:
`/tmp/ws5-center-namespace-proxy-20260928/`. This does not replace the full
custom-resource Kubernetes matrix or its CRD-schema gate.

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
| EdgionGatewayConfig | system/config | Current load-balancing controls, body/plugin policy, outbound TLS stripping; list reads retry.attempts including zero and exposes failed reads/retry |
| Gateway | infrastructure/gateways | Native and shared Controller status, listener counts/kinds/conditions; current listener TLS runtime exclusions; protocol-specific TLS mode selection and Form/YAML submission guard |
| ReferenceGrant | infrastructure/referencegrants | Version boundary and topology authorization projection; actual cross-namespace HTTP backend name/source grant edits drive RefNotPermitted and 500/200 transitions |
| HTTPRoute | routes/http | Backend AI references, mirror annotations, retry bounds, optional policy clearing; rule admission/provenance exclusions; shared current hostname validation |
| GRPCRoute | routes/grpc | Method-only/service-only/header-only edits, last-match removal, policy clearing; shared current hostname validation |
| TCPRoute | routes/tcp | v1 plus accepted alternate; stream-plugin/keepalive native edits |
| UDPRoute | routes/udp | v1 plus accepted alternate; stream-plugin native edits and TCP-control exclusion |
| TLSRoute | routes/tls | v1 plus accepted alternate; v2 Proxy Protocol, retries and keepalive native edits; explicit SNI hostname requirement and current list bounds |
| Service | services/list | Immutable-field handling, zero weights, single/batch delete workflows; targetPort clearing preserves omission and restores actual Gateway traffic |
| EndpointSlice | services/endpointslices | Native top-level addressType/endpoints/ports envelope retained |
| EdgionBackend | services/ai-backends | New resource, provider/credential/model editor, AI route and topology references |
| EdgionBackendTrafficPolicy | services/backend-traffic-policies | HTTPS probe editing, supported AI targets, unsupported AI controls, feature summary; live HTTP/HTTPS/TCP/gRPC/GRPCS probes, HTTPS/GRPCS mTLS, service and certificate failure/recovery through Center forms |
| EdgionTls | security/tls | Typed mTLS SANs and resolved-secret mutation boundary |
| BackendTLSPolicy | security/backendtls | Current identity/target restrictions and lossless form edits |
| Secret | security/dependencies | Metadata-only listing; explicit write controls; no global secret read model; read failure/recovery components and live default-policy denial; direct Kubernetes Controller namespace filtering plus the real dual-Controller Center proxy E2E |
| ConfigMap | security/dependencies | Restricted dependency operations and exact replacement/readback; live Kubernetes Center metadata-only reads, SAR denial and recovery; direct Kubernetes Controller namespace filtering plus the real dual-Controller Center proxy E2E |
| EdgionPlugins | plugins | 48 stage-plugin catalog entries; independent WAF form/count/references; mutation stripping; edited/referenced access-policy body capability |
| EdgionStreamPlugins | plugins/stream | Current connection/TLS-stage catalogs and stage-specific controls |
| EdgionConfigData | plugins/metadata | Nine typed variants; four new variants have dedicated native CRUD |
| LinkSys | system/linksys | All eight variants have dedicated native browser CRUD |
| EdgionAcme | system/acme | HTTP-01 scope; renewal/notification boundaries; captured and permission-gated trigger, queue/uncertain outcomes, actual default denial and FS service-unavailable proof; shared lifecycle/UTC expiry display with failure recovery; real Kubernetes HTTP-01 issuance, Secret publication, recovery and renewal; Gateway hot-loads the replacement certificate without restart; scheduler-clear repair is verified locally in uncommitted Edgion changes |

All five active probe types (HTTP, HTTPS, TCP, gRPC and GRPCS) have Gateway traffic
proof, including Center form edits, Controller version readback and failure/
recovery. HTTPS and GRPCS mTLS cover absent, trusted and untrusted client
identities. See [HEALTH-POLICY-TRAFFIC-EVIDENCE.md](HEALTH-POLICY-TRAFFIC-EVIDENCE.md).
Broader TLS options and other resilience mechanisms remain open.

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
| RegionRoute | Failover/restore, source-data sync, enable preservation, missing-target recovery; explicit write outcomes; two-Controller east/west traffic, real partial denial and source-sync recovery; clear removes invalid empty target and writes exclude cached status | Concurrent operator races and other terminal outcomes |
| Global ConfigData inventory | Eight menu leaves; per-type visibility/redaction; native inventory checks; catalog/list recovery and expired-cursor reset components | Additional multi-cluster unavailable/stale transitions |
| Provider accounts | Native and real Kubernetes browser create/edit, label retention and exact-generation conflict; 52-account pagination; staged SAR permissions, denied-read recovery and credential-value rejection | Metadata-only proof; external credential inspection is separate |
| Cloudflare DNS and Route53 DNS | API DTO/form tests; sanitized read failure/recovery; lost-response uncertainty; 195 hermetic backend tests | New browser error states have component evidence, not native provider mutation evidence |
| Login, audit, users, roles | Native password auth/logout, administration/restricted permissions; real Dex login/logout; mixed-provider password-cookie cleanup; explicit logout failure/unavailable feedback; deployed Kubernetes SAR, disabled administration routes and proxy logout | Further permission transitions beyond the dual-Controller dependency namespace proof |

[RegionRoute traffic evidence](REGION-ROUTE-TRAFFIC-EVIDENCE.md) records the
reproduced false-convergence defect and native backend repair. Base behavior
differs from overlay behavior so fallback cannot masquerade as restoration.
Two independent Gateways now verify fan-out, permission-induced mixed outcomes
and recovery through source synchronization. The Kubernetes v5 image predates
this repair. Current source backend gates pass 876
workspace and 247 no-default-feature app tests; the matrix retains the unrelated
English-only failure described in the evidence.

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

The latest full frontend run passed 706 tests in 107 files, including ACME
lifecycle source precedence, shared observations, expiry conversion, stale-version
rejection and failed-read clearing. Log: `/tmp/ws5-center-acme-lifecycle-full.log`.
The final layout adjustment passed all five lifecycle tests again, plus build
and lint (`/tmp/ws5-center-acme-lifecycle-layout.log`, `-build-final.log`,
`-lint-final.log`). The bundle-size warning remains. E2E types and inventory
passed in the preceding passes; this repair changed no E2E types or selectors.

This supersedes the 701-test checkpoint. Four lifecycle browser checkpoints
supplement four ACME trigger checks and six runtime-status checks; injected
Ready/expiry and denial responses are explicitly distinguished from actual
native Pending and server-side trigger denial in ACME-MENU-EVIDENCE.md.
The full native browser regression above predates the later frontend fixes;
focused browser evidence supplements it without upgrading that run's scope.

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

See [ACME-MENU-EVIDENCE.md](ACME-MENU-EVIDENCE.md) for the trigger repair and
the lifecycle/expiry repair and real Kubernetes issuance/renewal proof. The
[ACME scheduler clear repair](ACME-SCHEDULER-CLEAR-GAP.md) now passes actual
recovery, renewal and replacement-certificate hot loading with the locally rebuilt
Controller. The Edgion repair remains uncommitted; no CRD was changed.

Gateway configuration list follow-up: two component tests and three native
browser checkpoints pass for current retry values and list failure/recovery.
Evidence: `/tmp/ws5-center-gatewayconfig-list-20260928/`. This is additional
focused coverage, not a new full-suite count.

## Next audit actions

The native list-status gap is now repaired across 17 existing status columns.
[STATUS-SOURCE-GAP.md](STATUS-SOURCE-GAP.md) records current implementation and
live denial/recovery evidence. Continue the remaining per-resource and menu
audits below; this focused repair does not complete those audits.


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


Metadata/topology request-target follow-up: restricted dependency keys and all
22 topology inventory reads now bind both cache and HTTP requests to the captured
route Controller. 38 focused tests, build/lint and two native browser checks pass.
See [READ-TARGET-EVIDENCE.md](READ-TARGET-EVIDENCE.md). Existing Secret denial is
preserved; this adds request-isolation evidence, not new inventory permissions.


Pagination recovery follow-up: stale cursors now reset the exact active list and
restart page one. The full frontend suite passed 716 tests in 110 files; four
pagination regressions passed again after tightening the persistent-error guard.
Two browser checks recover injected expired pagination to actual native rows.
See [PAGINATION-RECOVERY-EVIDENCE.md](PAGINATION-RECOVERY-EVIDENCE.md).


EndpointSlice readiness follow-up: topology now matches current Gateway discovery,
which requires explicit ready=true. Seven actual flow/page checks cover missing,
null, false, serving-only, true and terminating+true states plus restoration.
29 focused tests and build/lint pass. See
[ENDPOINT-READINESS-EVIDENCE.md](ENDPOINT-READINESS-EVIDENCE.md); the fixture writes
are external harness setup, not evidence of default Center write permission.


Service targetPort follow-up: clearing the optional form value now omits the field
instead of sending an invalid empty string. 13 focused tests, build/lint and the
actual Center save/readback/Gateway recovery scenario pass. Temporary update
permission was revoked. See [SERVICE-PORT-EVIDENCE.md](SERVICE-PORT-EVIDENCE.md).


Legacy list-action follow-up: eleven active resource menus now use the shared
concrete-kind/verb permission controls, including batch actions. Full frontend
suite: 737 tests in 111 files. Twelve native checkpoints verify actual default
read-only and explicit ACME write policies without mutations. See
[LIST-ACTION-PERMISSIONS-EVIDENCE.md](LIST-ACTION-PERMISSIONS-EVIDENCE.md) for
empty-list limits and the Gateway annotation-hint corrections.


Batch-selection follow-up: nine resource lists retain selected rows hidden by
search; thirteen report the submitted count rather than live selection size.
Controller navigation now clears page selection/editor state. Full frontend:
751 tests / 113 files, build/lint passed; two native UI checkpoints passed without
mutations. See [BATCH-SELECTION-EVIDENCE.md](BATCH-SELECTION-EVIDENCE.md).
