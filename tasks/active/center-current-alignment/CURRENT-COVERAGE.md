# Current alignment coverage

Checkpoint: 2026-09-28. The overall goal is **active, not complete**.
This is a current index; [README.md](README.md) retains the chronological evidence.
[COMPLETION-AUDIT.md](COMPLETION-AUDIT.md) reconciles the objective and remaining work.
Older process handles and "next" notes there are historical unless revalidated.

## Latest complete regression

At Center `ba012f3`, the full frontend passes **848 tests / 117 files** and the
complete native standalone browser suite passes **174 tests with two
Kubernetes-only skips**. Center and Controller were rebuilt, the isolated runtime
used SQL RBAC and real mTLS federation, and all 70 retained fixture files were
verified. See [CURRENT-NATIVE-REGRESSION.md](CURRENT-NATIVE-REGRESSION.md).
This supersedes older full frontend/browser counts below; their feature-specific
evidence remains useful. Subsequent traffic and fault proofs are linked below;
the completion audit identifies the remaining requirements.

Current Kubernetes follow-up: the task deployment now runs the locally rebuilt
`ws5-alignment-current-36d771d` image. Three OIDC/capability browser checks and
ten forced owner-Pod recovery checkpoints pass, including new survivor ownership,
CAS readback and authorization restoration. See
[CURRENT-KUBERNETES-FAULT-EVIDENCE.md](CURRENT-KUBERNETES-FAULT-EVIDENCE.md).
Ambiguous post-dispatch response loss now also passes eight native checkpoints,
including actual Controller write readback and exactly one observed PUT dispatch.
See [AMBIGUOUS-WRITE-TRAFFIC-EVIDENCE.md](AMBIGUOUS-WRITE-TRAFFIC-EVIDENCE.md).

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

The previous unfiltered standalone regression passed **173 tests with two
Kubernetes-only skips**, and its frontend checkpoint passed **835 tests in 117
files** (after subsequent focused repairs). Build, lint, E2E types and strict inventory also pass. All 22 CRUD cases
and both new client-certificate clearing browser cases pass in the same execution.
See [latest regression evidence](LATEST-REGRESSION-EVIDENCE.md) for the exact
source, runtime and artifact scope. This supersedes the historical standalone
baselines below; their failure/retry records remain intact.

The earlier fully passing unfiltered resource run `alignment-full-current-20260928-v3` passed
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
That run predates subsequent authentication and shared-list repairs. The current
combined regression at production commit `daf70d3` ran all 158 standalone cases:
155 passed, one temporary ACME grant error failed, and two Kubernetes-only cases
skipped. Correcting the private grant to Service/acme-trigger made the unchanged
ACME case and its authentication setup pass. All 156 applicable cases have passing
evidence across these executions; the original full-run failure is retained,
not relabeled as a successful gate. All 22 CRUD cases and the new actual filtered
batch deletion case passed. See
[COMBINED-BROWSER-EVIDENCE.md](COMBINED-BROWSER-EVIDENCE.md) for exact scope,
artifact paths and the distinction between the original ledger and retry.

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
| EdgionBackend | services/ai-backends | New resource, provider/credential/model editor, AI route and topology references; real provider endpoint form edits, model alias resolution and credential Secret rotation through Center |
| EdgionBackendTrafficPolicy | services/backend-traffic-policies | HTTPS probe editing, supported AI targets, unsupported AI controls, feature summary; live HTTP/HTTPS/TCP/gRPC/GRPCS probes, HTTPS/GRPCS mTLS, service and certificate failure/recovery through Center forms |
| EdgionTls | security/tls | Typed mTLS SANs and resolved-secret mutation boundary; Gateway attachment form, optional reference clearing and native API readback |
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
AI backend endpoint/credential changes and attached concurrency limits now also
have real Gateway traffic evidence, including form edits and policy deletion.
Two isolated 12-checkpoint runs pass; see
[AI-BACKEND-TRAFFIC-EVIDENCE.md](AI-BACKEND-TRAFFIC-EVIDENCE.md). This covers
selected concurrency resilience beyond active probes, not every resilience
mechanism or provider protocol. Gateway frontend TLS transitions now also pass
seven real handshake matrices covering form mode changes, port override
clearing/deletion, default inheritance and CA rotation/restoration. See
[FRONTEND-TLS-TRAFFIC-EVIDENCE.md](FRONTEND-TLS-TRAFFIC-EVIDENCE.md).

Recent native logs supplement the baseline: `/tmp/ws5-center-waf-ui-native-v1.log`,
`/tmp/ws5-center-http-retry-native-v1.log`, `/tmp/ws5-center-grpc-match-native-v1.log`,
`/tmp/ws5-center-stream-annotations-native-v1.log` and `-v2.log`, and
`/tmp/ws5-center-route-policy-clear-native-v2.log`. The stream v1 run had one
test-locator failure; TLS passed its focused v2 rerun. Do not describe v1 as green.

## Center and shared menus

| Menu group | Verified behavior | Remaining evidence boundary |
|---|---|---|
| Controller dashboard, operations, topology | Native controls; AI nodes; freshness, polarity, stale/partial/conflict states; grant boundaries | Cross-resource runtime changes beyond the recorded scenarios |
| Center dashboard and Controllers | Registration, counts, proxy CRUD/CAS, watches, reload, offline/reconnect/resync; component read-failure/recovery states; deployed Kubernetes owner/non-owner reads, writes, CAS, connection migration and old-fence revocation | Current-image Pod crash takeover and post-dispatch response loss now have native proof linked above; other freshness transitions are outside those scenarios |
| RegionRoute | Failover/restore, source-data sync, enable preservation, missing-target recovery; explicit write outcomes; two-Controller east/west traffic, real partial denial and source-sync recovery; clear removes invalid empty target and writes exclude cached status | Concurrent operator races and other terminal outcomes |
| Global ConfigData inventory | Eight menu leaves; per-type visibility/redaction; native inventory checks; catalog/list recovery and expired-cursor reset components | Additional multi-cluster unavailable/stale transitions |
| Provider accounts | Native and real Kubernetes browser create/edit, label retention and exact-generation conflict; 52-account pagination; staged SAR permissions, denied-read recovery and credential-value rejection | Metadata-only proof; external credential inspection is separate |
| Cloudflare DNS and Route53 DNS | API DTO/form tests; sanitized read failure/recovery; lost-response uncertainty; 195 hermetic backend tests | New browser error states have component evidence, not native provider mutation evidence |
| Login, audit, users, roles | Native password auth/logout, administration/restricted permissions; real Dex login/logout; mixed-provider password-cookie cleanup; explicit logout failure/unavailable feedback; deployed Kubernetes SAR, disabled administration routes and proxy logout | Further permission transitions beyond the dual-Controller dependency namespace proof |

[RegionRoute traffic evidence](REGION-ROUTE-TRAFFIC-EVIDENCE.md) records the
reproduced false-convergence defect and native backend repair. Base behavior
differs from overlay behavior so fallback cannot masquerade as restoration.
Two independent Gateways now verify fan-out, permission-induced mixed outcomes
and recovery through source synchronization. The historical Kubernetes v5 image predates
this repair; the current release image supersedes that deployment.
Current source backend gates pass 876
workspace and 247 no-default-feature app tests; the matrix retains the unrelated
English-only failure described in the evidence.

Native federation evidence: 27 lifecycle checks in
`/tmp/ws5-center-federation-native-v2.log` and 9 mTLS checks in
`/tmp/ws5-center-mtls-native-v3.log`. Cloud gate:
`/tmp/ws5-center-cloud-contract-tests.log` (195 passed; one real-account opt-in ignored).
Real cloud accounts are optional per `cicd/integration/README.md`.

## Historical verification and environment checkpoints

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

The earlier v2 backend matrix passed fmt, clippy, workspace tests, app tests with
no default features (242), dependency isolation and kustomize. Its final exit was
1 at the pre-existing English-only violation in root
`fix-issue-workflow-generic.zh.md`; the subsequent legacy guard was run separately
and passed. See `/tmp/ws5-center-current-backend-matrix-v2.log`. External MySQL,
Kubernetes and federation stages were not opted into that matrix. A 10ms wall-clock
Cloudflare test race was fixed and passed both focused and matrix runs.

## Current validation authority

The latest full frontend and browser counts are 848/117 and 174 passed with two
Kubernetes-only skips, respectively, as linked at the top of this file. The
backend matrix passes 876 workspace and 247 no-default-feature app tests, with
the unrelated English-only guard failure retained explicitly. See
[COMPLETION-AUDIT.md](COMPLETION-AUDIT.md) for exact commands and limits.
The current Kubernetes image has separate OIDC/capability and forced owner-Pod
recovery evidence. Old kind-cluster failures do not describe that deployment.

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
operation. Separate native evidence now covers the selected cross-resource traffic,
attachment, probe and failure scenarios; follow the current evidence links above.
This source comparison establishes catalog/menu completeness only. The completion
audit assesses the wider task against those independent runtime artifacts.

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
That historical frontend checkpoint passed 642 tests, build/lint, E2E types and inventory. All
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

## Reconciled audit actions

The native list-status gap is now repaired across 17 existing status columns.
[STATUS-SOURCE-GAP.md](STATUS-SOURCE-GAP.md) records current implementation and
live denial/recovery evidence. The nested ownership review is complete in
[FIELD-OWNERSHIP-AUDIT.md](FIELD-OWNERSHIP-AUDIT.md); final scope reconciliation
is tracked separately in the completion audit.


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

1. The exact operator/runtime-field audit now covers all 22 catalog kinds,
   including nested HTTP/Stream configurations, opaque operator data and typed
   runtime exclusions. See FIELD-OWNERSHIP-AUDIT.md.
2. Selected capability/permission, OIDC, owner forwarding, owner-Pod failure and
   ambiguous post-dispatch transitions have native evidence. Source, component,
   hermetic adapter and runtime evidence retain their separate scopes.
3. Current frontend/backend gates and the complete native browser regression
   are recorded above. Rerun affected checks after further implementation fixes;
   retain the pre-existing English-only guard limitation.
4. Final requirement reconciliation remains active. CRUD does not prove arbitrary
   Gateway traffic or external DNS propagation. Optional account-backed tests are
   recorded as validation limits, not successful executions.

The following entries retain chronological focused findings; their old counts
and pending notes do not supersede the current validation authority above.


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


EdgionTls attachment follow-up: the form now exposes Gateway parent references,
preserves advanced fields, permits removing the last optional reference and caps
additions at 32. Shared namespace/listener clearing restores omission. Full
frontend: 755 tests / 114 files; build/lint/E2E types pass. Twelve native browser
cases pass, including actual attachment persistence and shared route regressions.
See [TLS-ATTACHMENT-FORM-EVIDENCE.md](TLS-ATTACHMENT-FORM-EVIDENCE.md). This does
not establish TLS handshake behavior.


Route attachment follow-up: HTTPRoute now uses the shared ParentRefsSection
instead of its untranslated duplicate. Seventeen focused tests and nine native
browser cases pass; all five route kinds prove optional namespace/sectionName
omission with actual API readback and unchanged rules/ports. Build/lint/E2E types
pass. See [ROUTE-PARENT-CLEARING-EVIDENCE.md](ROUTE-PARENT-CLEARING-EVIDENCE.md)
for the reproduced defect and retained initial selector failures.


Backend namespace follow-up: HTTP/gRPC/stream backend editors and HTTP mirror
editing omit a cleared namespace, restoring the owner namespace while preserving
port, zero weight and other reference fields. Full frontend: 760 tests / 115 files;
11 native browser cases and build/lint/E2E checks pass. All five routes have actual
save/readback evidence; HTTP additionally covers RequestMirror. The first invalid
gRPC mirror fixture and its correction are retained in
[BACKEND-NAMESPACE-EVIDENCE.md](BACKEND-NAMESPACE-EVIDENCE.md).


GRPCRoute filter follow-up: one shared type list now drives the selector and both
rule/backend mutation preflights. Unsupported Form/YAML submissions retain drafts
and issue no write request; corrected supported filters save normally. Forty-four
focused tests, seven native browser cases and build/lint/E2E checks pass. See
[GRPC-FILTER-PREFLIGHT-EVIDENCE.md](GRPC-FILTER-PREFLIGHT-EVIDENCE.md).


BackendTLSPolicy client certificate follow-up: clearing the optional form field
now removes the key, and preflight matches current Controller name validation.
Thirty-five focused tests cover mutation payload preservation and name boundaries.
See [evidence](BACKEND-TLS-CLIENT-CERT-EVIDENCE.md). Overall audit remains active.


BackendTLSPolicy topology follow-up: the graph now shares client certificate name
parsing with submission preflight and never interprets this same-namespace option
as a cross-namespace dependency. Invalid nonempty strings remain unknown. All 67
focused tests, build and lint pass; see the topology follow-up in
[client certificate evidence](BACKEND-TLS-CLIENT-CERT-EVIDENCE.md).


Gateway frontend TLS follow-up: listener frontendValidation is now correctly
read-only runtime data, omitted from mutations and absent from form controls.
The operator path remains spec.tls.frontend. Fifty-six focused tests and four
native browser cases pass; build/lint/types pass. See
[GATEWAY-FRONTEND-PROJECTION-EVIDENCE.md](GATEWAY-FRONTEND-PROJECTION-EVIDENCE.md)
for the retained initial fixture failure and next concrete CA-reference audit.
The preceding 804-test/173-browser full run predates this repair.


Gateway frontend CA follow-up: required group/kind/name, reference bounds and
frontend/default/per-port containers now match current Controller admission.
Starting from empty TLS configuration produces a valid per-port document. All 48
focused tests and four native browser cases pass, along with build/lint/types.
See [GATEWAY-FRONTEND-CA-EVIDENCE.md](GATEWAY-FRONTEND-CA-EVIDENCE.md).


Gateway TLS topology now includes default/per-port frontend CA and backend client
certificate dependencies, retaining namespace/grant and metadata-only boundaries.
Thirty-nine graph tests plus build/lint pass. See
[GATEWAY-TLS-TOPOLOGY-EVIDENCE.md](GATEWAY-TLS-TOPOLOGY-EVIDENCE.md).


Gateway serving-certificate topology follow-up distinguishes the strict empty
serving group from the backend core alias; invalid kinds no longer resolve to
same-named objects. All 43 graph tests, build and lint pass. See the serving
follow-up in [Gateway TLS topology evidence](GATEWAY-TLS-TOPOLOGY-EVIDENCE.md).
Stale adapter guidance about frontendValidation ownership is corrected.


Gateway frontend validation can now be explicitly cleared without deleting backend
identity or sibling port policies. Clearing a port keeps its empty TLS override;
deleting the override restores inheritance. Thirty-five focused tests, four native
browser cases and build/lint/types pass; see the removal follow-up in
[GATEWAY-FRONTEND-CA-EVIDENCE.md](GATEWAY-FRONTEND-CA-EVIDENCE.md).


BackendTLSPolicy CA-list follow-up: current CRDs cap references at eight. Form and
submission now enforce that limit, and System trust disables adding CA refs.
Neutral CA/target labels cover ConfigMap and EdgionBackend. Thirty-eight focused
tests plus build/lint pass; see the CA-count follow-up in
[backend TLS evidence](BACKEND-TLS-CLIENT-CERT-EVIDENCE.md).


Nested ownership audit: EdgionTls, BackendTLSPolicy and backend traffic policy
root/nested transport fields match the existing mutation exclusions. No additional
production edit was needed. Eighty-five existing tests pass. The exact source
inventory, cache/transport distinctions and remaining resource scopes are in
[FIELD-OWNERSHIP-AUDIT.md](FIELD-OWNERSHIP-AUDIT.md).
