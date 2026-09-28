# Center alignment with current Edgion

Start with [CURRENT-COVERAGE.md](CURRENT-COVERAGE.md) for the current menu/resource
index and evidence limits. This file is the chronological work log; older live
process notes and next steps are historical, not instructions to restart work.

## Objective and completion criteria

Review and update the entire Center backend and dashboard against the current
sibling Edgion source, resource by resource and menu by menu. Do not reconstruct
Edgion history. Preserve unrelated changes. The user authorized Center commits on
2026-09-27; commit validated task changes in batches, never push. Edgion commits
remain unauthorized.

A row is complete only after schema/API checks, applicable fixes, automated
verification, and runtime UI/API evidence. Source inspection or unit tests alone
do not establish complete end-to-end alignment. Center must reach managed
clusters through Controller federation, never directly through Kubernetes.

## Coverage ledger

All rows remain open unless explicitly marked complete.

| Area | Items | Current status |
| --- | --- | --- |
| Shared infrastructure | Resource catalog, mutation envelopes, conditions, permissions, Controller switching, pagination | 22-kind catalog and mutation boundaries aligned; per-writer condition polarity/freshness covered; broader runtime transitions pending |
| Infrastructure | GatewayClass, Gateway, ReferenceGrant | Native list actions and CRUD passed; validation/runtime stripping aligned; cross-namespace grant and listener attachment behavior pending |
| Routes | HTTPRoute, GRPCRoute, TCPRoute, UDPRoute, TLSRoute | Generic native CRUD passed; TCP/UDP v1 aligned; advanced route/attachment semantics pending |
| Services | Service, EndpointSlice, EdgionBackendTrafficPolicy | Lossless Service/EndpointSlice adapters and native CRUD/batch deletion passed; HTTPS probe policy and AI applicability aligned; live probe/resilience/discovery workflows pending |
| AI backends | EdgionBackend, HTTPRoute references, AiProxy, policy attachments | Catalog/access map, editor, route/plugin/policy wiring and topology added; native CRUD and supported AI policy round-trip passed; advanced attachment and failure workflows pending |
| Security | EdgionTls, BackendTLSPolicy, Secret/ConfigMap restricted dependencies | Native actions/CRUD passed; typed mTLS SANs and backend identity admission aligned; handshake and authorization denial workflows pending |
| Plugins | EdgionPlugins, EdgionStreamPlugins, EdgionConfigData | Current HTTP/stream catalogs aligned; nine ConfigData types editable, four new variants passed native browser CRUD; deeper validation and plugin workflows pending |
| System | EdgionGatewayConfig, LinkSys, EdgionAcme | GatewayConfig controls/policy aligned; ACME HTTP-01 aligned; all eight LinkSys variants have typed browser CRUD evidence; advanced behavior and remaining validation open |
| Controller views | Operations dashboard, user dashboard, topology, RegionRoute | Native dashboard/topology controls and AI backend nodes passed; graph freshness/partial-invalid/grant boundaries tested; advanced attachment semantics pending |
| Federation views | Center dashboard, Controllers, registration, counts, proxy, watches, reload | Native registration/counts/proxy/watch/reload, default RBAC, reconnect/resync/eviction and large-response scenarios passed; Kubernetes ownership/fencing and additional count-freshness scenarios remain open |
| Global views | RegionRoute overrides, global ConfigData inventory and subtypes | Eight inventory leaves wired, safe/redacted payload boundaries tested; live global verification and native RegionRoute failover/restore/source sync/enable preservation/missing-target recovery passed; mixed outcome retention covered by component tests; actual traffic remains open |
| Cloud | Provider accounts, Cloudflare DNS, Route53 DNS | Native local account create/edit/CAS conflict/label preservation passed; provider DNS adapters and external workflows still pending |
| Administration | Login/discovery, audit, users, roles, standalone/Kubernetes capabilities | Native standalone auth/logout, audit filters/pagination, user/role mutations and restricted-role denial passed; OIDC and Kubernetes capability workflows pending |
| Runtime validation | Local Center/frontend, current Controller, browser workflows, full matrix | Focused native browser runs and frontend checks passed; Full native standalone RBAC rerun passed 147 tests (one Kubernetes-only skip), with all 112 generated cases passed; Kubernetes/MySQL/live traffic remain open |

## Findings and work log

### 2026-09-27: initial current-source audit

- The Controller has 22 concrete resource kinds. Center registers 21 and omits
  EdgionBackend. Its E2E inventory also hard-codes 21. Update the catalog, types,
  menu/routes, editor/list, fixtures, cleanup map, and inventory together.
- EdgionBackend currently supports OpenAI, OpenAICompatible, and Anthropic in
  `edgion-resources/src/resources/edgion_backend.rs`. Its feature guide also lists
  Gemini, which is not in the current enum; use implementation/schema evidence.
- Kubernetes shared status is `status.controllers[].{controllerName,status}`;
  native FileSystem/etcd payloads remain flat. Center's shared condition component
  previously ignored the envelope, hiding all those conditions. Added native
  payload collection per observation and writer identity for parent/ancestor
  conditions. Compact tooltips retain observation context.
- The 16 already-cataloged kinds with a Controller-computed `spec.currentStatus`
  now exclude that exact path from create/update payloads. Nested operator-owned
  fields with the same name remain intact. EdgionBackend needs the same exclusion
  when registered, plus credential SecretSlot exclusions.
- Existing Edgion worktree changes were present at entry and are not owned by
  this task. Center had no tracked modifications at entry; `.claude/` and the WAF
  diagnostic task already existed.
- Default Node is 16.13.1. Targeted Vitest and Vite startup fail under that runtime
  (`crypto.getRandomValues`); use a temporary Node 22 toolchain for validation.
  ESLint passed. Under Node 22.23.3, the three targeted suites passed (42 tests)
  and `npm run build` passed (TypeScript + Vite; bundle-size warning remains).
  `git diff --check` passed. No live browser or Controller claim yet.
- The full `cicd/integration/run-matrix.sh` is running in exec session `80689`,
  log `/tmp/ws5-center-matrix.log`; last verified alive during Cargo Clippy
  compilation. Poll this session before starting another matrix. It inherited
  Node 16, so its later frontend stage needs the Node 22 PATH if it fails there.
  The cached temporary Node executable is
  `/Users/caohao/.npm/_npx/4bb4bc87b1b72b6c/node_modules/node/bin/node`.
  Use the public npm registry explicitly for new temporary tools: the machine's
  default registry is an old Taobao endpoint and the first download stalled.

### 2026-09-27: EdgionBackend dashboard and validation infrastructure

- Added the namespaced AI backend catalog/access entry and Services menu route
  in both dashboard route trees. Access-document parsing now includes all 22
  Controller kinds. Default Controller RBAC was not widened.
- Added the provider, endpoint, credential Secret references/weights/quotas,
  Redis failure policy, models/aliases/visibility/decimal pricing, and defaults
  form. YAML editing preserves unknown fields and multiple entries. Mutations
  remove credential `secret`, runtime status, and server-owned metadata; update
  retains resourceVersion and uses the captured Controller target.
- Reused the generic Center proxy (`center-app/src/api/mod.rs::proxy_handler`);
  no backend resource whitelist or federation wire change is required for this
  resource. HTTPRoute/AiProxy and policy reference support remains to be checked.
- Extended the 22-kind E2E inventory, fixture credential Secret, cleanup mapping,
  batch action inventory, and Controller fixture RBAC. Fixed the runner's obsolete
  `Edgion-resource-ui` paths: it uses the sibling Edgion or explicit EDGION_DIR.
- AI-focused suites passed (45 tests before the additional rename case); the
  subsequent editor rerun passed all three cases, including rename rejection.
  Build, ESLint, E2E inventory (22 kinds / 224 cases), E2E TypeScript, shell syntax,
  and diff whitespace checks passed. These are not live Controller/browser proof.
- Full frontend test run: 399 passed and one existing Route53 flow exceeded 5 s.
  Repeated with two workers, then diagnosed with one worker and 15 s: all four
  Route53 assertions passed. Only that multi-dialog test now has a 15 s deadline;
  its assertions and requests are unchanged. Full-suite rerun remains required.
- Full matrix initially stopped on missing protoc. Homebrew had no matching
  bottle; GitHub download timed out. A temporary protoc 31.1 from the published
  protoc-bin-vendored-macos-aarch_64 3.2.0 crate is available at
  `/tmp/ws5-center-tools/protoc-bin-vendored-macos-aarch_64-3.2.0/bin/protoc`.
- Rust 1.98 Clippy rejected generated tonic Result error sizes and a constant
  chunks_exact call. Scoped allowances cover only the generated proto modules;
  Route53 catalog lookup uses equivalent as_chunks iteration (supported by the
  repository's Rust 1.92 container toolchain). No wire contract changed.
- Sessions 80689, 87988, and 44403 terminated on the above diagnosed failures.
  The active matrix is session **78986**, log
  `/tmp/ws5-center-matrix-node22-v3.log`, with PROTOC, Node 22 PATH, and the public
  npm registry set. Poll it before launching another matrix.
- Matrix v3 passed Rust formatting and workspace/all-target Clippy, and is now
  compiling the Rust test targets. The validated tooling changes were committed
  as `62905d0` (`chore: support current toolchain and Controller checkout`), touching
  only the three Rust compatibility sites and the E2E runner. No push. Frontend
  and task-ledger changes remain uncommitted pending their final validation.

## Next work

### Latest checkpoint: AI reference chain, 2026-09-27

- EdgionBackendTrafficPolicy now accepts homogeneous Service or EdgionBackend
  references, rejects mixed/duplicate/excessive AI targets and AI active health
  checks, strips resolvedTargetClass, and uses current loadBalancer.degradeThreshold.
  Omitted retryConstraint budget/floor defaults are valid and survive editing
  without materializing defaults into untouched documents. Policy list labels
  include target kind. Three policy suites passed (25 tests).
- HTTPRoute's reference selector offers the typed AI backend and removes Service
  port on selection. Zero weight is preserved (the old `weight || undefined`
  silently restored the default traffic weight). Mutation validation rejects an
  explicit AI backend port. AiProxy is registered with its five current config
  fields and request-stage body capability. ExtProc is now offered only in the
  request stage, matching current Edgion applicable_stages_by_name; its response
  processing belongs to that entry. This does not complete the remaining plugin
  catalog audit (many other entries still need individual review).
- Route-selector/catalog/route-adapter suites passed (13 tests before adding
  the explicit AI-port adapter regression and body-capability assertions).
  Final frontend build passed with the port regression included; later one-line
  AiProxy body-gate change still awaits the full matrix's frontend tests.
  ESLint and diff whitespace checks passed before that one-line addition.
- The full matrix session 78986 remains live. Workspace Rust tests have passed;
  the no-default-feature application test target is compiling. Do not restart it.
- Current Edgion Controller build is active in session **80400**, log
  `/tmp/ws5-current-controller-build.log`, invoked with the sibling Edgion manifest
  and the temporary PROTOC path. No Edgion source was modified by this task.
- Playwright Chromium installation session **17501** completed successfully;
  Chrome, FFmpeg and Headless Shell are installed. Installation log:
  `/tmp/ws5-center-playwright-install.log`.
- Once these complete, run the standalone harness with unique fixture identity
  and environment credentials. Start with grep matching
  `real CRUD crosses the browser and API boundary for EdgionBackend`, then extend
  to the route/plugin/policy workflows. The script refuses occupied ports and
  uses the current sibling checkout. No runtime verification is claimed yet.

1. Poll the active full matrix and address failures; targeted shared-status
   verification is complete, runtime verification remains open.
2. Finish EdgionBackend's HTTPRoute/AiProxy/policy-reference integration and run
   live UI/API verification without widening default Controller RBAC.
3. Progress through every coverage row and retain actual runtime evidence.

For write-model changes, read the existing program authority at
`docs/superpowers/specs/2026-07-26-center-controller-write-model-convergence-design.md`
before acting on CCI execution files or proposing replacements.

### Latest checkpoint: global LB field and runtime preparation

- Matrix session 78986 passed workspace tests and the application tests without
  default features (242 tests). It is currently running npm ci with Node 22 and
  the public registry; the live npm process was verified, so do not restart it.
- Current Controller build session 80400 is still live, compiling resources.
- Started native Center standalone build session **99657**, log
  `/tmp/ws5-center-standalone-build.log`, with the same temporary PROTOC path.
  Poll this handle before invoking a duplicate build or launching the harness.
- Fixed the global EdgionGatewayConfig loadBalancing field from panicThreshold
  to the current degradeThreshold in its type, form, validation, and test fixture.
  Added integer/range and mutation retention regression coverage. Current source
  and CRD agree; frontend execution awaits npm ci completion in the matrix.
- Next AI attachment audit: BackendTLSPolicy currently has free-text target kind
  and group fields. Current Controller supports Service or EdgionBackend but
  requires exactly one target and forbids sectionName on AI targets; compare the
  editor and mutation validation before claiming attachment coverage complete.
- No new commit this checkpoint; frontend/runtime evidence remains pending.
  Edgion worktree remains unchanged by this task; diff whitespace checks passed.

### Latest checkpoint: TLS policy attachment and completed static matrix

- Matrix 78986 terminated with exit 1 at the English-only guard. Before that it
  passed Rust checks/tests, frontend lint, all 411 frontend tests, build, 22-kind
  E2E inventory, and E2E typecheck. Failure is the pre-existing tracked root
  `fix-issue-workflow-generic.zh.md`, unchanged by this task. Do not report the
  whole matrix as green or rewrite that unrelated workflow to hide the failure.
  The remaining no-legacy-PluginMetaData guard and diff whitespace check passed
  when run separately.
- Native Center standalone build session 99657 completed successfully. Current
  Controller session 80400 remains live; continue polling it before harness run.
- BackendTLSPolicy now offers Service/EdgionBackend selection with canonical
  group updates, clears/disables sectionName for AI targets, and validates exactly
  one supported target. Trust validation rejects conflicting CA sources,
  cross-namespace CA references, unsupported CA groups/kinds, and duplicate CA
  refs. resolvedTargetClass is stripped from operator mutations. Existing test
  that expected cross-namespace CA acceptance was corrected against Controller
  validate_policy source. Targeted adapter/editor tests passed (6 tests), log
  `/tmp/ws5-center-btp-tests-v2.log`.
- These TLS changes happened after the full frontend run. Follow-up build is
  session **20212** (`/tmp/ws5-center-btp-build.log`), lint session **53501**
  (`/tmp/ws5-center-btp-lint.log`); poll before repeating. Real browser proof is
  still pending. No new commit or push this checkpoint.

### Latest checkpoint: local and Redis rate-limiter fields

- Follow-up TLS policy build 20212 and lint 53501 both passed.
- Replaced the removed RateLimit menu/type choice with RateLimitLocal. Both
  limiter forms now use onKeyMiss (Allow/Deny), and the local form exposes
  onLimitExceeded (Reject/Continue) plus the current Instance/Cluster scope.
  Verified fields against rate_limit_local.rs and rate_limit_redis.rs. Existing
  unknown input remains losslessly editable; no automatic legacy-name conversion.
- Plugin catalog/form tests passed (10 tests), log
  `/tmp/ws5-center-ratelimit-tests.log`; build 47363 passed, log
  `/tmp/ws5-center-rate-build.log`. Lint session **85500** is running, log
  `/tmp/ws5-center-rate-lint.log`. Diff whitespace check passed.
- Controller build 80400 remains live and has progressed to edgion_controller
  compilation (verified from the actual rustc process); do not duplicate it.
- Next broader plugin issue to audit: conditions.run/skip use current Rust
  ConditionSet, while old dashboard fixtures still contain arrays. Source:
  `edgion-resources/src/resources/edgion_plugins/conditions.rs`.

### Runtime launch checkpoint

- Controller build 80400 finished successfully (15m47s), and limiter lint 85500
  passed. Both native binaries are available.
- Started standalone browser harness session **84611**, log
  `/tmp/ws5-center-ai-runtime.log`, run ID `alignment-ai-20260927`, artifact root
  `web/test-results/alignment-ai-20260927`. Credentials and JWT key are randomly
  generated environment values. Grep selects exactly the EdgionBackend CRUD case.
- Harness strict inventory passed. Its initial cached cargo build is currently
  waiting on the shared package-cache lock; the handle is live. Do not restart or
  kill other workspace builds. Poll 84611 and inspect the log for startup/tests.

### Latest checkpoint: runtime identity and condition shape

- First browser harness 84611 terminated on startup: current native Controller
  requires conf_center.controller_name. Added edgion.io/gateway-controller to
  both isolated filesystem fixtures, matching their GatewayClass controllerName.
- Second harness **98864** is live, log `/tmp/ws5-center-ai-runtime-v2.log`,
  run ID `alignment-ai-20260927-v2`. It is awaiting the shared Edgion build-dir
  lock; do not kill unrelated cargo processes or restart the live harness.
- Plugin conditions.run/skip types and form now use ConditionSet objects, not
  arrays. Current allOf/anyOf fixtures preserve nested header-key predicates.
  Structured object/array editors now display declared catalog defaults without
  mutating the document until an actual edit; this also exposes AiProxy's Weighted
  selection shape. Condition groups require actual nonempty predicates before
  Controller acceptance; empty editor scaffolds are not claimed valid configs.
- Adapter/form suites passed (11 tests) before the final default-display change.
  Build **12567** is running (`/tmp/ws5-center-conditions-build.log`), followed by
  a form rerun (`/tmp/ws5-center-conditions-form-v2.log`). Runtime CRUD evidence
  remains pending; no new commit or push.

### Latest checkpoint: RequestId and default auth example

- Post-default-display form rerun 23903 passed (4 tests), and condition build
  12567 passed. The displayed defaults do not materialize until edited.
- Replaced removed TraceContext catalog/type choice with current RequestId,
  exposing defaultId and ids. Updated editor/body-capability fixtures to the
  current plugin. Three relevant suites passed (17 tests), log
  `/tmp/ws5-center-requestid-tests.log`.
- Updated the unused exported BasicAuth YAML example from removed inline
  credentials to named secretGroups/secretRefs, checked against BasicAuthConfig.
- Build session **99687** (`/tmp/ws5-center-requestid-build.log`) and lint
  **31260** (`/tmp/ws5-center-requestid-lint.log`) remain to be polled.
- Source enum comparison still finds absent choices: AiGuard, CredentialInjector,
  FormJsonTransform, GeoIpLocation, Guardrail, JsonSchemaValidation,
  RequestAccessPolicy, RequestBodyBuffer, WebhookKeyGet. GlobalAccessControl is
  a removed choice still requiring replacement with the full current access-policy
  schema and resolved-field mutation filtering. Do not treat the catalog as complete.
- Harness 98864 is still live. lsof confirms another cargo (PID 81401) owns the
  sibling Edgion target lock. Preserve that build; poll the existing harness.
  Browser CRUD has not yet run. Diff whitespace check passed; no commit/push.

### Latest checkpoint: first real browser success and access policy

- Harness 98864 completed successfully: password login and real EdgionBackend
  browser/API CRUD passed (2 tests, 19.9s). Both current native Controllers
  registered over federation; exact isolated fixture files were retained by the
  cleanup ledger. Evidence: `/tmp/ws5-center-ai-runtime-v2.log` and
  `web/test-results/alignment-ai-20260927-v2`. This proves ordinary CRUD, not all
  advanced AI attachment/error scenarios.
- Replaced removed GlobalAccessControl choice with RequestAccessPolicy's current
  defaultProfile/profiles/activeProfileRef/status/message/description fields.
  Mutation filtering strips expanded resolvedProfiles and nested resolvedCandidates
  while retaining authored rules and unknown siblings. Catalog/adapter/shared
  mutation suites passed (34 tests).
- Body capability gates now also cover ForwardAuth, ProxyRewrite JSON bodies,
  buffered (not streamed) ExtProc, RequestRestriction/RequestAccessPolicy HMAC
  predicates, and the current intrinsic body consumers. Adapter tests passed
  (9 tests), including nested allOf inside anyOf and non-body alternatives.
  Build 37694 and lint 16283 passed; logs are
  `/tmp/ws5-center-access-body-{build,lint}.log`.
- Started **all catalog resource CRUD** harness session **21734**, run ID
  `alignment-crud-20260927`, log `/tmp/ws5-center-all-crud-runtime.log`. It is live
  waiting on the sibling build-dir lock before startup. Poll before restarting.
- RequestId build 99687 and lint 31260 also passed. The remaining catalog omissions
  are AiGuard, CredentialInjector, FormJsonTransform, GeoIpLocation, Guardrail,
  JsonSchemaValidation, RequestBodyBuffer, and WebhookKeyGet; deeper configuration
  audits still pending for existing plugins. No new commit/push this checkpoint.

### Latest checkpoint: 48 HTTP plugin choices

- Added the remaining request-stage catalog/type entries: AiGuard,
  CredentialInjector, RequestBodyBuffer, JsonSchemaValidation, FormJsonTransform,
  GeoIpLocation, Guardrail, WebhookKeyGet. Operator field names/enum options were
  checked against each config struct, including flattened Guardrail connection
  fields and the canonical stage exceptions table. Guardrail retry/dynamic timeout/
  custom success options are not offered because its validator rejects them.
- Imported the actual TypeScript catalog and compared names to current Rust enum:
  48 executable variants, 48 choices, missing=[], removed=[]. This proves name
  coverage only; existing plugin nested schemas and runtime semantics remain open.
- Three catalog/form/adapter suites passed (22 tests), log
  `/tmp/ws5-center-new-plugins-tests.log`. Subsequently strengthened the Guardrail
  negative assertion to check each forbidden field individually. Build session
  **78400** (`/tmp/ws5-center-48-plugins-build.log`) is running; poll it.
- All-resource browser harness **21734** is still live waiting for the sibling
  build-dir lock; no new result is claimed. Continue polling the same handle.
- No new commit or push. Diff whitespace check passed. The broad task remains open.

### Latest checkpoint: existing plugin fields and first dashboard batch

- Audited current top-level config structs and serialization wrappers. Fixed
  ForwardAuth's flattened connection fields and forwardBody, RequestRestriction's
  conditions (replacing removed ruleGroups), IpRestriction's RemoteIp/DirectPeerIp
  enum, ProxyRewrite jsonBody, DSL body scan/request caps and rateLimitPolicies,
  ExtProc mode-override controls, and removed LDAP cache fields. HmacAuth's custom
  wire struct intentionally omits matcher negate/descriptor; do not expose them.
- Targeted catalog/form/mutation suites passed (32 tests). Build 94891 and lint
  79097 passed. Full frontend run 20960 had 418 passing tests and one obsolete
  BackendTLSPolicy fixture combining System trust with a cross-namespace CA ref.
  Corrected it to exercise same-namespace CA and System trust separately while
  preserving every lossless-field assertion. The full owning adapter suite then
  passed (15 tests), log `/tmp/ws5-center-resource-adapters-v2.log`.
- The first dashboard batch is ready for a Center-only commit. Validation also
  includes the earlier Rust matrix and actual EdgionBackend browser CRUD. Whole
  hermetic matrix is not green solely because the pre-existing root Chinese
  workflow violates check_english_only.sh; the separate legacy-reference and diff
  checks passed. This commit is a progress batch, not completion of the objective.
- All-resource CRUD harness 21734 remains live waiting on the sibling build-dir
  lock. Next known source drift: stream IpRestriction still offers clientIp and
  remoteAddr; inspect stream-stage source separately before changing it.

### Latest checkpoint: stream menus and running full browser CRUD

- Committed prior dashboard batch as **11d3735** (59 files). No push. Existing
  `.claude/` and `tasks/todo/waf-support-diagnostics-center-integration.md` remain
  unrelated and untracked.
- Added connection-stage GeoIpLocation using the shared HTTP field definition;
  TLS-route IP source choices/types now use RemoteIp/DirectPeerIp. Added stage
  guidance for DirectPeerIp-only GeoIP and preserved both stage arrays in tests.
  Updated the stale stream skill example. Targeted tests passed (6 tests), and
  build 95049/lint 70831 passed. These changes remain uncommitted.
- Added documented standalone `E2E_SKIP_BUILD=1` with executable checks. Both
  binaries had already built and passed real AI CRUD; repeated builds were
  contending with another agent's build lock. Intentionally stopped ONLY our
  waiting cargo child PID 84203 after checking parent 83582 and exact command;
  old harness 21734 exited 143. Other cargo processes were untouched. This was
  a deliberate runner change, not an observation-timeout restart.
- New all-resource harness **68177** uses E2E_SKIP_BUILD=1, run ID
  `alignment-crud-20260927-v2`, log `/tmp/ws5-center-all-crud-runtime-v2.log`.
  It is now running 23 tests (login plus all 22 catalog resource CRUD cases).
  So far login, GatewayClass, Gateway, HTTPRoute, and GRPCRoute passed.
- EdgionGatewayConfig CRUD failed because the Controller drops the obsolete
  root `spec.maxBodySize`. Current schema moved to `spec.requestBody`, including
  defaultMaxBodySize/maxBodySize and other body settings. This is a real editor/
  fixture drift, not an assertion to weaken. Next priority: read RequestBodyConfig
  in current edgion_gateway_config.rs, update form/types/validation/fixtures, then
  rerun the failed browser case. Failure artifacts live under this run's
  playwright/specs-resource-mutations-r-72a34-ary-for-EdgionGatewayConfig-standalone/.
- Another pending audit: ConfigData frontend only admits five types while current
  source also contains RequestAccessUrlAllowList, ProxyProtocolTrust, WafRuleBundle,
  WafPolicy. Audit Center backend/global-read boundaries too before adding them.

### Request body and stream-route runtime corrections

- Harness 68177 finished: 20 passed, three failed (GatewayConfig, TCPRoute,
  UDPRoute). The two stream routes returned v1 from the Controller, while the
  mutation catalog admitted only v1alpha2, preventing the browser PUT entirely.
- Migrated GatewayConfig types, create YAML, form, validation, and fixture to
  requestBody with its six current fields. Validation checks positive sizes,
  effective memory/body bounds, positive storage deadlines, and obsolete root
  maxBodySize. Form edits retain sibling and unknown fields.
- Added current TCP/UDP v1 envelopes to the catalog and a regression exercising
  editor submission of Controller-returned resources. Targeted tests: 39 passed
  (`/tmp/ws5-center-body-route-tests.log`); build 2802 and lint 15478 passed.
- Real native rerun 54388 passed all four tests (authentication and all three
  formerly failing resources); log `/tmp/ws5-center-body-stream-runtime-v3.log`,
  artifacts `web/test-results/alignment-body-stream-20260927-v3`. Owned processes
  exited and 70 exact fixture files were retained by the harness.
- The vendored standard Gateway API v1.6.2 CRDs also declare TCPRoute/UDPRoute v1
  (not only the Rust resource envelope). Updated create templates, catalog
  canonical versions, type comments, and E2E fixtures to v1, retaining the
  previously exercised v1alpha2 mutation envelope as an accepted alternate.
- Final v1 template targeted unit run: session 65318,
  `/tmp/ws5-center-route-v1-tests.log`. Real native v1 fixture rerun is active;
  log `/tmp/ws5-center-route-v1-runtime-v4.log`, run ID
  `alignment-route-v1-20260927-v4`. Poll its session before any restart.
- All current changes remain uncommitted. The full resource/menu audit is still
  open; generic CRUD does not establish advanced feature correctness. Next:
  finish ConfigData typed variant and backend/global-read boundary audit.
- v1 unit session 65318 passed all 42 tests. Active browser handle is **25427**.

### ConfigData current variant audit

- Final TCP/UDP v1 fixture browser run 25427 passed all three tests; log
  `/tmp/ws5-center-route-v1-runtime-v4.log`. Its owned services terminated.
- Current ConfigEntry contains nine operator variants. Added missing
  RequestAccessUrlAllowList, ProxyProtocolTrust, WafRuleBundle, and WafPolicy to
  the local editor/parser and structured fields. Added all-variant round trips
  and real rendered nested field edits. The Controller storage GET/list path
  (api/types.rs center_get_resource/center_list_resources_namespaced) returns
  stored documents; only processed cache serialization uses AdminRedactGuard.
  Local mutation editors therefore remain on storage reads, not global snapshots.
- URL condition mutation serialization now strips resolvedValues,
  resolvedCredentials, resolvedIps and refDenied inside candidate conditions,
  scoped strictly to RequestAccessUrlAllowList so Misc payloads remain lossless.
- Added all four types to the backend catalog, exact global API filter parsing,
  and frontend global wire type. Global menu expansion and read-model payload
  projection are STILL PENDING. Existing fail-closed redaction allowlist remains
  unchanged: new payloads are currently redacted in global responses. WAF bundle
  content must stay redacted; URL condition Secret/HMAC internal fields require
  explicit projection before any allowlist expansion. ProxyProtocolTrust and
  WafPolicy contain CIDRs and references, respectively; audit/test their projection.
- Verification: local ConfigData/form tests passed 22 tests (67077,
  `/tmp/ws5-center-configdata-forms-v2.log`), then mutation boundary suite passed
  14 tests (9018, `/tmp/ws5-center-configdata-boundary.log`). The boundary fixture
  subsequently corrected condition casing/envelope to current source; rerun it.
  Build 29152 passed before the final boundary addition. Core catalog test 27208
  passed. App global API test session **68285** still compiling; log
  `/tmp/ws5-center-configdata-api.log`. Frontend lint launched after final edits;
  log `/tmp/ws5-center-configdata-lint.log` (capture/poll its handle).
- No commits/push this checkpoint. Next: finish global types/menu/redaction,
  add real per-variant CRUD/browser evidence, run final build/lint/matrix, and
  commit the verified Center batch. Preserve unrelated untracked files.
- Lint handle: **9205**. Corrected boundary-fixture rerun log:
  `/tmp/ws5-center-configdata-boundary-v2.log` (new handle below).
- App global API session 68285 finished successfully: 11 tests passed. Lint 9205
  passed. Corrected boundary fixture rerun 73068 passed all 14 tests. No running
  handles remain from this checkpoint except already-finished output collection.

### Global ConfigData menus and content visibility

- Added direct inventory/menu routes for RequestAccessUrlAllowList,
  ProxyProtocolTrust, WafRuleBundle, and WafPolicy, behind existing capability and
  permission gates. RegionRouteOverride retains its dedicated view.
- Audited current schemas: ProxyProtocolTrust has mode/CIDRs; WafPolicy has named
  bundle references. Both now pass through the global read model. WafRuleBundle
  rule/phrase contents and URL allow-list condition payloads remain globally
  redacted, alongside Misc. URL conditions can carry private values/credentials;
  metadata-only global visibility is deliberate, not an unfinished allowlist.
  The authorized Controller-local editor still reads complete storage documents.
- Global comparisons no longer infer equality from two hidden payloads. Added
  unavailable comparison state and a drawer explanation. Updated both relevant
  global-resource skill docs with current menu and payload visibility boundaries.
- Added real browser typed ConfigData cases for the four new variants: YAML
  creation, structured nested field edit, exact storage readback, and UI deletion
  with exact-ID fallback cleanup. E2E typecheck/inventory passed, but runtime
  execution awaits the rebuilt Center binary.
- Checks: global menu/API tests passed 14; global view suite passed 17; runtime
  read-model suite 65285 passed 9. Full frontend run 38396 passed **437 tests**
  across 85 files (`/tmp/ws5-center-configdata-full-web.log`). Build 11603 and
  lint 12548 passed; E2E inventory/typecheck also passed. Existing Vite chunk and
  JSDOM/AntD warnings remain non-failing.
- Native Center build **32403** still active, log
  `/tmp/ws5-center-configdata-native-build.log`; it is linking the standalone
  binary. Backend hermetic matrix **56421** is waiting on its Cargo lock; log
  `/tmp/ws5-center-configdata-matrix.log`. This invocation uses EDGION_SKIP_WEB=1
  because current full web/build/lint/E2E static checks were run separately, and
  npm ci must not remove node_modules during the planned live browser run.
  Poll these handles; do not restart on an observation timeout.
- Next: run grep `typed ConfigData browser CRUD` with E2E_SKIP_BUILD=1 after 32403
  completes, inspect failures, finish the matrix, and commit the verified batch.
  The full per-resource/per-menu program remains open beyond this batch.
- Native build 32403 completed successfully (2m22s). New typed ConfigData browser
  run launched: run ID `alignment-configdata-20260927-v1`, log
  `/tmp/ws5-center-configdata-runtime-v1.log`. Uses the rebuilt Center and current
  previously built Controller with E2E_SKIP_BUILD=1. Backend matrix 56421 now
  runs Clippy after acquiring the released lock.
- Active typed browser handle: **92676**. Preserve this process and poll it on
  continuation. The live native binary matches the current backend changes.

### Typed browser CRUD and remaining GatewayConfig schema gaps

- Typed ConfigData browser run 92676 passed all five tests (auth plus four new
  variants), including raw WAF rule content preservation. Exact 70 fixtures
  retained; owned processes exited. Log `/tmp/ws5-center-configdata-runtime-v1.log`.
- Extended those cases to verify watch-fed global type filters, safe body
  visibility versus redacted bodies, and actual inventory page rendering before
  returning to the Controller page for deletion. Typecheck passed. New run ID
  `alignment-configdata-global-20260927-v2`, log
  `/tmp/ws5-center-configdata-global-runtime-v2.log`; poll its new handle below.
- Backend matrix 56421 passed workspace/all-target Clippy and is compiling the
  all-target Rust test binaries. No terminal result yet.
- Current-source follow-up found additional GatewayConfig gaps: root maxRetries
  was replaced by retry.attempts (u32 bounded to i32::MAX); forwardedHeaders has
  remoteIpHeader with shared header-name validation; pluginPolicy has allow/deny,
  defaultAction, deniedAction, and blockStatus with per-entry overrides. These
  are NOT implemented by the current Center form yet. Next batch must audit
  and add them, with special care for absent versus empty pluginPolicy.allow.
- EdgionTls now also carries resolvedListenerAttachments; audit its mutation
  exclusion together with the broader TLSRoute attachment changes. Current
  Edgion worktree includes unrelated conformance/shutdown work; preserve it.

- Active global browser handle: **66449**; backend matrix remains **56421**.
- Global browser run 66449 finished with two passes and three failures. Trace
  evidence: storage and global watch both held updated config and resourceVersion
  71 for ProxyProtocolTrust, but metadata.annotations was absent. The shared
  round-trip helper conditionally skips metadata edits when its immediate count
  occurs before lazy form mounting; the new global poll incorrectly assumed that
  optional edit always happened. Added explicit annotation-control visibility
  before the typed helper and an immediate storage annotation assertion. Existing
  propagation/body assertions remain. New run v3 is launched, log
  `/tmp/ws5-center-configdata-global-runtime-v3.log` (handle below).
- Matrix 56421 passed workspace Rust tests and is compiling the app's
  no-default-features tests. No terminal result yet.
- Active rerun handle: **35644**. Matrix remains **56421**. Do not launch another
  browser or matrix until these report terminal state. Pending commit must wait
  for these results; current index is clean and all tracked task edits remain
  unstaged. Unrelated `.claude/` and `tasks/todo/` stay untouched.

### Verified batch checkpoint

- Global browser rerun **35644 passed all five tests**. Each new ConfigData type
  passed local CRUD, post-write watch-cache propagation, payload visibility
  assertions, its actual global inventory page, and exact deletion. Log:
  `/tmp/ws5-center-configdata-global-runtime-v3.log`. Owned services exited; 70
  exact fixture files retained. No active browser handles remain.
- Backend matrix **56421 finished exit 1 only at the English-only guard**:
  the pre-existing tracked `fix-issue-workflow-generic.zh.md` violates that guard.
  Preserved the unrelated file. Before that guard, formatting, all-target Clippy,
  all workspace/all-target tests, app no-default-features (242 tests), dependency
  boundaries and manifest validation passed. External Kubernetes/MySQL were not
  enabled. Full web 437 tests/build/lint and E2E static checks passed separately.
  The remaining no-legacy-PM guard and diff whitespace check passed separately.
- This batch is ready for the user-authorized Center commit, with the existing
  English-only baseline failure explicitly recorded. No push. Next implementation
  remains GatewayConfig retry/forwardedHeaders/pluginPolicy and the other open
  coverage rows; the entire alignment goal is NOT complete.

### GatewayConfig retry, forwarding, and plugin execution policy

- Previous verified batch committed as **0178fe1**. No push; unrelated untracked
  `.claude/` and `tasks/todo/` preserved.
- Replaced removed root maxRetries with retry.attempts in types, default YAML,
  create drafts, and form; current default is 2, permitted range 0..2147483647.
  Added forwardedHeaders.remoteIpHeader with canonical header grammar/length
  and framing/hop-by-hop/reserved-name checks.
- Added pluginPolicy fields and explicit allow-list-mode control: absence/null
  leaves allow-list mode off, an explicit empty array enables deny-all. Preserves
  untouched defaults/unknown siblings. Validation checks canonical family/type
  names, duplicates, enum actions, 400..599 statuses, and effective bypass/status
  conflicts. Reused HTTP plugin catalog and extracted existing stream names to
  shared constants consumed by the stream form and policy validator.
- Optional policy/allow/entry action/status nulls follow Rust Option semantics;
  no automatic migration of removed maxRetries (validation identifies it).
- Extended the native GatewayConfig fixture with zero retries, a custom RemoteIp
  header, and qualified allow/deny plugin names. New live browser run **24241**,
  run ID `alignment-gateway-policy-20260927-v1`, log
  `/tmp/ws5-center-gateway-policy-runtime-v1.log`; poll before restarting.
- Validation: three targeted suites passed 24 tests (65601); build 28581 passed.
  After null-semantics correction, validation-only rerun **10985** is logged at
  `/tmp/ws5-center-gateway-policy-validation-v2.log`. Lint handle **46222**, log
  `/tmp/ws5-center-gateway-policy-lint.log`. Changes remain uncommitted. No backend
  code changed in this batch; previous Rust matrix evidence still applies.
- GatewayConfig nested server/timeouts/security and complete advanced policy
  workflows still need audit; do not mark the whole System row complete from
  these edits or generic CRUD alone.
- GatewayConfig browser 24241 finished successfully: auth plus real CRUD passed
  (2 tests), including current retry/forwardedHeaders/pluginPolicy readback and
  Form/YAML round trips. Validation rerun 10985 passed 10 tests; lint 46222 passed.
  No live handles remain for this checkpoint. This frontend-only batch is ready
  for a Center commit; no additional backend matrix rerun is needed absent any
  backend changes since 0178fe1.

### GatewayConfig nested controls and Controller-only route fields

- Prior retry/forwarding/policy batch committed as **f05277c**, no push.
- Current GatewayConfig ServerConfig contains only enableCompression and
  downstreamKeepaliveRequestLimit. Removed obsolete process controls (threads,
  workStealing, gracePeriodSeconds, gracefulShutdownTimeoutS,
  upstreamKeepalivePoolSize, errorLog) from the form, types, and create defaults.
  Removed securityProtect.rejectDuplicateHost control. Existing YAML is preserved
  losslessly, with clear validation errors identifying these known obsolete fields.
- Added LinkSys maxInstancesPerKind (1..10000, default 200); preserved webhook
  response cap and added u32 keepalive limit validation. Updated the live fixture
  with both active server fields and LinkSys limits.
- All five route kinds now exclude resolvedStatusController on mutation;
  TCPRoute, UDPRoute, and EdgionTls also exclude resolvedListenerAttachments.
  Those fields are schema-hidden Controller-owned identity/proof data. Tests
  preserve same-named nested operator values and verify the exact exclusions.
- Targeted tests 93940 passed 35 tests. Build 77859 passed. Lint **48559** and
  browser **42633** were launched; browser log
  `/tmp/ws5-center-nested-config-runtime-v1.log`, run ID
  `alignment-nested-config-20260928-v1`. Browser covers GatewayConfig, TLSRoute,
  and EdgionTls. Extra LinkSys form regression is running with log
  `/tmp/ws5-center-nested-config-form-v2.log` (handle below). Poll before restart.
- Changes remain uncommitted. Continue nested schema/validation audit (RealIp,
  outbound TLS, access-log policy, preflight) and then remaining System resources.
- Final results: lint 48559 passed; extra form suite 32800 passed all six tests;
  browser 42633 passed all four tests (auth plus GatewayConfig/TLSRoute/EdgionTls
  CRUD). Owned services exited; 70 exact fixture files retained. All handles for
  this batch are terminal. Ready for the authorized Center commit.

### RealIp and preflight admission alignment

- Previous nested-controls batch committed as 82b2639; no push.
- Matched current RealIp shared admission validation: require a nonempty trusted
  group array and a valid header token of at most 256 characters. The read header
  continues to allow X-Forwarded-For; outbound header write restrictions do not
  apply. Added optional u32 maxTrustedHops bounds, preserving zero and null.
- Preflight response status now requires an integer from 200 through 599, matching
  validate_preflight_policy. Form numeric controls use the same bounds.
- Targeted adapter/form suites passed 19 tests (59517), production build passed
  (51020), and full lint passed (23914). Logs: /tmp/ws5-center-realip-tests.log,
  /tmp/ws5-center-realip-build.log, /tmp/ws5-center-realip-lint.log. All handles
  terminal. This batch changes frontend validation only; existing runtime CRUD
  and Rust matrix evidence above does not prove the newly rejected input cases.
- Confirmed GatewayConfig mutation exclusions already strip outbound TLS resolved
  CA/client certificate data. Outbound TLS validation, access-log behavior, and
  remaining System resources still require further audit; overall goal remains
  open. No Edgion files changed and no push performed.

### ACME HTTP-01 scope alignment

- RealIp/preflight batch committed as 8ad1572, no push.
- Current AcmeChallenge contains HTTP-01 only; removed DNS-01 types, switching,
  provider/credential/propagation controls. YAML and form submissions now reject
  unsupported challenges and wildcard domains, plus missing email/domain lists.
- Separated editable-draft serialization from submit validation, so empty create
  forms and unfinished YAML transitions remain usable. Kept EAB Secret references,
  renewal durations, storage and auto-TLS data. Filtered resolvedListenerAttachments
  and notifyAfterPublish at the shared mutation boundary. Updated ACME skill notes.
- Adapter/form tests passed 15 cases (17690); production build 68686 and lint
  68901 passed. Logs: /tmp/ws5-center-acme-tests-v2.log,
  /tmp/ws5-center-acme-build.log, /tmp/ws5-center-acme-lint.log.
- Native browser run 42653 passed auth and EdgionAcme CRUD, including optimistic
  conflict handling and form/YAML round trips. Log:
  /tmp/ws5-center-acme-runtime-v1.log; run ID alignment-acme-20260928-v1.
  Owned processes stopped; 70 fixture files retained. This verifies resource
  management, not external ACME issuance; the fixture uses a local invalid issuer.
- Next concrete gap: LinkSys SystemConfig now includes Otlp, but Center lists only
  six kinds. Audit its wire envelope and credential/TLS fields before adding UI.
- Outbound TLS source confirms custom CA refs take precedence when configured;
  do not incorrectly import BackendTLSPolicy's mutually exclusive CA rule into
  GatewayConfig. Remaining TLS/access-log and full menu coverage stay open.

### LinkSys OTLP provider

- ACME HTTP-01 batch committed as 4e39f31, no push.
- Added current otlp envelope, endpoint/timeout/auth/TLS types, defaults, form
  selection/controls, and list endpoint summary. Preserved Secret reference group
  and kind on narrow edits. Shared mutation exclusions already remove OTLP auth
  SecretSlot material and resolved TLS certificates; added explicit coverage.
- Mirrored OTLP origin grammar and timeout bounds, core Secret identity checks,
  and enabled-only TLS rules (HTTPS, verify=true, no hostname/SAN override,
  Secret-only CA refs). Disabled TLS data is preserved and not validated as active.
- Four LinkSys suites passed 15 tests (39586). After the reference-preservation
  change, two affected suites passed 13 tests (41404). Build 65741, lint 53735,
  E2E typecheck 89389 passed. Logs: /tmp/ws5-center-otlp-tests.log,
  /tmp/ws5-center-otlp-tests-v2.log, /tmp/ws5-center-otlp-build.log.
- Browser v1 stopped before service launch on a missing action-inventory entry;
  registered the new input. Browser v2 (2083) passed authentication plus OTLP
  create/form-edit/YAML-roundtrip/API-readback/delete. Log:
  /tmp/ws5-center-otlp-runtime-v2.log; run ID alignment-otlp-20260928-v2.
  Owned services stopped; 70 seeded fixture files retained. This proves management
  CRUD, not collector connectivity/export (fixture endpoint is deliberately inert).
- LinkSys is still open: current source has eight variants, including the missing
  credentialSource provider. Audit its OAuth2 acquisition/rotation/publication
  schema and nested credential boundaries next. Other provider field drift and
  broader menu workflows still need verification.

### LinkSys credentialSource provider

- OTLP batch committed as 55329c6, no push.
- Added eighth LinkSys variant credentialSource with flat oauth2ClientCredentials
  provider, clientSecretBasic active/previous Secret references, scopes, nested
  TLS, rotation durations, egress blockPrivate, and publication controls. Omitted
  defaults and explicit false survive narrow form edits and mutation serialization.
- Validation mirrors current provider limits: HTTPS issuer without userinfo/query/
  fragment, distinct core bootstrap references, RFC 6749 scopes and aggregate
  bounds, bounded GEP-2257 durations/retry ordering, memoryMaxKeys 1..10000.
- Added exact nested provider TLS resolved-certificate exclusions. Credential
  acquisition remains Controller-owned; Center does not read bootstrap secrets.
- Unit suites passed 14 utility cases (42508), plus four form cases (29341).
  Production build 65677, lint 76643, E2E typecheck 66540, and final TS/new-test
  lint 37500 passed. Logs /tmp/ws5-center-credential-{tests,form,build,lint}.log.
- Native browser 50832 passed auth plus both typed OTLP and credentialSource CRUD
  with Form/YAML round trips and Controller readback. Log:
  /tmp/ws5-center-credential-runtime-v1.log. All owned services stopped; 70 seed
  fixtures retained. The missing bootstrap fixture intentionally prevents issuer
  access; this proves configuration management, not token acquisition/rotation.
- Full frontend regression currently runs as **11255**, log
  /tmp/ws5-center-all-web-current.log. Poll this exact handle before restarting.
- Next confirmed drift: Redis timeout.connect/command are GEP-2257 strings;
  db allows 0..255, retry/observability and pool.minIdle were removed, cluster
  readFromReplicas was removed. Kafka adds maxTopics/maxPendingRecords/
  maxPendingBytes. Audit validation and controls before marking LinkSys complete.
- Full frontend run 11255 is now terminal success: **456 tests across 88 files**.
  No active handles remain for this checkpoint. Existing Rust matrix findings are
  unchanged because these batches modify frontend, tests, and documentation only.

### Redis/Kafka current-field alignment

- CredentialSource batch committed as 69ad738, no push.
- Redis now uses duration-string connect/command timeouts, db 0..255, pool 1..64,
  redirects 0..64. Removed stale retry/observability/read-write timeout/minIdle/
  replica-read controls and types; YAML retains obsolete data but submission
  reports the removed fields rather than silently discarding them.
- Corrected topology admission: standalone exactly one endpoint; cluster permits
  omitted optional settings; Sentinel uses only its nested endpoints. Topology
  switches clear incompatible known fields. Added endpoint/transport checks and
  prevented URL normalization from concealing unsupported paths.
- Kafka adds maxTopics/maxPendingRecords/maxPendingBytes controls and types;
  all capacities are positive safe integers, zero linger remains valid.
- Initial targeted run 84831 had one obsolete message expectation (standalone
  changed from at-least-one to exactly-one endpoint). Corrected the expectation;
  utility rerun passed 14 tests, log /tmp/ws5-center-redis-tests-v2.log. Form rerun
  50388 passed four cases; build 90648, lint 22285, E2E typecheck 58874 passed.
- Native 64408 passed auth and both typed Redis Sentinel/Kafka browser CRUD,
  form/YAML preservation and Controller readback. Log:
  /tmp/ws5-center-redis-runtime-v1.log. All owned services stopped; 70 fixtures
  retained. No Redis/Kafka server was contacted by Center; no live data-plane
  connectivity claim is made by this management test.
- Next: remaining LinkSys provider schemas (Elasticsearch, etcd, HTTP DNS,
  Webhook), advanced provider workflows, then remaining resource/menu ledger.

### Remaining LinkSys variants and validation

- Redis/Kafka batch committed as add464b, no push.
- Current Elasticsearch and etcd schema fields are present in Center types and
  structured/advanced editors. Added typed native CRUD cases exercising timeout,
  pool/bulk/index and keepalive/namespace/message-size configuration respectively.
- HTTP DNS was missing fallback DNS server validation. Added literal IPv4/IPv6
  and socket parsing (including bracketed IPv6 and port zero, matching Rust).
  Switching fallback away from dns clears its variant-owned servers field.
- Webhook success.body now enforces the Controller's 4096-byte response cap floor.
  Added typed HTTP DNS and Webhook cases with response/fallback/TLS and body/retry
  options; narrow edits preserve all supplied siblings in YAML and API readback.
- Utility suites 44406 passed 13 tests. First build 16741 found two test-only
  union-property typing errors; explicit concrete config types fixed them.
  Build 31270 passed; lint 25870 and E2E typecheck 56260 passed.
- Native run 97115 passed all five cases (auth plus Elasticsearch, etcd, HTTP DNS,
  Webhook). Log /tmp/ws5-center-remaining-link-runtime-v1.log; owned processes
  stopped, 70 fixtures retained. Unit/build logs:
  /tmp/ws5-center-remaining-link-tests.log and
  /tmp/ws5-center-remaining-link-build-v2.log. No live handles remain.
- All eight current LinkSys variants now have typed CRUD evidence across these
  batches, but this does not prove live backend service integration or every
  advanced permission/status/error workflow. Continue the full resource/menu
  ledger; next infrastructure pass is GatewayClass, Gateway, ReferenceGrant.

### Infrastructure resource boundary pass

- Remaining LinkSys batch committed as db5c8c5, no push.
- ReferenceGrant normalization no longer rewrites kind or injects namespace into
  existing YAML/API documents. It requires correct identity and preserves the
  original operator document. Submission validates 1..16 from/to entries, group,
  kind, and source namespace against the current vendored CRD. Core empty groups
  and extension resource kinds remain permitted; no resource-kind whitelist.
- Gateway mutation filtering now removes Controller-owned inbound PROXY policy
  and attachment proof. Added 64-entry listener/per-port TLS bounds and explicit
  errors for current unsupported allowedListeners/infrastructure/defaultScope.
  Same-named nested operator values remain intact.
- Three adapter suites 54245 passed 12 tests; build 39131 and lint 7292 passed.
  Native CRUD 46514 passed auth plus GatewayClass/Gateway/ReferenceGrant, including
  form/YAML round trips and Controller readback. Log:
  /tmp/ws5-center-infra-runtime-v1.log. Owned services stopped, seeds retained.
- Infrastructure action/retry browser run **4293** is active; log
  /tmp/ws5-center-infra-actions-v1.log. Poll before restarting. Covers list search,
  refresh, view/edit/new dialogs, delete/batch confirmation cancellation, and a
  bounded list API failure recovery. It does not itself execute batch deletion.
- Cross-namespace grant activation/revocation, listener attachment behavior,
  Kubernetes status propagation, and the rest of the menu ledger remain open.
- Action run 4293 finished successfully: all five tests passed (auth, three
  resource action pages, list retry recovery). All handles terminal; owned
  processes stopped and exact fixtures retained. Ready for Center commit.

### Service and EndpointSlice preservation/menu pass

- Infrastructure batch committed as e07c231, no push.
- Edgion uses the upstream k8s-openapi Service/EndpointSlice types. Their Center
  adapters still merged create defaults into existing API/YAML documents, unlike
  the other updated adapters. Removed that merge on reads; only createEmpty uses
  defaults. Existing missing namespace/selector/ports/labels are not fabricated.
  Wrong resource identities and malformed required structures are rejected.
- Added lossless cases for ExternalName Service and IPv6 EndpointSlice with false
  readiness, serving/terminating flags and topology hints. Empty and absent values
  retain their distinction; mutation filtering still removes server metadata.
- Utility run 35067 passed six cases; build 16390 and lint 60189 passed. Native
  run 31908 passed six cases: auth, both list action pages, both resource CRUD
  flows, and actual isolated Service single/batch deletion with API absence
  checks. Logs /tmp/ws5-center-service-{tests,build,lint}.log and
  /tmp/ws5-center-service-runtime-v1.log. All handles terminal, owned services
  stopped, 70 exact fixtures retained.
- Current EndpointSlice ready count matches Gateway's explicit-ready-only logic
  (discovery/coordinator.rs and endpoint_slice/discovery_impl.rs use false when
  absent). Do not change it merely to a generic Kubernetes assumption. Gateway
  discovery currently accepts IPv4/IPv6, not FQDN; the generic resource editor's
  FQDN option does not establish routing support. Advanced discovery workflows
  still require validation. Remaining security/routes/fleet/admin menus stay open.

### Security menu and typed mTLS identities

- Service/EndpointSlice batch committed as 36f830c, no push.
- EdgionTls.allowedSans had the obsolete string[] type and no form controls.
  Added typed DNS/URI/Email/IP/OtherName entries with Exact/Prefix/Suffix/Contains/
  RegularExpression matching, ignoreCase and OtherName OID, plus allowedCns.
  CA reference edits retain group/kind/unknown fields; parent reference types now
  include sectionName and port. Existing unknown fields remain lossless.
- Submit-time structural checks cover 16 hosts/32 parents, verification depth,
  typed SAN shape, nonempty value, match vocabulary, OID, and regex source byte
  bound. Rust regex compilation and exact SAN semantic admission remain owned by
  Controller; frontend does not substitute JavaScript regex behavior.
- Adapter tests 6343 passed two cases; form test 30257 passed. Lint 20212 passed.
  Initial build 98913 passed before the new optional-field test; final typecheck
  63841 found that test's missing non-null assertion. Corrected test typing;
  final build **36636** is running, log /tmp/ws5-center-mtls-build-v2.log.
- Native security browser run 23148 passed all nine cases: auth; action pages for
  EdgionTls/BackendTLSPolicy/Secret/ConfigMap; and each resource's real CRUD.
  TLS fixture includes typed URI/OtherName SANs and CNs, preserved on readback.
  Log /tmp/ws5-center-security-runtime-v1.log. Owned services stopped, 70 exact
  fixtures retained. This is configuration/UI evidence, not mTLS handshake proof.
- Security authorization denial, live certificate verification and cross-namespace
  attachment behavior remain open; continue route/fleet/admin menu coverage.
- Final build 36636 passed. All checkpoint handles are terminal.


### BackendTLSPolicy identity admission follow-up

- Previous typed mTLS batch committed as ae36a4a, no push.
- Compared current backend_tls_identity_validation_errors and gwapi_types with
  the Center submission validator. Added precise SNI hostname checks, the 1–5
  SAN bound, typed-field exclusivity, wildcard Hostname syntax, and absolute URI
  shape/253-byte bound. Browser URL parsing is a preflight; Controller Rust URL
  parsing remains authoritative.
- Form limits targetRefs to one and SANs to five; removing the last SAN omits the
  field. Editing a SAN value preserves unknown sibling fields.
- Updated BackendTLSPolicy resource guide from obsolete v1alpha3/basic-only notes.
- Build 3930 and lint 64247 passed. Initial parameterized test table incorrectly
  spread array rows; fixed the test table. Final test handle 83182 passed all
  17 adapter/editor submission cases. Logs /tmp/ws5-center-btp-identity-*.log.
- No native browser or handshake rerun in this follow-up; prior security CRUD
  coverage is recorded above. Security runtime, routes, fleet and administration
  coverage remain open. All current tool sessions terminal.

### Route menus and explicit zero backend weight

- BackendTLSPolicy identity batch committed as c524197, no push.
- Inspected all stream-route adapters, shared editor and current TLSRoute schema.
  Controller gateway.rs still explicitly rejects TLS/Terminate; do not advertise
  termination from Center. Alternate source versions remain a separate inventory
  contract and were not changed based on older canonical-only prose.
- Fixed shared BackendRefsEditor converting weight zero to undefined. Gateway's
  backend runtime defaults absent weight to one; explicit zero must survive form
  edits. The new real Ant Design input test distinguishes zero from clearing and
  asserts preservation of another backend and unknown fields.
- Unit handle 43998 passed eight cases across backend input, stream form and
  route adapters. Build 18597 and lint 23199 passed. Logs:
  /tmp/ws5-center-route-weight-{tests,build,lint}.log.
- Native handle 28521 passed eleven cases: auth, list actions and real CRUD for
  HTTPRoute/GRPCRoute/TCPRoute/UDPRoute/TLSRoute. Log:
  /tmp/ws5-center-route-menus-v1.log. Owned services stopped; 70 exact seeds
  retained. Action cases cancel deletion confirmations; CRUD executes isolated
  deletion. The zero-input regression is component-level evidence, not a claim
  that the browser CRUD cases specifically exercised zero weights.
- Forwarding, advanced route features and cross-namespace authorization remain
  open, as do the remaining fleet/admin menus. All current sessions terminal.

### Administration menus and reload completion evidence

- Route weight batch committed as ebeb3cf, no push.
- Native standalone run 31774 passed nine cases: password login, identity API,
  shell language/navigation/reload/logout, Controller search/filter/reload/entry,
  both dashboards/topology controls, audit filters/reset/pagination, Controller
  deletion confirmation cancellation, isolated role/user CRUD/status/password/
  membership changes, and Controller switching. Log:
  /tmp/ws5-center-admin-menus-v1.log. This does not prove restricted-role denials,
  changed-password login, OIDC, actual Controller deletion or Kubernetes behavior.
- Existing reload test accepted failed/unknown outcomes, which proved response
  handling but not completion. For the healthy single-owner standalone topology,
  it now reads both initial server IDs through the federation HTTP tunnel,
  requires converged/200 with a changed nonempty ID, then reads Controller
  server-info through the same tunnel and requires that exact ID and ready=true.
  Kubernetes retains its multi-replica outcome handling contract.
- Strengthened native reload run 55078 passed auth plus Controller page flow;
  log /tmp/ws5-center-reload-proof-v1.log. E2E TypeScript handle 84487 passed.
  Both runtime runs stopped their owned services and retained 70 exact seeds.
  All tool sessions terminal; production code and federation contracts unchanged.
- Updated stale Pending cells with actual partial evidence. No broad coverage
  row is declared complete; remaining requirements stay visible in the ledger.

### Standalone database authentication and RBAC denial evidence

- Reload proof batch committed as 4a26155, no push.
- Added opt-in E2E_RBAC=1 runtime rendering (DB authentication + RBAC), retaining
  the existing allow_all default. Documented required database admin bootstrap
  environment and fresh run/database. Rendering rejects invalid toggle values.
- Added an isolated roleless user lifecycle case with a fresh request context:
  successful login/me, 403 for users/roles/audit/Controller proxy, password reset
  rejecting the old password, disabled-login rejection, and successful login
  after reactivation. Finally deletes exactly the created user. This proves the
  Center authorization boundary, not the independent Controller RBAC boundary.
- First run 32335 failed because creation correctly returned 201, not the test's
  assumed 200. Second run 86458 exposed that the existing fixture enabled only
  single-admin auth/allow_all, so the DB user's login correctly failed. Neither
  was a production defect; the test now requires the explicit RBAC topology.
  The first failed run's isolated artifact database may retain its test user;
  no shared or user-owned database was modified.
- RBAC run 31717 passed both auth and the full lifecycle case (14.8 s); log
  /tmp/ws5-center-denial-v3.log. E2E typecheck 70949 passed. Owned runtime stopped,
  70 exact seeds retained. All sessions terminal. Existing-token revocation,
  browser restricted-menu behavior, OIDC and Kubernetes remain open.

### Restricted-user browser navigation

- Database/RBAC harness committed as cbd2029, no push.
- Extended the restricted-user lifecycle with a fresh browser context and real
  login using the changed password. Verified Users/Roles/Audit Log navigation
  buttons are absent, and direct /users, /roles and /audit visits redirect to
  the root without rendering their management controls. Existing API denials
  and password/status lifecycle assertions still run in the same scenario.
- Native RBAC handle 3413 passed auth plus the complete scenario (18.1 s), log
  /tmp/ws5-center-browser-denial-v1.log. E2E typecheck 34409 passed. Owned runtime
  stopped, 70 exact seeds retained, all sessions terminal. No production change
  was necessary. This covers the roleless-user case; individual permission
  combinations, existing-session revocation, OIDC and Kubernetes remain open.

### System and TLS developer-guide reconciliation

- Restricted browser batch committed as fa5a300, no push.
- Replaced obsolete GatewayConfig examples with a field map checked against
  current edgion_gateway_config.rs, Center TypeScript and form controls. Removed
  guidance to edit process settings/maxRetries/enableReferenceGrantValidation,
  and the unsupported singleton suggestion. Recorded grouped RealIp, retry,
  forwarded headers, plugin policy, request-body and remaining current fields.
- Corrected the TLS guide's scalar allowedSans examples and cipherSuites name
  to typed SANs and ciphers; documented current identity variants and resource
  bounds from edgion_tls.rs/allowed_san.rs.
- These are targeted knowledge corrections, not new claims of runtime coverage.
  Reviewed the documentation diff and checked whitespace. No executable code
  changed, so no application tests rerun. Other older guide sections and the
  remaining menu/runtime ledger still require their own audits.

### Trusted proxy IP/CIDR validation

- Guide reconciliation committed as b11bade, no push.
- GatewayConfig treated any colon-containing address as IPv6 and ignored extra
  slash segments. Replaced this with shared literal IP helpers extracted from
  the existing DNS endpoint validator, strict characters and exact CIDR shape.
  IPv4/IPv6 prefix bounds follow current radix_ip/types.rs; host bits and Rust's
  optional plus on prefix integers remain accepted. No operator normalization.
- Malformed YAML groups/CIDR collections now yield validation errors instead of
  throwing on null or non-array values. Tests include bogus IPv6, extra slash,
  newline, zone/socket syntax, mapped IPv6, zero prefixes and null values.
- Initial and final targeted runs passed 19 cases (final handle 42543), covering
  IP syntax, DNS/LinkSys regression and GatewayConfig admission. Build 49595 and
  lint 56854 passed before the final strict-character refinement; final targeted
  tests passed afterward. Logs /tmp/ws5-center-cidr-*.log. No native browser or
  traffic rerun for this adapter-only correction; runtime coverage remains open.

### Provider account edit preservation and revision capture

- Trusted proxy batch committed as 89ced35, no push.
- Audited cloud architecture/security guides and account API/editor. Cloud DNS
  is independent of Edgion/federation and provider calls require explicit
  enabled composition. No external account or DNS was accessed in this pass.
- Confirmed account editing discarded labels and fetched a fresh ETag only at
  save time, allowing old form values to overwrite concurrent account changes.
  Editor now opens from one fetched account/ETag snapshot, preserves its labels,
  and saves with that captured revision. It does not refresh the precondition
  at submission. Existing server generation CAS rejects a concurrent writer.
- Added a form regression distinguishing list/open/save snapshots, asserting
  preserved labels and one GET only. Cloud suite 21305 passed ten cases; lint
  12833 passed. Build 37353 caught a Testing Library/Playwright option mismatch
  in the new test; removed unsupported exact options. Build 10229 then passed.
  Logs /tmp/ws5-center-cloud-edit-*.log; all sessions terminal.
- Native account CRUD/conflict proof and provider-specific live DNS workflows
  remain open. This batch is component and source-level evidence only.

### Provider account conflict and native browser verification

- Account revision fix committed as 4bda2fe, no push.
- Added missing-revision load feedback and component tests for blocked edits,
  retained conflict drafts, captured ETag and no automatic retry. Native browser
  verification found delayed Modal/Form mounting cleared prefilled values;
  seeded Form initialValues from the captured account and retained synchronization
  for mounted forms. This was not visible in the component mock runtime.
- Added standalone cloud-account metadata browser test: create isolated account,
  open edit, concurrently update via real API, require 412 and unchanged stored
  concurrent values, reopen and save with the new revision while retaining labels.
  No provider adapters, DNS mutation or credential inspection are involved.
  Account has no delete endpoint and remains in the isolated run database.
- Native attempts: 70651 stopped on wrong test API prefix (404); 82780 and 46405
  exposed empty edit inputs; corrected rendering, then 17657 passed both auth
  and full account conflict/edit scenario. Logs /tmp/ws5-center-cloud-metadata-v*.log.
  Owned runtimes stopped and exact seeds retained. E2E typecheck 99301 passed.
- Component runs 76952/63363 passed 12 cases; final rerun 79373 pending at this
  checkpoint. Build 16560 passed; lint 61877 passed before the initialValues fix.
  Provider-specific live DNS workflows remain unverified; this closes only local
  account metadata editing and concurrency evidence.
- Final component run 79373 passed all 12 cases. All current sessions terminal.

### Provider account browser creation and full frontend regression

- Native conflict/rendering batch committed as 2fc68ab, no push.
- Extended the real account browser scenario to create through the actual form,
  validate stored default provider/management/credential fields, then seed labels
  via the API before the existing edit/CAS conflict checks. After saving an edit,
  opening Create must show blank account/name/credential reference fields.
- Native handle 89016 passed auth plus complete create/edit/conflict/reset flow;
  log /tmp/ws5-center-cloud-create-v1.log. No cloud-provider calls were made.
  Owned services stopped; account metadata remains only in the run artifact DB.
- E2E typecheck 46194 passed. Full frontend regression 33364 passed **489 tests
  across 93 files** in 69.89 s, log /tmp/ws5-center-full-web-latest.log. This is
  current broad frontend evidence, separate from the earlier backend matrix and
  from unverified live DNS/Gateway workflows. All tool sessions terminal.
- Updated Cloud ledger status to reflect verified local account behavior without
  claiming provider-specific DNS completion.

### Plugin/system menus and optional boolean controls

- Cloud creation batch committed as 14dcd78, no push.
- Native handle 50326 passed thirteen cases: auth plus list actions and actual
  CRUD for GatewayConfig, HTTP plugins, stream plugins, ConfigData, ACME and
  LinkSys. Log /tmp/ws5-center-system-actions-v1.log. Owned services stopped,
  70 exact fixtures retained. Cases exercise generic resource workflows, not
  every plugin configuration or data-plane execution.
- Known boolean fields previously displayed an unchecked switch when absent,
  suggesting false even for server defaults such as RealIp.recursive=true.
  Replaced that control with an explicit true/false selector whose absent value
  stays unset. Existing Clear removes the field. No default is materialized into
  the resource; unknown siblings and false are retained.
- Added omitted/false/clear regression and updated stream GeoIP form interaction.
  Initial runs 24595/17525 exposed outdated switch selectors and duplicate hidden
  AntD option text in tests. Corrected visible-option queries; final 52800 passed
  ten cases. Build 30752 and lint 79033 passed. Logs:
  /tmp/ws5-center-plugin-boolean-*.log. All current sessions terminal.
- Generic browser coverage above does not specifically prove the new boolean
  interaction. The per-plugin nested schema and runtime audit remains open.

### Rewrite plugin audit and YAML structure boundary

- Optional boolean batch committed as 539eab3, no push.
- Checked ProxyRewrite and ResponseRewrite top-level catalog fields against
  current Rust config structs. All current operator fields are represented;
  nested patch/header validation and execution remain Controller/Gateway-owned
  and are not marked verified by this top-level comparison.
- Found YAML parser returned unvalidated objects while form normalization only
  checked broad object types. Unified YAML parsing with normalization and reject
  array metadata/spec, non-array stages, null/scalar plugin entries, missing
  string type and non-object configs before rendering/submit. Optional null
  stage values, unknown plugin names and unknown fields remain lossless; this
  is editor structure validation, not acceptance of unknown runtime plugins.
- Updated misleading adapter comments that claimed defaults/empty values were
  rewritten. Added malformed-input and unknown-field preservation cases.
- Fifteen adapter tests passed; build 96120 and lint 54678 passed. Logs
  /tmp/ws5-center-plugin-shape-{tests,build,lint}.log. No native rerun for this
  structural guard; prior generic plugin CRUD remains separate evidence. All
  sessions terminal; nested plugin semantics and runtime coverage remain open.

### Stream plugin structural boundary

- HTTP YAML guard committed as 1fbeee4, no push.
- Checked current stream schema: plugins and tlsRoutePlugins are independently
  optional lists. Center normalization had accepted scalar/array metadata/spec
  and malformed entries that the form could not render. Applied the same list
  boundary as HTTP plugins using one shared validatePluginStages helper rather
  than maintaining duplicate checks.
- Both stream stages retain null/absent values, unknown plugin names and unknown
  config fields. Wrong container/entry/config shapes fail before form rendering.
  This does not replace Controller semantic validation or widen supported types.
- Nineteen HTTP/stream adapter tests passed, covering both stages and lossless
  mutations; build 6620 and lint 35329 passed. Logs
  /tmp/ws5-center-stream-shape-{tests,build,lint}.log. All sessions terminal.
  No new browser/data-plane evidence claimed; remaining nested schema and menu
  runtime audit stays active.

### RegionRoute failover and restore native proof

- Restored stable action selectors on the current Override page and updated the
  obsolete restore-option locator. Browser checks now require both standalone
  Controller outcomes to be converged, identify the exact namespace/name in the
  global watch view, and verify both Controllers after failover and restoration.
- Native v1 exposed obsolete selectors; v2 reached real writes and correctly
  rejected unversioned file seeds. Direct file installation bypasses Admin API
  creation/version assignment. Standalone seeds now persist an initial CAS token;
  Controller owns later versions. Production CAS validation was not relaxed.
  Manually installed unversioned files still cannot receive Center CAS writes.
- Native v3 passed browser assertions but exposed the retain script's seed-hash
  assumption after legitimate writes. Retain now checks exact run labels, keeps
  the original deletion hashes, and reports changed files. Deletion still refuses
  changed content. Isolated scratch checks verified retain, unchanged hash,
  rejection of modified deletion, and rejection of foreign labels.
- Final run alignment-region-20260928-v4 passed authentication plus browser
  failover/restore (2 tests), retained 70 files with 2 changed, and exited zero
  (session 19289). Log: /tmp/ws5-center-region-v4.log. Earlier failed runs remain
  available under their unique artifact directories.
- Eight RegionRoute unit tests, E2E typecheck, lint and frontend build passed;
  logs /tmp/ws5-center-region-{unit,types,lint,build}.log. Backend source unchanged;
  prior matrix's unrelated English-only guard failure remains recorded above.
- This proves configuration CAS and watch convergence, not Gateway traffic
  redirection. RegionRoute conflict/sync actions, runtime traffic, Kubernetes
  ownership behavior and the remaining full menu/type audit remain open.

### RegionRoute source-to-target synchronization native proof

- Added inventory-backed selectors for source selection and synchronization.
  The native browser case creates a real CAS-protected divergence only on B,
  observes the inconsistent row and disabled failover action, explicitly selects
  B as source, and synchronizes to A through the page.
- The response must contain one converged outcome for A. Both Controller watch
  documents must agree afterward; A retains its own labels and changes version,
  while B retains its version. The page removes the sync warning and re-enables
  failover. Finally both resources return to no failover.
- Run alignment-region-sync-20260928-v2 passed all 3 cases (login, failover/restore,
  selected-source sync), retained 70 exact files with 2 changed, and exited zero
  (session 74201). Log: /tmp/ws5-center-region-sync-v2.log. Initial v1 stopped
  before runtime because the new selectors needed action-inventory entries.
- E2E typecheck 94405, frontend build 1350, and lint 50879 passed. Logs:
  /tmp/ws5-center-region-sync-{types,build,lint}.log. All processes terminal.
- Remaining RegionRoute audit includes mixed/non-converged outcomes under watch
  refresh, enable-state differences (consistency includes enable while sync
  intentionally copies only spec.data), missing-resource sync, and traffic-level
  proof. These are open inspection areas, not conclusions from this happy-path run.

### RegionRoute failover outcome lifetime

- Reproduced loss of per-controller outcomes: refetch inside the mutation changed
  the row to inconsistent, and FailoverAction replaced the Popover with a disabled
  tooltip. Even consistent refetches could reset the editor through its
  config-derived React key. Component regression failed before the fix because
  the conflict outcome vanished.
- Keep one editor snapshot per explicit opening, retain the active component
  through watch updates, and disable selects/apply when the live row becomes
  inconsistent. A new opening uses the latest document; automatic close still
  requires all outcomes to be converged.
- Added three full-page component cases for converged/conflict, accepted/unknown,
  and converged/superseded while refetch changes the underlying documents.
  Eleven RegionRoute tests passed (session 59900); initial reproduction log
  /tmp/ws5-center-region-outcomes-before.log and fixed results
  /tmp/ws5-center-region-outcomes-final.log.
- Build 39028 and lint 1191 passed. Native run
  alignment-region-outcomes-20260928-v1 passed all 3 login/failover/restore/sync
  cases and retained 70 files (session 94545, terminal zero). Logs
  /tmp/ws5-center-region-outcomes-{build,lint,native}.log.
- Mixed outcomes are deterministic component evidence, not injected native
  transport failures. Sync-action outcome lifetime, enable differences, missing
  resources and Gateway traffic remain separate open audit items.

### RegionRoute sync outcome lifetime

- Reproduced all three non-confirmed sync results disappearing when refresh made
  the row consistent: accepted, unknown and superseded. The consistency cell
  previously unmounted SyncOverrideButton together with its result state.
- Keep the cell and sync component mounted across consistency transitions; hide
  only the source/apply controls when the documents agree. Retain the operation
  result without inferring convergence from the later document comparison.
- Three regression cases failed before the change. Fourteen RegionRoute tests
  now pass (session 44176); logs /tmp/ws5-center-sync-outcomes-{before,after}.log.
- Build 64894 and lint 26243 passed. Native run
  alignment-sync-outcomes-20260928-v1 passed login, failover/restore and selected
  source sync (3 tests), retained 70 files, and exited zero (session 44015).
  Logs /tmp/ws5-center-sync-outcomes-{build,lint,native}.log. All sessions terminal.
- These deterministic component cases verify result presentation; they do not
  establish transport-failure injection or Gateway traffic behavior. The full
  type/menu objective remains active, including RegionRoute enable/missing
  resource boundaries and the other open coverage rows.

### RegionRoute enable and missing-resource boundaries

- Verified current Edgion: envelope spec.enable is the actual kill switch,
  default true; the nested RegionRouteOverride.enable is reserved/inert. Center
  sync copies spec.data only, and its CAS write core cannot create a missing
  target. Retained this contract.
- Page now explains enable-state divergence and missing target IDs, pointing
  operators to per-Controller EdgionConfigData creation/editing. Disabled override
  summaries explicitly state that base routing applies, rather than showing
  stored failover settings as active.
- Added component coverage for both explanations and disabled failover, plus
  consistency coverage for omitted enable versus true/false. Seventeen tests
  passed (99425), build 13930, lint 85308 and E2E typecheck 43560 passed.
- Native alignment-region-boundaries-20260928-v1 passed 5 cases including the
  new enable-preservation workflow, failover/restore, selected-source sync,
  login and operation discovery. Data sync reported convergence while B remained
  disabled and the page correctly retained its enable warning/inconsistent
  state. The test restored B's enable switch. Session 42554 exited zero and
  retained 70 exact files. Logs /tmp/ws5-center-region-boundaries-{unit,build,
  lint,e2e-typecheck,native}.log. All sessions terminal.
- Missing-resource behavior currently has source/component evidence; native
  missing-target failure/recovery and Gateway traffic remain unverified. Other
  resource/menu rows remain open.

### RegionRoute missing-target native recovery

- Added a native case that verifies the exact run label and CAS token before
  deleting only B's run-owned override. The browser waits for the missing-target
  warning, checks disabled failover, submits data sync, and requires HTTP 502
  with exactly one failed outcome for B and no modified targets.
- Verified the failure remains visible, B is still 404 (no implicit creation),
  and A's resourceVersion is unchanged. Explicitly recreate B in finally using
  the saved operator spec/labels and no generated metadata; then require both
  watch documents, disappearance of the missing warning, enabled failover and
  preserved run label.
- Native alignment-region-missing-20260928-v1 passed all 6 cases, including the
  prior failover/restore, source sync, enable-preservation and discoverability
  checks. Session 30504 exited zero; 70 exact files retained. E2E typecheck 82723
  and lint 67758 passed. Logs /tmp/ws5-center-region-missing-{native,types,lint}.log.
  All sessions terminal. No production code changed in this batch.
- This closes the previously missing native failure/recovery evidence. Gateway
  traffic and Kubernetes ownership remain distinct unverified paths; the full
  Center type/menu alignment goal remains active.

### Backend traffic policy structural boundary

- Continued the per-kind audit with current EdgionBackendTrafficPolicy schema.
  Its current top-level operator sections already have Center controls; malformed
  YAML containers/references could nevertheless reach form array/string methods.
- Added shared normalization/validation structure checks for metadata/spec,
  target arrays and entries, optional object sections, numeric expected-status
  lists and authority strings. Invalid drafts now yield validation errors before
  form rendering. Unknown fields, omitted defaults and optional null sections
  remain lossless.
- Corrected AI healthCheck presence validation: null is absent for the current
  Rust Option, so it must not be mistaken for an enabled unsupported probe.
  Added malformed-input and null/unknown-field preservation cases.
- Thirty-eight adapter/form/editor tests passed (81766); build 52549 and lint
  99724 passed. Native alignment-ebtp-structure-20260928-v1 passed authentication,
  resource actions and real browser CRUD (3 tests, 73063 terminal zero), retaining
  70 files. Logs /tmp/ws5-center-ebtp-structure-{tests,build,lint,native}.log.
- Added the missing dashboard guide/router entry for this policy. This batch
  establishes editing boundaries, not runtime balancing/probes/ejection/retry
  semantics or all target attachment cases. Those and the remaining menu/type
  rows stay open. No Edgion changes or commits.

### HTTPS and GRPCS active health checks

- Current Edgion health_check.rs supports five probe modes and independent
  OutboundTlsLocal configuration. Center still allowed only http/tcp/grpc and
  omitted new Controller-resolved health TLS fields from mutation filtering.
- Added https/grpcs types/options, HTTP fields for HTTPS and gRPC service fields
  for GRPCS, plus a TLS JSON editor covering hostname, CA/SAN settings and client
  identity references. Basic encrypted-probe TLS/verification/hostname checks
  precede submission; full identity, trust and namespace validation stays with
  Controller. Invalid TLS drafts and HTTP-status drafts aggregate independently.
- Mutation filtering now drops exactly healthCheck.active.resolvedCaCertificates,
  resolvedClientCertificate and resolvedTlsError, preserving operator references
  and unknown sibling fields. Added HTTPS/GRPCS round-trip, resolved-field removal,
  form TLS edits and simultaneous draft-error tests.
- Final 63 tests passed (91329); build 20414 and lint 53906 passed. Native run
  alignment-health-tls-20260928-v1 used a HTTPS fixture and passed login, policy
  page actions and full browser CRUD (3 tests, 21255 terminal zero). All 70
  fixture files retained. Logs /tmp/ws5-center-health-tls-{tests,build,lint,native}.log.
- The Controller binary postdates the inspected health-check source. No Edgion
  edits or commits. Native evidence proves HTTPS configuration round-trips,
  not a successful probe handshake; GRPCS editing has component/adapter evidence.
  Runtime probes and remaining policy/menu cases stay open.

### Backend policy list completeness and full frontend regression

- Full frontend regression after encrypted-probe editing passed 527 tests across
  95 files (29260 terminal zero, /tmp/ws5-center-full-web-health-tls.log).
- Found the list omitted retryConstraint, circuitBreaker and connection tags;
  policies configured only with those sections appeared to have no features.
  Added localized tags and the probe protocol to the health-check tag. These
  report configuration presence, not live health or probe success.
- Three targeted list tests passed (55584). Build 40384, lint 15466 and E2E
  typecheck 45344 passed; /tmp/ws5-center-policy-list-{tests,build,lint,types}.log.
- Native v1 exposed a too-specific exact text-node locator although the visible
  row contained the expected labels. Switched to asserting text on the visible
  resource row. Final alignment-policy-list-20260928-v2 passed login plus page
  actions and all four new labels (2 tests, 35002 terminal zero), retained 70
  fixtures. Log /tmp/ws5-center-policy-list-native-v2.log. All sessions terminal.
- Full regression preceded this final list-only change; the list change has the
  targeted component/native/build evidence above. Remaining deeper target-policy
  and runtime behavior is still open.

### AI backend traffic policy applicability

- Current Controller evaluate_policy rejects loadBalancer and upstreamAuthority
  for resolved AI targets; schema validation already rejects healthCheck.
  Center only rejected healthCheck and incorrectly displayed RoundRobin as the
  default algorithm for AI policies without a load-balancer section.
- Added both missing validation restrictions, a localized applicability notice,
  disabled creation of unsupported sections for homogeneous AI refs, and kept
  existing sections available for explicit removal (including an empty
  healthCheck envelope). No silent field deletion; supported siblings survive.
  Shared target classification removes the invented AI RoundRobin list default.
- Forty-eight adapter/form/editor/list tests passed (58400); build 42077, lint
  59028 and E2E typecheck 91480 passed. Initial component attempt only had
  inaccurate accessible label strings, corrected to current localized labels.
- Native alignment-ai-policy-20260928-v1 passed login, Service policy CRUD and
  the new AI policy create/form-YAML round-trip/save/readback/exact-delete case
  (3 tests, 39625 terminal zero). Verified unsupported switches disabled,
  supported sections preserved, and no RoundRobin list claim. All 70 seeds
  retained. Logs /tmp/ws5-center-ai-policy-{tests,build,lint,e2e-typecheck,native}.log.
- No Controller RBAC widening or Edgion edits. Attachment arbitration and live
  resilience execution remain separate from this configuration/UI proof; the
  full alignment objective stays active.


### Condition polarity across resource pages

- Audited shared status rendering against Edgion's fixed condition vocabulary.
  ResourceConditions colored every True value green, including Conflicted and
  PartiallyInvalid. This could present a conflicting policy or partially invalid
  route as successful in both lists and detail views.
- Color by known condition semantics: positive Accepted/ResolvedRefs/Programmed,
  negative Conflicted, warning PartiallyInvalid. Unknown statuses stay cautionary;
  future condition types are neutral. Preserve the original condition values,
  reasons and ancestor/writer context. Topology's existing conflict detection is
  independent and unchanged.
- Eight shared component tests passed (39892), covering compact and detailed
  display, negative conditions and unknown types. Build 69513 and lint 78484
  passed; logs /tmp/ws5-center-condition-colors-{tests,build,lint}.log. All terminal.
- This is rendering evidence only. Generation freshness and status propagation
  through real attachment changes remain open, along with the broader audit.


### Condition generation freshness in lists and details

- Pass resource metadata.generation through every ResourceConditions caller,
  including Gateway listener details and topology details. Older observedGeneration
  values now display as gold stale observations with both versions available,
  preserving the original condition and writer/ancestor context. Unknown versions
  do not manufacture staleness; stale reference observations cannot satisfy the
  current grant/denial browser selectors.
- Added numeric generation to shared and local resource metadata interfaces.
  Initial validation caught three missing metadata declarations and an unsafe
  HTTPRoute processed-response metadata access; all corrected before commit.
- Full frontend suite: 538 tests in 95 files passed (73487 terminal zero),
  including compact/detail multi-writer freshness, reference recovery, missing
  versions, list integration and existing route editor mutation tests. Build
  87244 and lint 41716 terminal zero. Logs:
  /tmp/ws5-center-generation-tests-v2.log,
  /tmp/ws5-center-generation-build-v2.log,
  /tmp/ws5-center-generation-lint.log.
- No native browser run or Controller change in this batch. Topology summary
  classification (resourceIssues), real attachment transitions, Kubernetes writer
  behavior and the remaining resource/menu audit are still open. Do not infer
  runtime propagation or full alignment completion from component rendering tests.


### Topology condition summaries

- Reuse the same condition-generation predicate in resourceIssues. An old
  observation contributes stale status, not a current rejection, unresolved-ref
  or conflict diagnostic. Other writers' current conditions still contribute.
- Surface PartiallyInvalid and condition-reported unresolved references on nodes.
  Keep these separate from missing-object placeholders so graph construction
  still follows every concrete reference. Badges coexist rather than hiding
  additional diagnostics. Reference-existence edges do not prove readiness.
- Twenty-eight relevant tests passed (37706 terminal zero): shared Conditions,
  issue classification, graph construction and rendered canvas badges. Lint
  86721 passed. Build first caught a test cleanup callback return-type error;
  fixed its void return, then build 19637 passed. Logs:
  /tmp/ws5-center-topology-status-{tests,lint,build-v2}.log.
- No runtime propagation claim or native browser run in this batch. The audit
  also found EdgionBackend absent from TOPOLOGY_KINDS/KIND_ALIASES/node styles;
  AI backend references and policy targets require the next source-backed pass.
  Full menu/resource alignment remains active.


### AI backends in topology

- Audited EdgionBackendSpec/AiCredentialPool/AiCredential and flattened SecretSlot
  against current Edgion source. Added the missing EdgionBackend inventory kind,
  alias, group identity, layer and legend style. HTTPRoute backendRefs and
  EdgionBackendTrafficPolicy targetRefs now connect to actual AI backend nodes.
- Follow only declared credentialPool secretRef and redisRef dependencies.
  Restricted Secrets stay metadata-only; no resolved credential values are used
  for relationship discovery. Tests cover foreign groups, missing dependencies,
  denied/unavailable inventory and preserved policy/route links.
- Eleven topology tests passed (15249), build 81848 and lint plus E2E typecheck
  7837 passed, all terminal zero. Native standalone run
  alignment-topology-ai-20260928-v1 passed 4 browser cases (64108 terminal zero):
  login, dashboard/topology actions, topology controls and AI backend/credential
  nodes plus redacted detail. All 70 run-owned seeds retained without changes.
  Logs /tmp/ws5-center-topology-ai-{tests,build,lint-types,native}.log.
- Browser evidence proves inventory/display and metadata-only dependency views;
  route/policy/Redis relationship variants have graph tests, not live AI traffic
  evidence. No provider requests or real credential use. The full audit continues.


### Topology cross-namespace authorization boundaries

- Current Controller route_utils::listener_allows_route_namespace confirms that
  Gateway parent attachment uses allowedRoutes (Same/All/Selector). The graph
  previously applied ReferenceGrant projection to all cross-namespace arrows,
  including reversed Gateway-to-Route attachment arrows with the wrong owner.
  Track attachment edge identities and exclude them from grant projection.
- A failed ReferenceGrant inventory fetch now produces an unknown check, even
  when validation is known enabled; an incomplete inventory cannot prove denial.
  Added AI credential grant matching and unavailable-inventory tests, alongside
  enabled/unknown parent-attachment cases.
- Fifteen topology tests passed (88343 terminal zero), build 90485 and lint 36966
  passed. Logs /tmp/ws5-center-topology-grants-{tests,build,lint}.log. No native
  browser run in this batch. Runtime hook still correctly treats Controller
  ReferenceGrant validation configuration as unknown; graph-only enabled cases
  do not claim that configuration is exposed by the API.
- Parent attachment acceptance remains Controller-condition evidence, not graph
  existence. Kubernetes selector evaluation and real traffic remain unverified;
  the overall resource/menu audit continues.


### Gateway native status details

- Rechecked current gateway.rs: GatewayStatus is native addresses/conditions/
  listeners, unlike GatewayClass's ControllerStatus envelope. Kept that correct
  boundary. GatewayHandler emits ListenersNotValid=True for listener port and
  inbound PROXY policy conflicts; shared condition tags now treat it as an error.
- Removed duplicate listener conditions from the Gateway-level detail section;
  each listener retains its own conditions, counters, supported route kinds and
  generation comparison. No condition content is dropped.
- Sixteen component tests passed (15941 terminal zero), including native address,
  two listener counters, route kinds, stale listener status, no duplicate tags,
  empty status and condition polarity. Build 35097 and lint 42923 passed.
  Logs /tmp/ws5-center-gateway-status-{tests,build,lint}.log.
- This batch has component evidence, no new runtime listener-conflict or traffic
  test. Broader per-resource/menu and Kubernetes ownership checks remain open.


### Full standalone RBAC regression and current policy arbitration

- Ran all 148 browser cases with real database RBAC, two native Controllers,
  current frontend and run-owned fixtures: alignment-full-rbac-20260928-v1,
  session 92554 terminal 1, /tmp/ws5-center-full-rbac-native.log. Result: 142
  passed, 5 failed, 1 Kubernetes-only namespace test skipped. All 22 resource
  CRUD cases, typed ConfigData/LinkSys variants, RegionRoute flows and restricted
  user denials passed. This run is not a green full-suite claim.
- Two failures were stale policy arbitration assertions. Current handlers for
  both backend policies emit Accepted=False/Conflicted, not the old standalone
  Conflicted=True/LostOldestWins condition. Corrected generated expectations and
  topology conflict classification, retaining generation checks.
- BackendTLSPolicy's add button is bounded to one target, but its remove button
  was only available above one target, leaving replacement controls unreachable.
  Allow removing the sole target in the draft, then adding its replacement;
  mutation validation still requires exactly one target. The action case now
  verifies the bound and exercises remove-then-add.
- Policy action selection matched a same-prefix conflict fixture. Use exact
  resource-name matching in action/generated list tests. Keep action assertions.
- The remaining failure was role modal cancellation timing. The failed screenshot
  showed the modal already gone; no component defect was established. The
  unchanged role/user case passed its focused rerun. Do not call it resolved
  without a clean full rerun; no timeout increase or assertion removal was made.
- Focused native RBAC run alignment-full-rbac-fixes-20260928-v1 passed login and
  all five prior failures (6 cases, 80658 terminal zero). All 70 seeds retained.
  /tmp/ws5-center-full-rbac-fixes-native.log. Twenty-three affected tests (28360),
  build 11350, lint 3836 and E2E types 73793 passed, all terminal zero, logs
  /tmp/ws5-center-full-rbac-fixes-{tests,build,lint,types}.log.
- Separately, the pre-fix full frontend suite passed 550 tests in 97 files
  (55454 terminal zero), /tmp/ws5-center-full-web-final-status.log. Current
  follow-up scope adds one classifier test. Full browser rerun remains next;
  Kubernetes, external-provider and Gateway traffic proof remain outstanding.


### Clean full standalone RBAC rerun

- At product commit 6e18420, ran the unfiltered native standalone browser suite
  with database RBAC and two current Controllers. Run:
  alignment-full-rbac-20260928-v2; session 40915 terminal zero. No product code
  changed during the run. Reused binaries had no newer Rust sources in the
  Controller/resources/common or Center crates checked before execution.
- Result: 147 passed, 1 Kubernetes-only namespace-boundary case skipped, zero
  failures, 8.9 minutes. All five prior failures passed, including the unchanged
  role/user cancellation and mutation flow. No timeout or assertion weakening.
- Independently inspected case-ledger.json: 112 unique standalone generated
  cases, all passed. The reporter checks the exact expected set. The 35 other
  passed cases cover authentication, menus, RegionRoute, typed variants and
  additional actions; the skipped case does not apply to standalone.
- All 70 run-owned seed files retained and verified, two changed by RegionRoute
  workflows with original deletion hashes preserved. Runner stopped its own
  services. Log: /tmp/ws5-center-full-rbac-native-v2.log. Detailed evidence:
  web/test-results/alignment-full-rbac-20260928-v2/{case-ledger.json,html/}.
- This establishes a green integrated standalone baseline for current menus,
  resource CRUD and exercised operations. It does not prove Kubernetes writer
  ownership/fencing, MySQL/OIDC, provider calls, Gateway traffic, or all advanced
  resource semantics. Keep the overall alignment goal active. Next audit should
  address those resource/backend behavior gaps rather than repeat this unchanged
  browser matrix without a new reason.


### Kubernetes preflight and generation-aware API state checks

- Read-only OrbStack check: context orbstack, node Ready. Existing namespaces
  include edgion-system and dragonfly-system; none were modified. The browser
  harness API preflight failed because center.edgion.io/v1alpha1 is not served.
  Log /tmp/ws5-center-kube-preflight.log. Do not report Kubernetes runtime proof.
  Provisioning should preserve existing CRDs/workloads rather than overwrite the
  shared local installation while pursuing the remaining mode coverage.
- Found the E2E API oracle only collected flat condition groups and accepted old
  generations. It now collects each identified controllers[] observation, avoids
  flat fallback when an envelope is present, and matches observedGeneration to
  metadata.generation whenever the latter exists. Versionless standalone cases
  retain their valid condition checks. Pure helper tests are independent of the
  dashboard component implementation.
- Five observation tests passed (19626), build 96534, lint 68884 and E2E types
  74078 passed, all terminal zero. Initially importing the full E2E API module
  into dashboard tests exposed different TS targets; extracted a pure observation
  module rather than changing production compilation settings. Logs:
  /tmp/ws5-center-oracle-generation-{tests-v2,build-v2,lint-v2,types-v2}.log.
- Native alignment-state-oracle-20260928-v1 passed login plus all six state cases
  on both Controllers (13 tests; 43593 terminal zero). All 70 seeds retained.
  /tmp/ws5-center-oracle-generation-native.log. Native run preceded the pure
  module extraction; final helper/import wiring was checked by unit/build/type
  gates. Envelope-specific evidence remains fixture-based, not Kubernetes live.
- Also inspected the backend federation integration runner: it calls a global
  kill_all utility during cleanup. Before using it for reconnect/eviction/default
  RBAC proof, isolate cleanup to owned PIDs and preserve run artifacts. No backend
  integration process was launched through that unsafe cleanup path this turn.
  Overall alignment remains active.


### Native federation backend lifecycle verification

- Updated run_center_test.sh to refuse occupied ports and stop only its own shell
  child jobs. Removed global kill_all calls from startup, normal cleanup and
  failure paths. Retain private run directories (umask 077) on both outcomes.
  Occupied-port experiment confirmed startup refusal without disrupting the
  unrelated listener. Shell syntax and diff checks passed.
- Initial run stopped because current Controller requires explicit
  conf_center.controller_name outside Kubernetes. Added the current identity
  field to generated configurations; no Controller code or production RBAC
  changes. Initial 42002 terminal 1 with retained diagnostic directory.
- Final run 92288 terminal zero: 27 passed, zero failed. Covered registration,
  StatsReport counts, multi-namespace cached reads, proxied create/update/delete
  and CAS conflicts, default Secret/other-kind write denial, RegionRoute terminal
  outcomes, reload re-watch, disconnect/offline/reconnect/resync/eviction, a
  4,801,350-byte proxied list, and terminal deny-all watch refusal with a stable
  denial count. /tmp/ws5-center-federation-native-v2.log.
- Artifacts retained at
  /var/folders/tn/mmms5lc161v541bskkg3rtz00000gn/T/tmp.9MQUX9HdGC.
  Verified all recorded child PIDs stopped and all 16 test endpoints closed.
- This adds backend lifecycle/default-policy evidence beyond the browser fixture
  RBAC. It does not establish Kubernetes Lease/fencing, MySQL/OIDC or Gateway
  data-plane behavior. Overall alignment remains active.


### Federation mTLS and shared native-runner isolation

- Applied current explicit conf_center.controller_name to both valid and bad-cert
  mTLS fixture Controllers. Reused owned-process cleanup and occupied-port checks
  through examples/test/scripts/utils/owned_runtime.sh in both native runners.
  Both retain private artifacts and never call kill_all or remove run directories.
- Tightened identity metric assertions: missing/zero counters now fail, instead
  of warning then passing or substituting log evidence for metric evidence.
  Renamed misleading watch-sync progress text to registration and corrected the
  bad Controller identity description to ctrl-bad/east-cluster.
- Native final mTLS run 44165 terminal zero: 9 passed, zero failed. Covers valid
  mTLS registration, positive success counter, SPIFFE mismatch rejection with
  positive mismatch counter and no extra online Controller, and no-TLS startup
  refusal with the specific reason. /tmp/ws5-center-mtls-native-v3.log.
- Shared-helper lifecycle rerun 59308 terminal zero: 27 passed, zero failed;
  /tmp/ws5-center-federation-shared-runtime.log. Runs overlapped on distinct ports
  without cross-process cleanup. Recorded child PIDs were verified absent after
  completion. Initial mTLS run 97670 also passed; a subsequent attempt correctly
  exposed overly strict TIME_WAIT preflight, then terminated before startup.
- Final port guard permits server-style address reuse but probes loopback too:
  macOS allows wildcard/specific-address overlap even at listen time. Negative
  experiments verified both existing loopback and wildcard listeners are refused
  and remain reachable; all 18 released mTLS endpoints passed final preflight.
  These final helper-only checks followed the native runs. Shell syntax/diff
  checks passed. No production Rust or Edgion changes.
- Kubernetes mTLS/ownership and other remaining mode/provider/data-plane scenarios
  are still open. Overall alignment remains active.

### Real Kubernetes adapter persistence and Lease verification

- OrbStack context was verified available. Created only the previously absent
  EdgionController and EdgionProviderAccount CRDs and the isolated namespace
  center-alignment-20260928024712. Existing Edgion resources and deployment
  configuration were not changed. Used a private context-pinned kubeconfig.
- Opt-in real_cluster integration target completed in session 94856, exit zero:
  two passed, zero failed, zero skipped. Log:
  /tmp/ws5-center-kube-adapter.en6y2P/test.log.
- Verified provider account persistence across fresh adapters and stale-generation
  CAS rejection; Controller status resourceVersion advancement without changing
  spec generation, observedGeneration and directory reconstruction; real Lease
  expiry/takeover, increasing fencing epoch and stale-holder release rejection.
- Post-run API listing confirmed no Controller, provider account or Lease objects
  remain in the test namespace. Namespace, CRDs and private artifacts are retained.
- Updated integration instructions to pin context, install only absent required
  CRDs and use a unique namespace instead of applying the complete deployment to
  edgion-system. Documented provider account coverage and adapter-test boundaries.
  Diff whitespace and documented shell syntax checks passed.
- This does not prove deployed runtime RBAC, OIDC login, internal mTLS forwarding,
  browser Kubernetes capabilities or managed-cluster data-plane behavior. Those
  remain open; overall alignment remains active.

### Kubernetes browser preparation and image context isolation

- The read-only browser API preflight now passes on OrbStack: 23 fixture kinds
  across seven served API versions. This is discovery evidence, not CRD schema
  equivalence or browser execution. No browser runtime has been deployed yet.
- Found that build-image.sh copied the whole checkout (except four directories),
  including approximately 599 MB of retained web/test-results with generated
  credentials, TLS keys and browser sessions, into the image build context.
  Stopped the owned first build (session 28836, exit 130) during base-image
  metadata resolution. Its log does not show a completed context transfer.
- Restricted staging to Cargo manifests/build script, crates, bins and web;
  explicitly exclude web test results, Playwright reports, dependencies, dist
  and .env files. Keep E2E source helpers imported by frontend typechecking.
  Existing artifacts are preserved. Real-tree staging and synthetic positive/
  negative fixture checks passed, along with bash syntax and diff checks.
- Replacement image build is active in session 62359, log
  /tmp/ws5-center-kube-image-v2.log, local-only tag
  edgion-center-kubernetes:alignment-kube-20260928-v2. Last confirmed progressing
  through base-image resolution. Poll this handle before starting another build.
  Image completion and Kubernetes browser evidence remain unproven.

### Kubernetes schema drift and capability boundary preparation

- Read-only comparison found six existing OrbStack Edgion CRDs differ from the
  current source: EdgionGatewayConfig, EdgionPlugins, EdgionBackend,
  EdgionStreamPlugins, EdgionConfigData and LinkSys. Differences include the old
  WAF rules requirement, absent policyRef, old ConfigData enum and required
  gatewayClassName/gatewayClassUid in shared status. Existing CRDs were preserved.
- Extended browser preflight to compare all nine cataloged Edgion CRD schemas
  against the selected checkout, including descriptive metadata, ignoring object
  key order. Discovery remains the check for built-ins and Gateway API resources.
  Final real-cluster negative run 19124 exited one with exactly the six expected
  schema mismatches; /tmp/ws5-center-kube-schema-preflight.log. A matching live
  schema positive run remains pending an isolated current-schema cluster.
- Added a Kubernetes-only browser case for capability discovery, hidden SQL
  administration menus, direct-route redirects and denied user/role/audit APIs.
  E2E typecheck passed and Playwright lists the new case with its OIDC setup.
  Browser execution is pending; initial list attempt lacked harness environment
  variables and was repeated successfully with explicit run/Controller IDs.
- Center build 62359 remains live, now with a 5.77 MB transferred context.
  Edgion image build 91476 is also live, compiling current source; log
  /tmp/ws5-edgion-kube-image-v2.log. Both use local-only alignment-kube-20260928-v2
  tags and neither pushes. Poll both handles before launching replacements.
- Existing Edgion changes belong to other work and were preserved. No Edgion
  source edits or commits. Next: finish images and establish an isolated cluster
  with current CRDs rather than altering shared OrbStack contracts. Overall
  alignment remains active.

### Isolated kind runtime preparation

- Added a shared context guard: accept OrbStack or only the exact
  kind-eruie2e-<SHA-256 run prefix> context for the current E2E_RUN_ID. Seed,
  reset, preflight and cleanup all use it. Unit tests cover accepted contexts,
  unrelated/missing names and previous-run rejection (two passed, session 80751).
- The Kubernetes runner imports local images into that kind cluster and owns a
  loopback Dex port-forward. Chromium maps only the run's Dex hostname to loopback;
  issuer and certificate identities remain unchanged. Full login/forwarding
  behavior awaits deployment; no live OIDC success is claimed here.
- E2E typecheck and shell syntax passed. Kind-context Playwright discovery passed
  (67863). Frontend build 36579 and lint 76710 completed zero; existing bundle-size
  warning remains. Documented private-kubeconfig setup and retention behavior.
- Direct kind binary downloads timed out. Built kind v0.33.0 from the official
  Go module in a temporary owned container (1883 terminal zero), executable
  /tmp/ws5-center-tools/kind/kind. Existing kind cluster edgion-standard-162 is
  unrelated and preserved.
- Isolated creation session 35885 remains live for eruie2e-655cd051, using the
  cached arm64 kindest/node digest a1ed56cfb0e7b93589bdf97c8cd566405a265939e3620fc4f5de89adff580ae5.
  Private kubeconfig and create.log are under
  /tmp/ws5-center-kind-alignment-20260928-v2. Last inspection shows the owned
  control-plane container Created; cluster readiness is not established.
- Auxiliary cached-image kubeadm-version inspection session 40231 is also live
  (owned --rm container wonderful_shockley, ID c557cab82a69); poll it rather than
  repeating the command. Center image 62359 and Edgion image 91476 remain live;
  Center reached frontend build and Edgion is compiling current Rust sources.
  Continue polling these handles, install CRDs only in the new cluster once ready,
  then execute Kubernetes preflight and browser cases. Overall alignment is active.

### Isolated control-plane bootstrap diagnosis

- Kind creation 35885 terminated one during kubeadm admin bootstrap: the API
  server was not serving before its deadline. Retained the owned node and logs.
  Version inspection 40231 completed zero, reporting Kubernetes v1.37.0; its
  --rm container finished. These two handles are no longer live.
- Kubelet/containerd are active. Four static-pod sandboxes report Ready but no
  application containers exist yet; this is not control-plane readiness. Image
  content is present, while containerd reports unpacked=false and six active
  extraction snapshots (62 MB at last inspection). Do not confuse sandbox state
  with running API/controller processes or repeat cluster creation blindly.
- Exported the private kubeconfig to the existing run directory without changing
  the user's default kubeconfig. API readiness check 32073 terminated one (EOF).
  Attempted to resume only remaining kubeadm phases, preserving certificates,
  configuration and static manifests; 38708 terminated one with the same admin
  bootstrap deadline. Log: resume-init.log in the retained kind run directory.
  Inspect extraction/API progress before another bootstrap attempt.
- Real Chromium experiment 86110 completed zero: the exact Dex hostname mapping
  reached an ephemeral loopback server and preserved the original Host header.
  Browser/server were closed in finally. This validates resolver behavior only,
  not OIDC authentication or TLS against the deployed Dex instance.
- Image builds 62359 and 91476 remain live. Center completed its frontend build
  and is copying Rust/dashboard inputs; Edgion completed the Controller/CLI release
  build and moved to Gateway compilation. No user workload or existing cluster
  was modified. Overall alignment remains active.

### Container toolchain failure and snapshot recovery attempt

- Center build 62359 terminated one. Cargo explicitly rejects rustc 1.92.0 for
  the locked AWS SDK/Smithy packages, which require 1.94.1. Updated the Dockerfile
  default to Rust 1.96.1 (also used by the sibling build), and added --locked to
  preserve the checked-in dependency resolution. No dependency downgrade/update.
  Replacement build 43367 is live, same local-only v2 tag, log
  /tmp/ws5-center-kube-image-rust196.log. Image validation remains pending; keep
  the Dockerfile change uncommitted until the replacement gate establishes it.
- Containerd logs now prove repeated overlayfs snapshot commit timeouts, beyond
  simply slow initialization. Backed up the isolated node's original config to
  /kind/containerd-before-native.toml and switched its supported snapshotter to
  native; restart command 95390 completed zero. Only this owned node changed.
- Native extraction also reached the CRI deadline. Started a single API-server
  image prewarm with ctr images mount/unmount and a 600 s timeout, session 95256,
  log /tmp/ws5-center-kind-alignment-20260928-v2/prewarm-apiserver.log. Kubelet is
  stopped on the isolated node to prevent competing retries. On success the
  command unmounts and restarts kubelet. If it fails, explicitly restart kubelet
  after inspecting the log (set -e skips that final command). Do not launch a
  second prewarm while this handle is live. No validated cluster exists yet.
- Edgion build 91476 remains live in Gateway compilation. Source changes in that
  repository remain untouched. Diff whitespace check passed; overall alignment
  remains active, with no claim of Kubernetes browser completion.

### Native WAF mutation-boundary audit

- Current WafConfig is a top-level spec.waf logical plugin, separate from the
  four stage arrays. Its operator inputs are policyRef, activeProfile or
  activeProfileRef, mode, requestBody and priority. The existing stage-only
  mutation exclusions did not cover this structure.
- Added exact WAF exclusions for inspection-only rules, resolved policy/bundles,
  selected profile/reference indices, resolution diagnostics and owner namespace.
  Operator refs, request-body settings, zero priority, sibling stages and nested
  unknown operator fields remain intact. Create/update tests also verify CAS
  resourceVersion behavior. No Controller/backend contract change.
- Final mutation suite passed 20 tests and frontend build completed zero in
  session 51812. Lint 75733 passed. Initial build 57935 found type errors in the
  new test's unknown-document access; changed assertions to typed-safe property
  checks, then reran tests/build. Diff whitespace check passed.
- Remaining concrete WAF gaps: no top-level form/type, list totals omit WAF,
  topology omits policyRef and policy bundleRefs. Continue these against current
  source, then add native UI/API proof. The unrelated WAF diagnostic task under
  tasks/todo is preserved. This batch does not complete WAF alignment.
- Builds 43367 and 91476 and prewarm 95256 remain live. Native extraction grew
  from about 2.8 to 12 MB; no API readiness yet. Center build 43367 staged its
  source before this WAF boundary fix, so its future image cannot prove the fix;
  rebuild current sources before browser validation. Dockerfile change remains
  uncommitted pending its build gate. Overall alignment remains active.

### Logical WAF dashboard follow-through (2026-09-28)

- Added the operator WAF type and a separate `spec.waf` form section, keeping
  WAF outside the HTTP execution-stage catalog. Policy/Selector references,
  profile selection, mode, priority, and request-body inspection are editable;
  removing WAF preserves all stage arrays and unrelated spec fields. Read-only
  views disable controls. The Controller remains the semantic validator.
- List totals now count logical WAF once and show a WAF tag. Topology follows
  plugin policy/Selector references and WafPolicy profile bundle references,
  resolving omitted namespaces from each referencing resource. Scope guards
  prevent unrelated policyRef/bundleRefs payloads from creating false edges.
- Structural YAML checks reject scalar/array WAF objects and nested references
  or body configs before entering the form. Narrow edits preserve unknown fields.
- Focused unit/component/topology suite: 46 passed, session 78420 exit zero,
  `/tmp/ws5-center-waf-ui-tests-v2.log`. E2E typecheck and lint also passed in
  that sequential session; the initial lint found an unused test destructuring
  variable, which was removed. Final build 98687 exited zero, with the existing
  chunk-size advisory (`/tmp/ws5-center-waf-ui-build-v2.log`).
- Native standalone browser run `alignment-waf-ui-20260928-v1`, session 60411
  exit zero: authentication plus the logical-WAF case, 2 passed. It creates
  real WafRuleBundle/WafPolicy inputs, creates a WAF-only plugin through the
  browser, checks count/tag, round-trips Form/YAML, edits prefixSize, verifies
  the exact outgoing operator WAF object and Controller result, then deletes
  the exact test resources. Log: `/tmp/ws5-center-waf-ui-native-v1.log`.
  Seventy original fixture files remain unchanged; owned services were stopped
  by the runner. This proves dashboard CRUD, not Gateway WAF enforcement.
- Environment checkpoint: Edgion build 91476 exited one after compilation;
  runtime image packaging could not find `target/aarch64-unknown-linux-gnu/release`
  in the build context. No Edgion changes made. Center build 43367 remains live
  at OCI layer export after successful locked release compilation with Rust
  1.96.1; its staged source predates these frontend changes.
- API-server image prewarm 95256 exited 124 at its bounded timeout. Explicitly
  restarted kubelet in our `eruie2e-655cd051-control-plane` node and confirmed
  systemd reports active. No Kubernetes browser readiness claim. Preserve the
  private context and existing user clusters. The Dockerfile toolchain fix is
  still separate and uncommitted pending image completion. Overall goal active.

### Center image build toolchain gate completed (2026-09-28)

- Session 43367 exited zero. The canonical Kubernetes image script completed
  locked release compilation, OCI export and Docker import with Rust 1.96.1.
  Image `edgion-center-kubernetes:alignment-kube-20260928-v2` is linux/arm64;
  local image ID is
  `sha256:30c0cecba87368e5fab7ab712fbac4cd916f7d38b39b4fefbea2e9c8706d95bd`.
  Full log: `/tmp/ws5-center-kube-image-rust196.log`.
- Commit the Dockerfile's Rust default update and `cargo build --locked`
  separately from logical WAF commit `150ed3c`. No dependency/lockfile changes.
  This resolves the prior Rust 1.92 dependency MSRV rejection. Other target
  architectures and standalone image composition were not built in this run.
- This image's staged frontend predates the latest WAF work; rebuild current
  sources before using an image as proof for that UI. Kubernetes browser
  validation still requires a ready isolated cluster and Controller image.

### HTTP retry status-code boundary (2026-09-28)

- Current Edgion `http_route_preparse.rs::parse_retry` restricts response retry
  codes to 400..=599; the routing review guard for informational response state
  pollution confirms why 1xx must never trigger retries. Center still advertised
  and accepted 100..=599 in its shared policy editor, HTTP Zod rule schema and
  mutation validator. Aligned all three boundaries and both locale labels.
  gRPC remains 0..=16. No backend or Edgion source changes.
- Regression tests cover 1xx, 2xx, 3xx, out-of-range and fractional rejection,
  accepted boundary codes, lossless sibling preservation, form entry and the
  independent gRPC range. Session 49084 exited zero: 22 focused tests, frontend
  build and lint passed. Logs: `/tmp/ws5-center-http-retry-{tests,build,lint}.log`.
  E2E typecheck session 37013 also exited zero.
- Native run `alignment-http-retry-20260928-v1`, session 9034 exit zero:
  authentication plus focused HTTP retry browser test, 2 passed. Creates an
  isolated HTTPRoute, enters rejected 200 then supported 429 through the form,
  verifies Form/YAML output retains 503 plus 429, saves, checks the Controller
  resource and cleans the exact resource. Seventy original fixture files remain
  unchanged. Log: `/tmp/ws5-center-http-retry-native-v1.log`.
- Next concrete route gap: GRPCMethodMatchEditor writes an empty string when
  clearing optional service/method fields and prohibits removing the last match.
  Current `GRPCRouteMatch::compile_method` accepts an omitted method block for
  match-all and rejects invalid Exact names; service-only delegation also needs
  method.method omitted. Correct these editor transitions and verify them next.
- Isolated kind API readiness check still returned EOF at this turn's start.
  No live build/prewarm remains from the prior checkpoint. Center image version
  smoke command session 31484 exited zero with version 0.1.0 after image build.
  Overall resource/menu alignment remains active.

### gRPC optional method editing (2026-09-28)

- Checked current `GRPCRouteMatch::compile_method` and the routing knowledge
  page `grpc-method-matching-and-precedence.md`: omitted outer method means
  unconstrained; Exact service/method may be omitted individually, but explicit
  empty names are invalid. Center now deletes the field when its input is
  cleared and removes a type-only method object when both predicates are gone.
  Headers, sibling matches and unknown fields survive these narrow edits.
- Removed the last-match deletion restriction. New matches, new rules and empty
  resource defaults no longer materialize invalid empty service/method strings.
  Added accessible field/button labels and corrected optional-field documentation.
- Session 5031 exited zero: 12 component/editor tests, frontend build, lint and
  E2E typecheck passed. Logs `/tmp/ws5-center-grpc-match-tests-v2.log` and
  `/tmp/ws5-center-grpc-match-{build,lint,types}.log`. Initial test session 33742
  exposed an icon-prefixed accessible button name; explicit labels fixed it.
  Existing jsdom portal and Ant Design deprecation advisories remain.
- Native standalone `alignment-grpc-match-20260928-v1`, session 14273 exited
  zero: authentication plus focused browser workflow, 2 passed. Saved and read
  back service-only, headers-only and empty matches in three consecutive browser
  edits against a real Controller. Exact test resource removed; 70 original
  fixture files retained unchanged. Log `/tmp/ws5-center-grpc-match-native-v1.log`.
  This establishes editor/Controller behavior, not live gRPC forwarding.
- Route guide still contains unrelated stale statements (pending headings,
  TCP/UDP v1alpha2 examples, GRPC RequestMirror example, UDP stream-plugin claim).
  Reconcile those against source during the next route-menu pass. Overall goal
  remains active; no Kubernetes readiness or data-plane completion claim.

### Stream route menu annotation verification (2026-09-28)

- Checked StreamAnnotationsSection against current TCP/TLS route-unit builders
  and UDP route-unit/session admission wiring. Controls match the implementation:
  all three route kinds support StreamPlugins; TCP/TLS expose keepalive; only
  TLSRoute exposes upstream Proxy Protocol v2 and bounded connection retries.
  No changes to those production controls were needed.
- Added native per-menu annotation edits, preserving unrelated annotations and
  the complete route spec, round-tripping through YAML, then checking the real
  Controller resource. Existing stream-plugin fixtures are referenced; no new
  permissions or Gateway behavior were introduced.
- Added the gRPC retry-code runtime limitation to the shared rule-policy form
  in both locales. Current `grpc_route.rs::parse_retry` retains attempts/backoff
  but sets parsed codes to None; the prior UI implied status retries worked.
  The existing gRPC browser case now verifies the visible limitation.
- Updated the route guide from current source: TCP/UDP v1, UDP Stage-1 support
  and session boundary, TLS `v2`, no GRPC RequestMirror, route-level HTTP mirror
  tuning annotations, removed nonexistent extensionRefMaxDepth, and removed
  stale pending/completed development labels. This is an editor guide, not a
  claim to reproduce the complete authoritative schema.
- Typecheck/build/lint session 4551 exited zero. Logs:
  `/tmp/ws5-center-stream-annotations-{types,build,lint}.log`.
- Native v1 session 80431 exited one: authentication, TCP annotation edit, UDP
  annotation edit and gRPC workflow passed; TLS stopped on an ambiguous test
  locator matching both visible and accessibility option nodes. Restricted the
  locator to the visible option content. Focused TLS v2 session 66649 exited
  zero, authentication plus TLS annotation workflow, 2 passed. Logs:
  `/tmp/ws5-center-stream-annotations-native-v1.log` and `-v2.log`.
  Both runs retained all 70 original fixtures unchanged and stopped owned
  services. Exact mutation fixtures were removed. No live forwarding claim.
- Next route edit boundary to inspect: shared RulePoliciesEditor still stores
  empty strings when clearing optional timeout/backoff/session fields. Compare
  omission semantics and validation before changing them. Overall audit active.

### Optional route policy fields and current retry validation (2026-09-28)

- RulePoliciesEditor now removes cleared request/backend timeout, backoff,
  session name and absolute-timeout fields. It keeps explicit configured blocks
  and unknown/sibling values, preserving the current two-state delegation
  inheritance contract rather than silently switching an override to inherit.
- Current shared SessionPersistence has no idleTimeout field. Removed it from
  supported types/schema and fresh form inputs. Existing values remain visible
  as an unsupported-field warning with explicit removal; unrelated edits retain
  them and read-only views cannot remove them. Added the FileSystem/etcd-only
  scope of strict (standard Kubernetes schemas prune it). No CRD changes.
- Native verification exposed another current Controller boundary: HTTP retry
  attempts must be at least one (GRPC can retain zero). Updated the protocol-
  aware input, HTTP schema and mutation validator. HTTP duplicate retry codes
  are also rejected, matching HTTPRouteRetry::validation_error. All unrelated
  GatewayConfig retry semantics remain untouched.
- Final sequential session 89039 exited zero: 30 focused tests, frontend build,
  lint and E2E typecheck. Logs `/tmp/ws5-center-route-policy-clear-tests-v3.log`
  and `-build-v3.log`, `-lint-v3.log`, `-types-v3.log`. Earlier build 21793 failed
  on Testing Library's unsupported `exact` option in a new test; removed it.
  Intermediate reruns 98215 and 54480 passed before the added HTTP retry bounds.
- First native session 24297 exited one: auth and GRPC passed, HTTP creation
  was rejected with retry.attempts zero. The corrected protocol-specific
  fixture and frontend boundaries were rerun in session 1456, exit zero:
  authentication plus HTTP and GRPC clearing cases, 3 passed. Each case clears
  five fields, inspects YAML, saves and verifies the exact Controller sections.
  Standalone false strict remains; the Kubernetes branch does not expect a
  pruned strict field. Kubernetes execution is still unverified.
  Logs `/tmp/ws5-center-route-policy-clear-native-v1.log` and `-v2.log`.
  Seventy original fixtures retained unchanged; exact mutation fixtures removed
  and runner-owned services stopped. Overall audit remains active.

### Cloud DNS inventory failure handling and contract gates (2026-09-28)

- Cloudflare and Route53 pages did not render account/zone/record query errors;
  zone failures could display the normal empty-inventory message. Added a
  sanitized error alert that identifies stale/incomplete observations and
  suppresses the no-zones message on query failure. Refresh now invalidates
  the account query as well as DNS queries, so account-list failures recover.
  Permission-disabled reads remain disabled; no mutation retry was introduced.
- Six new component workflows cover both providers' account, zone and record
  failures, refresh recovery, absence of raw provider errors, and absence of
  mutation dispatch. Session 64155 exited zero: 22 cloud page tests, frontend
  build and lint. Logs `/tmp/ws5-center-cloud-read-errors-{tests,build,lint}.log`.
  These use deterministic API mocks; no native browser/provider claim for the
  new error states.
- Full frontend baseline session 92305 exited zero: 590 tests across 100 files,
  `/tmp/ws5-center-frontend-full-current.log`. It started before the cloud read
  changes; the subsequent targeted suite validates the six new workflows.
- Hermetic cloud contract session 18896 exited zero: Cloudflare adapter 71,
  Route53 adapter 45, AWS SDK HTTP transport fixtures 13, Cloudflare integration
  service 50, Route53 integration service 16, totaling 195 passed. One explicit
  real-account test stayed ignored; no cloud credentials or external mutations
  were used. `/tmp/ws5-center-cloud-contract-tests.log`. This exercises actual
  SDK/HTTP fixtures, one-shot writes, guards and ambiguous dispatch outcomes;
  optional real-account tests are not part of the required hermetic gate.
- Next confirmed cloud concern: both frontend mutation classifiers map an Axios
  transport failure with no HTTP response to rejected. Such a request may have
  been dispatched, so distinguish this from explicit rejection without treating
  local validation guards as dispatched. Verify this boundary next. Overall
  audit active; Kubernetes readiness and remaining menu workflows are open.

### Cloud browser transport ambiguity (2026-09-28)

- Replaced the two cloud mutation-error classifiers with one provider-neutral
  `api/cloudMutation.ts` implementation. Explicit unknown_outcome remains
  ambiguous, 409/412 remain conflicts, and an Axios POST/PUT/PATCH/DELETE with
  a transport request but no response is now ambiguous instead of rejected.
  Local pre-dispatch guards, setup errors without a request, and failed GETs
  do not become dispatched mutations. Existing definitive rejections remain.
- Both DNS pages consume the shared helper; removed the obsolete provider-
  specific classifier. No backend, credential, retry or dispatch changes.
  Updated the owning cloud security guide with the browser observation boundary.
- Added actual AxiosError-shaped tests for all write verbs and the non-dispatch
  cases, plus Cloudflare/Route53 component workflows preserving drafts and
  asserting one dispatch after network failure/timeout. Final session 47373
  exited zero: 30 tests, frontend build and lint passed. Logs:
  `/tmp/ws5-center-cloud-transport-tests-v2.log`, `-build.log`, `-lint.log`.
  Initial session 75880 failed because the new Route53 test queried the edit
  button before async inventory arrived; changed it to await the button and
  reran all three suites. Existing jsdom/Ant Design advisories remain.
- These are deterministic frontend transport/component checks, not real cloud
  mutations or a browser-to-provider acceptance claim. Prior 195 hermetic cloud
  backend tests remain applicable because no backend code changed. Overall
  resource/menu audit is active; no goal completion claim.

### Current coverage index and ExtensionRef runtime boundary (2026-09-28)

- Added CURRENT-COVERAGE.md as a compact current-state index: all 22 resource
  menu rows, Center/shared/cloud/admin menus, evidence levels, environment
  limitations and remaining audit actions. Compared all 22 menu paths directly
  with the current catalog (one draft GatewayConfig path was corrected).
  Inspected retained full-browser, federation, mTLS, frontend and cloud logs;
  historical green runs are explicitly not relabeled as final-tree verification.
- Current shared LocalObjectReference serializes Controller-owned
  resolvedNamespace for delegated rule provenance. HTTP and gRPC mutation
  catalogs omitted it. Added exact exclusions for rule-filter and backend-filter
  ExtensionRefs. Operator group/kind/name and unrelated fields with the same
  terminal name remain intact. No Edgion or transport schema change.
- Session 96414 exited zero: 43 resource-document/catalog tests, frontend build
  and lint. Logs `/tmp/ws5-center-ref-namespace-{tests,build,lint}.log`.
  Four new cases cover both route types and both create/update modes, including
  input immutability and preservation of unrelated resolvedNamespace fields.
  This is mutation-boundary evidence, not new delegated-route native traffic
  evidence. Overall goal remains active.


### Gateway listener TLS mutation boundary (2026-09-28)

- Compared the current GatewayTLSConfig in sibling Edgion with the Center
  mutation catalog. Replaced obsolete certificate-runtime field names with
  resolvedCertificateRefs and resolvedFrontendCaRefs, and excluded the current
  frontendMatcherEligibility. No Edgion files changed.
- Create/update coverage checks both form and YAML paths, multiple listeners,
  redacted vectors and structured resolution outcomes, resourceVersion behavior,
  and input immutability. Operator certificateRefs, frontendValidation, global
  TLS settings and unrelated same-name option keys remain intact.
- Corrected adapter guidance: schemars(skip) alone does not prove a field is
  Controller-owned. Gateway frontendValidation is an operator-input exception.
- Build passed in session 19710; its lint step found three unused test bindings.
  After correcting those bindings and matching the eligibility fixture to the
  current Rust enum, session 38661 exited zero: 66 tests and lint passed.
  Logs: /tmp/ws5-center-gateway-resolution-tests-v2.log,
  /tmp/ws5-center-gateway-resolution-build.log,
  /tmp/ws5-center-gateway-resolution-lint-v2.log.
  This verifies serialization boundaries, not new native TLS traffic coverage.
  The full resource/menu audit remains active.


### HTTP rule admission and provenance boundary (2026-09-28)

- Inspected current serialized runtime fields across top-level resource structs.
  EdgionTls, EdgionAcme, BackendTLSPolicy and the health-check/TLS portion of
  EdgionBackendTrafficPolicy already have corresponding mutation exclusions.
  This is a source boundary audit, not a claim of complete field/UI coverage.
- HTTPRouteRule now carries resolvedAiAdmission, resolvedTerminalRouteUid and
  resolvedTerminalRuleIdentity. Verified their Controller owners in the HTTP
  handler and route_delegation::set_terminal_rule_identity; added exact rule-level
  exclusions. Preserve operator rules, backend references, explicit empty values,
  unsupported-but-operator-owned useDefaultGateways, and unknown nested content.
- Added create/update cases with multiple rules, AI and Service references,
  same-name nested operator keys, CAS version behavior and input immutability.
  Session 67212 passed tests but caught an unknown metadata type in the new test
  at build time. Corrected the assertion; session 82242 exited zero with 64 tests,
  build and lint. Logs: /tmp/ws5-center-http-provenance-{tests,build,lint}-v2.log.
- No Edgion source changed. No new native AI/delegation traffic test is claimed.
  Recorded the next discovered condition-runtime omission in CURRENT-COVERAGE.md;
  the resource/menu audit remains active.


### Shared plugin condition resolution boundary (2026-09-28)

- Checked current SecretMatchCondition and IpAccessMatch in Edgion conditions.rs,
  their entry/body/dye owners, RequestRestriction and RequestAccessPolicy nested
  condition owners. resolvedValues is redacted Secret-derived state; resolvedIps
  is the expanded inline/reference union. Both must stay out of operator writes.
- Added both terminals to the existing stage-scoped recursive runtime exclusions,
  covering all four plugin stage trees and flattened RequestAccessPolicy IP rules.
  The separate RequestAccessUrlAllowList ConfigData adapter already strips these
  condition fields; no change was needed there.
- Create/update regression cases exercise allOf, anyOf and grouped allOf, multiple
  stages, entry/config/body/dye nesting, preservation of references and original
  lists, false/empty values, unknown content outside stage trees, and input
  immutability. These are structural serialization cases, not proof that every
  plugin or condition is accepted in every runtime stage.
- Session 41005 passed 82 tests, build and lint. Corrected test dye fixtures to
  use current name/on/value fields; session 93778 reran all 82 tests successfully.
  Logs: /tmp/ws5-center-plugin-resolved-tests-v2.log,
  /tmp/ws5-center-plugin-resolved-build.log,
  /tmp/ws5-center-plugin-resolved-lint.log.
  No Edgion source or deployed cluster changed. Overall audit remains active.


### RequestAccessPolicy edited body capability (2026-09-28)

- Found that the browser body gate preferred stale resolvedProfiles over current
  operator profiles. Adding an HMAC body condition to an existing policy could
  hide the body editor or reject serialization until Controller reconciliation,
  although the new operator document had not yet been submitted.
- The conservative browser gate now checks both sources and permits URL rules
  with unresolved configRefs: referenced candidates can consume a body, and the
  browser cannot resolve them. Controller resolution/validation remains the
  authority, so browser eligibility is not a claim of runtime acceptance.
- Create/update tests preserve the body block alongside new operator profiles,
  exclude old resolved profiles from the mutation, and check input immutability.
  Covered unresolved references, resolved body evidence, no body demand and
  request-stage restriction. A real form test edits maxBodySize with an empty
  old resolvedProfiles map and verifies sibling preservation.
- Session 79701 exited zero: 35 focused utility/component tests, build and lint.
  Logs: /tmp/ws5-center-policy-body-{tests,build,lint}-v2.log.
  Earlier session 23703 also exited zero before the unresolved-reference and
  additional form cases. No new native request-body traffic claim; no Edgion
  changes. Overall resource/menu audit remains active.


### Controller inventory read-state UI (2026-09-28)

- ControllersPage previously rendered an ordinary empty table on first read
  failure and silently retained cached online rows after failed refreshes.
  Inventory and enabled history query errors now produce a persistent localized
  stale/incomplete warning. The initial table failure no longer says No data.
- CenterDashboard now uses a sanitized description rather than raw error text.
  Without any snapshot, failed reads show unknown membership/online/cluster
  counts and suppress the resource overview instead of reporting an empty fleet.
  Cached snapshots remain available with the warning.
- Component cases cover initial inventory failure, cached list/history failure,
  refresh recovery to changed rows, suppression of private error strings, and
  unknown dashboard totals. Existing reload and capability tests still pass.
- Session 19885 exited zero: 12 page tests, build and lint on the final code.
  Logs: /tmp/ws5-center-controller-read-{tests,build,lint}-v3.log.
  Earlier focused runs 52957 and 93337 also completed successfully before the
  final no-snapshot overview guard. This is component evidence, not a new native
  federation outage run. No Edgion files changed; overall audit remains active.


### Current frontend suite and environment revalidation (2026-09-28)

- Session 89063 exited zero at Center commit 712ed0f: 101 files / 622 tests passed.
  Log: /tmp/ws5-center-frontend-full-20260928-current.log. This includes all recent
  committed frontend fixes, not just their focused test subsets. The working
  tree had only unrelated .claude/ and tasks/todo/ entries before this log update.
- Compared the Controller menu's RequirePermission(controllers:read) route guard
  with backend middleware mapping. The route protects page mounting; a missing
  per-query gate on that protected list page is not itself a permission bypass.
- Revalidated dedicated kind node eruie2e-655cd051-control-plane. API readiness
  returns EOF; crictl ps -a is empty; containerd reports reserved CreateContainer
  names for control-plane pods. Its main thread is blocked in fifo_open per
  /proc/862/stack and openat per /proc/862/syscall. Services are running, but this
  is not a healthy Kubernetes runtime. No restart or user-cluster mutation.
- Current UI suite success does not close the native regression, backend gates,
  OIDC/ownership/forwarding or remaining nested-field evidence. Goal stays active.


### Backend matrix and deterministic provider deadline regression (2026-09-28)

- Current native builds completed: Center session 2501 exited zero; Controller
  session 71323 exited zero after 8m04s. Logs:
  /tmp/ws5-center-current-native-build.log and
  /tmp/ws5-controller-current-native-build.log. Edgion's concurrent changes were
  preserved. Federation protobuf definitions match except comments/spacing.
- Initial backend matrix session 56252 failed one Cloudflare deadline test:
  the operation correctly returned Unavailable, but a 10ms real deadline expired
  during credential file IO before FakeApi dispatch (calls=0, expected=1).
  The regression now waits for provider dispatch before pausing Tokio time and
  advancing the operation deadline. It still requires exactly one provider call
  and an Unavailable result. Only test code and a Tokio dev feature changed.
- Focused session 21273 passed. Matrix rerun 5121 passed fmt, clippy, workspace
  tests, app no-default-feature tests (242), dependency purity and kustomize.
  Its 19 successful test-result groups report 1112 passes in total (includes
  repeated feature-mode coverage, not a unique test count) and 3 ignored tests;
  external Kubernetes/MySQL and federation stages were not opted in. Web was
  explicitly skipped because the fresh 622-test frontend run is recorded above.
- Matrix v2 exited 1 at the pre-existing English-only guard on the tracked root
  fix-issue-workflow-generic.zh.md. Preserved that unrelated file. Ran the
  subsequent no-legacy guard separately: passed. Diff check passed.
  Logs: /tmp/ws5-center-current-backend-matrix-v2.log,
  /tmp/ws5-center-current-legacy-guard.log, /tmp/ws5-center-deadline-test.log.
- Full current-native browser run alignment-full-current-20260928-v3 is separate;
  this checkpoint does not claim its completion. Overall audit remains active.


### Complete current-native browser regression (2026-09-28)

- Session 20509 exited zero: 155 passed / 2 skipped in 11.2m. Run ID
  alignment-full-current-20260928-v3; log /tmp/ws5-center-full-current-native-v3.log.
  Both skips are Kubernetes-only: capability behavior and restricted dependency
  namespace scope. No failed cases. The case ledger independently contains all
  112 expected annotated cases with passed status.
- Current native Center and Controller binaries were rebuilt before startup.
  RBAC was enabled; the run used two private Controller configurations, generated
  credentials, an isolated database and 70 run-owned fixture files. Coverage
  includes all 22 generic CRUD resources, resource/menu actions, permission and
  reload controls, RegionRoute writes/convergence, and the newer route policy,
  stream annotation, WAF, ConfigData and eight LinkSys variant cases.
- E2E typecheck session 57844 exited zero; log
  /tmp/ws5-center-current-e2e-types.log. The runner's inventory gate passed.
- Runtime trap completed; lsof confirms no listeners on this run's Center,
  Controller Admin or Vite ports (12201/13101/13201/15173). Cleanup verification
  retained all 70 fixture files, including two expected modified fixtures with
  their original deletion hashes preserved. Artifacts remain under
  web/test-results/alignment-full-current-20260928-v3/.
- Updated CURRENT-COVERAGE.md to replace obsolete full-regression pending notes
  with current frontend, backend and native evidence. This still does not claim
  deployed Kubernetes, external provider mutations or exhaustive data-plane
  conformance. Remaining audit actions stay explicit; the overall goal is active.


### Global inventory failure and expired-cursor recovery (2026-09-28)

- Reviewed aggregate inventory list/catalog queries and detail snapshot fallback
  against the canonical watch-cache read model. Existing page logic already
  reports query failures and resets the paginated inventory on explicit refresh;
  no production change was needed for the scenarios checked here.
- Added catalog/list failure-and-recovery cases that verify sanitized errors,
  restored rows and fresh requests. Added expired continuation-token coverage:
  a failed next-page read retains current rows with a warning; refresh requests
  page one without the old token, replaces old rows and removes Load more when
  the new snapshot has no continuation token.
- Initial test run 12551 failed because its exact accessible button-name matcher
  omitted Ant Design's reload icon label; corrected the matcher. Session 41920
  exited zero: 20 global inventory/consistency tests, build and lint passed.
  Logs: /tmp/ws5-center-global-recovery-tests-v2.log,
  /tmp/ws5-center-global-recovery-build.log,
  /tmp/ws5-center-global-recovery-lint.log.
- This is component recovery evidence in addition to the previously completed
  native inventory checks; no deployed multi-replica outage is claimed.
  No production code or Edgion files changed; the overall audit remains active.


### Completion audit: catalogs closed, runtime gaps remain (2026-09-28)

- Verified current ResourceKind/catalog equality (22), all 22 passed native CRUD
  ledger entries, and all 20 first-class catalog routes in menus and coverage.
  Verified exact 48 HTTP-plugin name equality; all nine ConfigData variants,
  eight LinkSys variants, four connection-stage and one TLSRoute-stage plugin
  variants are represented by the current forms/catalogs. These are direct
  source comparisons, not inferred from the test count.
- Re-read the original completion criteria and remaining runtime scenarios.
  OIDC signature/claim validation, SAR identity mapping, fencing and owner
  forwarding tests passed in the current backend matrix, but are not actual
  browser OIDC or deployed replica evidence. Kept those limits explicit and
  identified the existing OAuth browser setup as the next authentication path.
- Final frontend suite session 81433 exited zero: 101 files, 625 tests, 49.90s.
  Log: /tmp/ws5-center-frontend-final-20260928.log. The only changes since the
  complete native run were test/evidence files; no production regression is
  implied by this count increase.
- Updated CURRENT-COVERAGE.md with these findings. This is not a completion claim:
  source/menu completeness does not prove all original runtime flow criteria.

### Real OIDC browser login and logout finding (2026-09-28)

- Ran native Center with isolated SQLite and OIDC-only authentication, current
  Vite dashboard, and the Kubernetes fixture's pinned Dex/oauth2-proxy versions.
  Discovery/JWKS TLS verification stayed enabled; Chromium used only the test
  certificate SPKI exception. No existing cluster or user environment changed.
- Browser session 61015 passed six redirect/callback, identity, deep-link,
  refresh and missing-session checks. Session 27987 separately reproduced an
  OIDC logout defect: the local endpoint returns 404, the client ignores it,
  and the proxy cookie still grants API access after refresh.
- Full setup, artifacts, current owned runtime handles and repair criteria are
  in [OIDC-BROWSER-EVIDENCE.md](OIDC-BROWSER-EVIDENCE.md). The validated runtime is
  retained for the logout repair. No production code changed in this evidence
  batch; Kubernetes and remaining resource runtime gaps stay open.

### Provider-aware logout and mixed-authentication login (2026-09-28)

- Repaired OIDC logout through an operator-configured same-origin proxy path;
  authenticated identity selects the logout flow. Local cookie logout failures
  now propagate to visible, sanitized feedback. Mixed authentication clears a
  coexisting password cookie before external logout. Login bootstrap accepts
  existing sessions before presenting password fields.
- Real browser checks verify deletion of the proxy session and 401 after logout,
  including direct Center denial after clearing the coexisting password cookie.
  Final repository auth/shell cases passed for password and OIDC entry points.
  See [OIDC-BROWSER-EVIDENCE.md](OIDC-BROWSER-EVIDENCE.md) for artifacts and scope.
- Current frontend full suite: 102 files, 635 tests; build/lint and E2E checks
  passed. Backend workspace/default/no-default tests and matrix build gates
  passed; the unrelated English-only guard baseline remains explicitly open.
- No Edgion code changed. Dedicated native authentication runtime is retained;
  deployed Kubernetes and original cross-resource runtime gaps remain open.

### Kubernetes logout deployment wiring and runtime preparation (2026-09-28)

- Audited the deployable configurations after the authentication repair. Added
  the bundled proxy's `auth.logout_path` to the canonical Kubernetes ConfigMap
  and both cloud example overlays; these replace the embedded config string and
  therefore need the field independently. Updated the deployment instructions.
- All three Kustomizations render with `/oauth2/sign_out`. Kubernetes config
  tests passed (8, session 37540). A current native Kubernetes binary built
  successfully (session 68090, `/tmp/ws5-center-kube-native-build.log`).
- Revalidated the owned kind node and restarted only its containerd service.
  The bounded command returned 124 while systemd was still stopping it; later
  inspection confirmed a new containerd PID 1651, with containerd and kubelet
  active. Snapshot unpacking resumed, but the final CRI list still had no
  control-plane containers and the API returned EOF. This is not cluster-ready
  evidence, and no unrelated cluster/runtime was restarted.
- Started the current Linux image build through `cicd/build-image.sh`, tag
  `edgion-center-kubernetes:alignment-auth-20260928-v3`, session 69460. Log:
  `/tmp/ws5-center-kube-auth-image-v3.log`. It was still loading build context
  at the checkpoint; re-poll that handle rather than launching another build.
- OrbStack remains reachable. Its existing CenterController/ProviderAccount
  CRDs were observed, while ProviderCapabilitySnapshot is absent. The next
  practical deployment scope is Center plus Dex/proxy in an owned namespace,
  validating real SAR/capability behavior without altering shared Gateway CRDs.
  No resources were installed in OrbStack during this batch. The full goal
  remains active; native/browser auth proof is not deployed Kubernetes proof.

### Deployed Kubernetes authentication and disabled route repair (2026-09-28)

- Completed image v3, then deployed two real Center replicas and pinned canonical
  OAuth sidecars with Dex in OrbStack namespace `eruie2e-c0c61d3a-system`.
  Existing Controller/ProviderAccount CRDs matched the current schemas; only the
  absent ProviderCapabilitySnapshot CRD was created. Shared Gateway CRDs and
  existing workloads were not changed.
- Nine live ServiceAccount/viewer RBAC checks passed. Real OIDC browser access
  reproduced a blank page on capability-disabled `/users`, `/roles` and `/audit`.
  Their APIs already rejected access. Added a Center unmatched-route redirect
  to the authenticated home page; permissions remain enforced there and by APIs.
- Built image v4, rolled both replicas successfully, and passed five custom
  browser scenarios plus all three selected repository authorization cases.
  The denied-user screenshot shows explicit missing fleet access; proxy logout
  deletes the cookie and `/oauth2/auth` returns 401.
- Frontend full suite: 102 files, 635 tests passed. Fixed a test readiness race
  by waiting for the refresh button to leave its loading state before clicking.
  Build/lint and E2E types passed. No backend source changed in this batch;
  the prior backend matrix English-only baseline limitation remains open.
- See [KUBERNETES-AUTH-EVIDENCE.md](KUBERNETES-AUTH-EVIDENCE.md) for retained
  runtime, artifact locations, image identity and explicit coverage limits.
  No push; the overall alignment goal remains active.

### Real replica forwarding and ownership release repair (2026-09-28)

- Connected a native Controller to one of the two deployed Kubernetes Center
  Pods and proved owner/non-owner reads, actual CAS mutation, stale-version 409
  and default Controller RBAC across the dedicated internal mTLS hop.
- Migrated the connection to the other Pod, observed the Lease holder/epoch
  transition and verified the original owner now forwards remotely. Direct
  gRPC checks enforce the holder, fence and one-hop boundary.
- Reproduced an old-owner lifecycle defect: normal cancellation released its
  Lease without invalidating the cached ownership flag, returning ambiguous
  Unavailable for an old fence. Added invalidation before release and a
  regression assertion that failed before the fix. No stale write executed.
- Built and rolled out v5; all 22 proxy/fencing checks passed, including the
  previous valid fence returning FailedPrecondition on the old owner. Removed
  temporary proxy grants and verified restored permissions on both replicas.
  All three selected Kubernetes browser cases passed again.
- Backend matrix: 873 workspace tests and 244 no-default-feature app tests
  passed; format, Clippy, dependency purity and manifests passed. Its final
  exit remains the unrelated tracked English-only guard violation. Separate
  cargo check and no-legacy checks passed. Source/API/wire contracts in Edgion
  were not modified; unrelated working-tree changes were preserved.
- Detailed evidence, the bounded migration observation timeout and retained
  runtime pointers: [KUBERNETES-FORWARDING-EVIDENCE.md](KUBERNETES-FORWARDING-EVIDENCE.md).
  The overall goal remains active; no push.

### Provider account menu pagination and Kubernetes workflows (2026-09-28)

- Fixed the shared account client dropping continuation tokens after the API's
  first 50 rows, affecting both the account table and DNS account selectors.
  Later-page failures reject the read and repeated cursors terminate safely.
- Added explicit account-read and capability-read failures with cached-data
  warnings. Loading/failure no longer masquerades as a successful missing
  capability snapshot. Added seven meaningful API/component regression cases.
- Ran the existing CRUD/CAS browser case against the real Kubernetes store:
  creation, labels, concurrent 412, preserved draft and fresh edit all passed.
  Staged SAR grants proved that account-write alone cannot use credentials.
- Seeded 52 metadata-only CRDs and verified the last account on table page 3.
  Actual permission revocation/restoration proved visible failure and recovery
  for list and capability reads. Credential-value injection returned 400 and
  left no record. All temporary provider grants were removed afterwards.
- Updated Kubernetes E2E permissions/preflight so the common CRUD/CAS case no
  longer needs a standalone-only skip. No Secret permission or provider network
  calls were added. The updated preflight passed discovery and then rejected
  the same six stale shared Edgion CRD schemas; none were overwritten. This
  remains distinct from the passing isolated Center-only runtime proof.
- Current frontend gates: 642 tests, build/lint, E2E types/inventory and shell
  syntax. Current Vite source was tested; the retained v5 image was not rebuilt.
  See [PROVIDER-ACCOUNT-KUBERNETES-EVIDENCE.md](PROVIDER-ACCOUNT-KUBERNETES-EVIDENCE.md).
  No Edgion changes; goal remains active, no push.


### 2026-09-28: Gateway listener TLS mode alignment

- Current Controller `listener_has_unsupported_tls_mode` rejects TLS/Terminate
  and HTTPS/Passthrough for listener support; Center previously guarded only
  the HTTPS combination. Added the missing TLS guard to the shared Form/YAML
  validator and disabled unsupported choices in the listener dropdown.
- An omitted TLS mode no longer appears as an implicitly selected Terminate.
  Existing invalid documents remain intact for explicit repair; the editor
  does not rewrite certificates, options, or the protocol automatically.
- Five added regression cases cover validator immutability, both disabled
  protocol/mode choices, and Form/YAML submission followed by passthrough repair.
  Full frontend suite: 647 tests in 102 files passed. Build (TypeScript + Vite),
  lint and diff whitespace checks passed. Existing Vite chunk-size warnings
  remain. Logs: `/tmp/ws5-gateway-tls-{focused,full,build,lint}.log`.
- TLSRoute source-version registry still explicitly registers v1alpha3, so
  Center's alternate-version allowance remains. The older upstream feature
  document's canonical-only statement is not current source authority.
- Evidence is source, unit and component validation; this pass adds no live
  data-plane TLS conformance proof. No backend/Edgion source changes, no image
  rebuild, and no push. The full alignment goal remains active.


### 2026-09-28: Current route hostname mutation contract

- Added one shared hostname-list validator at the HTTPRoute, GRPCRoute and
  TLSRoute mutation adapters. TLSRoute requires explicit SNI names (1..1024);
  HTTPRoute/GRPCRoute allow omitted/empty lists with at most 16 entries.
  Current resource `hostname.rs`, algorithm `radix_hostname/pattern.rs`, and
  both vendored v1.6.2 CRD channels agree on these bounds.
- Invalid DNS patterns, IP literals, trailing newline/CR and oversized names
  now fail submission. Single-label names, 253-character labels, wildcard
  numeric suffixes and numeric DNS labels outside strict IPv4 syntax remain
  accepted. No input normalization or operator-field projection was added.
- New tests cover both mutation modes, all three route adapters, immutability,
  list bounds, non-array input, and TLSRoute Form/YAML block-and-repair flows.
  Two existing mutation fixtures had invalid empty TLSRoute hostnames; they
  now contain valid names. The first run's two failures were those fixtures;
  the final suite passes 676 tests in 103 files.
- Build (TypeScript + Vite), lint and diff checks passed. Logs are
  `/tmp/ws5-route-hostnames-final.log`,
  `/tmp/ws5-route-hostnames-build-final.log`, and
  `/tmp/ws5-route-hostnames-lint-final.log`. Existing chunk-size warnings remain.
- Upstream `skills/04-review/routing/regression-guards/tls-hostnames-per-attachment.md`
  still describes a v1.5/16-hostname TLSRoute contract. This is a documentation
  discrepancy against current source and the v1.6.2 bundles; it was not used
  to reintroduce a stale limit. No Edgion files were changed in this pass.
- This is source/adapter/component evidence, not new live TLS forwarding proof.
  Backend/runtime coverage remains as recorded above; the full goal stays active.


### 2026-09-28: Restricted dependency read failure and recovery

- Secret/ConfigMap metadata read failures now have a persistent sanitized error
  with a stale-cache warning. Refresh shows its pending state. The query uses
  the silent API option so raw transport errors do not also reach a global toast.
  Cached rows are hidden while Controller list-keys authorization is unavailable.
- Added component coverage for both kinds' read failure/recovery, retention of
  stale rows while authorization remains valid, and hiding previously visible
  metadata when Controller access is revoked. No value-read request was added.
- Real browser proof used current Vite source on 15174, deployed Kubernetes
  Center v5, Dex authentication, real Kubernetes SAR, and the retained FS
  Controller over the federation tunnel. A dedicated exact-path, GET-only
  temporary grant exposed Controller access and ConfigMap keys. The browser
  loaded metadata, never a ConfigMap value; Secret remained default-denied.
  Removing the list grant produced the persistent error with cached metadata;
  restoring it recovered. Final proof also asserts no raw error toast.
- Artifacts: `/tmp/ws5-center-dependencies-20260928/` contains `proof.cjs`,
  `proof-final.log`, `result.json`, inspected `read-denied.png`, and
  `restored.json`. All four live checks passed. The temporary ClusterRole and
  binding were removed; a subsequent proxy read returned 403 as before.
  The labeled, synthetic ConfigMap `default/dependency-metadata-proof` remains
  on the isolated FS Controller for review. No shared cluster resource changed.
- This proves Kubernetes Center authorization and browser error states, not
  Kubernetes Controller watch-namespace filtering. The existing namespace
  isolation E2E remains outstanding; do not count this narrower proof as that
  gate. No Edgion source change or image rebuild was made.
- Final frontend suite: 679 tests in 103 files passed; TypeScript/Vite build,
  lint and diff checks passed. Logs: `/tmp/ws5-dependencies-full-final.log`,
  `/tmp/ws5-dependencies-build-final.log`, `/tmp/ws5-dependencies-lint-final.log`.
  Existing Vite chunk-size warnings remain. Goal active; no push.


### 2026-09-28: Direct Kubernetes Controller namespace boundary

- Built the current Edgion Controller (`cargo build -p edgion-controller --bin
  edgion-controller`, exit zero) and ran an isolated native Kubernetes-mode
  Controller on loopback Admin 15912, probe 15932, metrics 15942 and conf-sync
  50964. This focused namespace probe intentionally uses a separate identity,
  ports and ServiceAccount from the retained full resource/federation runtime;
  no Gateway or traffic topology was started. Federation is disabled here.
- Created only run-owned namespaces `ws5-center-ns-20260928-{a,b,outside}`,
  labeled synthetic Secret/ConfigMap fixtures, scoped roles and ServiceAccount.
  The account can read both kinds in all three namespaces, but Controller
  `watch_namespaces` includes only a and b. A dedicated pause Pod supplies the
  Pod metadata identity required by native Kubernetes startup; get/patch is
  limited to that exact Pod. No existing CRDs were modified.
- Startup evidence required two fixture corrections: POD_NAME is mandatory,
  and startup must patch its Pod identity. The current Controller also starts
  EndpointSlice and ACME background reads despite this reduced no-watch set;
  their read-only permissions were added only in a and b. These are not proof
  of full custom-resource CRD compatibility. The first process wrote startup
  diagnostics into the inherited prior runtime log; the final process uses
  its own `logs/namespace-proof.log`.
- Six live assertions passed: for Secret and ConfigMap, the same ServiceAccount
  can read the outside fixture, metadata lists include both watched namespaces
  and exclude all others without returning values, and explicitly requesting
  the outside namespace returns 403. The initial ConfigMap count assertion
  failed because Kubernetes also creates `kube-root-ca.crt`; the corrected
  oracle requires both labeled fixtures and verifies every returned namespace.
  No Controller restart was used to resolve that assertion failure.
- Artifacts and private runtime configuration are under
  `/tmp/ws5-center-namespace-20260928/`; `result.json` and `proof-v2.log` are
  passing evidence, `result-initial.json` preserves the earlier failed oracle.
  The runtime remains live (session 55730 at this checkpoint); revalidate before
  reuse. ServiceAccount token lifetime is eight hours; no token is committed.
- Strengthened the repository's Kubernetes namespace authorization E2E to test
  both dependency kinds, absence of value/spec/status fields, and explicit
  out-of-scope denial for both Controller slots. E2E TypeScript and inventory
  checks passed (22 kinds, 224 cases). The strengthened two-Controller proxy
  case was not run against this direct-only probe and remains outstanding.
  The shared cluster's six CRD schema differences remain unmodified.
- No Edgion source edits or commits. This is new direct Kubernetes namespace
  evidence, complementary to the previous live Center browser/SAR evidence;
  neither is being reported as the full two-Controller E2E gate. Goal active.
- Final Center lint and diff checks passed. No production code changed in this
  pass; the frontend unit baseline remains the preceding 679-test run.


### 2026-09-28: Dual-Controller namespace proxy and browser proof

- Extended the isolated metadata-only probe with two native Kubernetes-mode
  Controllers, each using a distinct CA-signed mTLS identity and Center
  registration: `e2e-a/namespace-scope-a` and `e2e-b/namespace-scope-b`.
  They use Admin ports 15914/15915, probes 15934/15935, metrics 15944/15945,
  conf-sync 50966/50967, and independent Leases/Pod metadata anchors.
  Both registered Online on the retained Kubernetes Center v5 owner replica.
  This is an explicit isolated namespace probe, not another Gateway traffic
  topology; the original federation runtime and direct namespace probe remain.
- Created labeled `eruie2e-5c376d07-{a,b,denied}` namespaces for run
  `alignment-namespace-proxy-20260928`. Controller federation RBAC enumerates
  Secret/ConfigMap `list-keys` only, EdgionConfigData get/list/watch, and
  NonResource server-info. No wildcard grants or dependency value-read/write
  permission were added. Temporary Center SAR grants were GET-only and limited
  to these two identities' access, server-info and dependency-key paths.
- Executed the actual strengthened repository test from commit 69f6240:
  `authorization.spec.ts` / `restricted dependency metadata stays inside
  configured namespaces`, project kubernetes, against Vite 15174 and real
  deployed Center. Existing verified Dex storage state was reused with
  `--no-deps`. Result: one test passed in 22 seconds, covering both controllers,
  both kinds, fixture presence, all returned namespaces, absence of content/
  spec/status fields, and explicit `/default` namespace requests returning 403.
- Six additional real browser checks passed: for each Controller, the access
  document grants exactly `list-keys` for both dependency kinds; both menus
  show the three allowed namespaces, never request dependency values, and
  disable create/replace. Screenshots and request-path evidence were retained.
  Initial browser probes needed a longer bounded startup wait and a fixture-row
  locator that distinguishes Kubernetes `kube-root-ca.crt` rows; these were
  probe corrections, not application changes. Adding the exact server-info
  SAR URL removed unrelated header-fetch permission errors.
- Removed the temporary ClusterRole and binding. Four API reads (two kinds on
  two Controllers) returned 403 afterwards, and a fresh browser page showed
  access denied with no dependency rows. Five restoration checks passed.
- Artifacts: `/tmp/ws5-center-namespace-proxy-20260928/` contains private configs,
  setup/run scripts, `repo-browser.log`, `repo-browser/` Playwright artifacts,
  `browser-result.json`, inspected `namespace-metadata.png`, and `restored.json`.
  Controller process handles at this checkpoint: A 57312, B 30098; revalidate
  before reuse. Keys/tokens are private and not committed. Fixtures are retained.
- This closes the previously skipped dual-Controller dependency namespace gate.
  The reduced Controller profiles still deny unrelated DNS background Service
  reads; no general Controller readiness, gateway traffic, or custom-resource
  CRD compatibility claim follows. Shared CRD schemas and the retained Center
  image were not changed. No production source changed in this pass; the last
  full frontend baseline remains 679 passing tests. Goal remains active.


### 2026-09-28: Center health policy edits drive real traffic

- Fresh native Gateway build and the retained Controller now have a synthetic
  Service/EndpointSlice/HTTPRoute health-check topology. Center form edits to
  the policy probe path produced real Gateway 200 → 503 → 200 transitions,
  with Controller resourceVersion readback and backend request counters.
- The initial default loopback denial was resolved only for the test Gateway
  through its existing operator setting; the final probe sends the proper Host
  authority. No production code repair was necessary for this HTTP flow.
- Controller's exact original policy configuration and Center permissions were
  restored; three restoration checks passed and traffic remains healthy.
- See [HEALTH-POLICY-TRAFFIC-EVIDENCE.md](HEALTH-POLICY-TRAFFIC-EVIDENCE.md) for
  exact topology, artifacts, failed setup probes, retained handles and limits.
  This is HTTP active-health evidence, not encrypted-probe or resilience-wide
  proof. No Edgion edits or commits; overall goal remains active, no push.


### 2026-09-28: Center HTTPS health probe identity and recovery

- The actual policy form now has live HTTPS evidence: correct hostname gives
  200, incorrect certificate identity gives 503, and restoring it gives 200.
  Each edit advances Controller resourceVersion. Healthy assertions require
  multiple actual HTTPS requests before accepting the business traffic result.
- Encrypted probes intentionally reject loopback in current source. The first
  loopback attempt failed and is excluded; the passing fixture uses the host's
  private interface. Business HTTP and probe HTTPS remain independent.
- All three authorization-restoration checks passed. No production source
  changed, no Edgion changes committed, and no push. See the HTTPS follow-up in
  [HEALTH-POLICY-TRAFFIC-EVIDENCE.md](HEALTH-POLICY-TRAFFIC-EVIDENCE.md).
  mTLS and GRPC/GRPCS remain open; the overall goal remains active.


### 2026-09-28: Center mTLS and gRPC health probes

- Four HTTPS mTLS and ten gRPC/GRPCS form scenarios passed through deployed
  Center, retained Controller and actual Gateway traffic. Absent/untrusted
  client identity, incorrect server hostname and NOT_SERVING responses remove
  the backend; corrected configurations recover it. mTLS servers confirm the
  actual trusted client CN, and healthy checks require fresh probe requests.
- The pre-run HTTPS policy and exact Controller configuration were restored;
  all three permission/traffic restoration assertions passed. No production
  code changes or Edgion commits; no push. The overall goal remains active.
- See [HEALTH-POLICY-TRAFFIC-EVIDENCE.md](HEALTH-POLICY-TRAFFIC-EVIDENCE.md) for
  the 14-case matrix, synthetic fixture limits, artifacts and retained processes.
  TCP-only probes and other resilience behavior remain open.


### 2026-09-28: TCP probe flow and current verification index

- Three TCP form saves passed exact Controller readback and actual Gateway
  200/503/200 transitions. Fresh connection counters exclude stale health state.
  All five basic probe types now have recorded live Center integration evidence.
- Restored the pre-run HTTPS policy and exact Controller configuration; all
  three grouped authorization/traffic restoration assertions passed.
- Rechecked retained frontend logs and corrected CURRENT-COVERAGE.md's outdated
  latest-suite section to the actual 679-test baseline. No source changes or
  fresh full-suite claim. Probe details remain in
  [HEALTH-POLICY-TRAFFIC-EVIDENCE.md](HEALTH-POLICY-TRAFFIC-EVIDENCE.md).
- Next focused menu flow is RegionRoute failover with actual routed traffic,
  retaining the current overlay/CAS/watch contract. The broader audit remains
  active. No Edgion changes committed and no push.


### 2026-09-28: RegionRoute restoration defect found through traffic

- A first east/west/east browser result was misleading: Gateway logs revealed
  invalid-overlay fallback on clear. With different base and overlay targets,
  old Center reports `converged` but traffic goes west instead of east.
- Clearing now removes `failoverTo`; an existing empty string is no longer an
  idempotent success. The shared ConfigData writer also strips top-level status
  and `spec.currentStatus` instead of persisting Controller-derived fields.
  CAS versions and nested user data stay intact.
- Three new Rust regressions pass. The strengthened native browser run repairs
  the existing bad document, then proves east/west/east through the actual
  RegionRoute menu with no invalid-overlay fallback. Default Controller RBAC
  still rejects plugin writes, and the restored overlay is valid.
- Added persistent two-Controller E2E assertions for absent failover and status
  fields; its type/inventory checks and lint pass. The new live traffic run is
  single-Controller, not a rerun of that entire repository E2E scenario.
- See [REGION-ROUTE-TRAFFIC-EVIDENCE.md](REGION-ROUTE-TRAFFIC-EVIDENCE.md) for
  topology, initial weak oracle, before/after evidence and retained runtimes.
  The native Center was rebuilt; deployed Kubernetes v5 is not updated by this
  run. No Edgion production source changes. Overall goal remains active.

- Final gates: 876 workspace plus 247 no-default-feature app tests passed,
  alongside fmt, Clippy, dependency isolation and manifest rendering. Backend
  matrix exits at the unchanged Chinese workflow document's English-only
  violation. No-legacy and diff checks pass separately. Web dependency install
  and full unit/build stages were skipped because frontend component code is
  unchanged; focused E2E type/inventory/lint and real browser traffic passed.


### 2026-09-28: Two-Controller RegionRoute traffic and partial recovery

- Added an independent second Controller/Gateway to the owned native topology.
  Both menu failover and clear report two converged outcomes, advance each CAS
  version and change actual traffic east/west/east on both Gateways. Both base
  plugin documents stay identical; the overlay validity oracle remains distinct.
- Removing only B's ConfigData update permission produces a real HTTP 207 mixed
  result. The editor retains each outcome, disables inconsistent application,
  and actual traffic splits west/east. After exact policy restoration, menu
  source synchronization makes both Gateways reach west; final clear restores
  east/east. No invalid-overlay fallback occurred.
- Five fan-out checks and three partial/restoration checkpoints pass. The first
  partial attempt's incorrect 200 expectation was fixed to canonical 207; its
  finally restoration also passed. No production source change in this follow-up.
- Evidence and retained handles are in
  [REGION-ROUTE-TRAFFIC-EVIDENCE.md](REGION-ROUTE-TRAFFIC-EVIDENCE.md). The native
  backend includes the repair; Kubernetes v5 still predates it. Overall goal
  remains active, no Edgion changes or push.


### 2026-09-28: ReferenceGrant traffic and native status-source gap

- Cross-namespace HTTPRoute references now have live grant-form evidence:
  correcting a named Service target permits traffic, changing the source
  namespace denies it, and restoring it permits traffic again. Processed
  ResolvedRefs conditions and 500/200 responses agree. Five checks pass.
- Exact Controller config was restored; temporary ReferenceGrant update
  permission is absent and the owned valid route remains healthy.
- Actual route-list inspection found a gap: raw FS resource lists lack status,
  while the Controller's processed response has conditions. The UI shows a dash.
  Recorded [STATUS-SOURCE-GAP.md](STATUS-SOURCE-GAP.md) as the next repair, with
  source/observation separation, bounded reads and permission/freshness guards.
- See [REFERENCE-GRANT-TRAFFIC-EVIDENCE.md](REFERENCE-GRANT-TRAFFIC-EVIDENCE.md).
  No production source change or fresh test-matrix claim in this pass. Overall
  task remains active; no Edgion edits or push.


### 2026-09-28: Native list status and Controller-bound source reads

- Repaired 17 existing status columns with a shared display-only processed-status
  fallback. Preserve source multi-writer status and require exact resource identity
  and version. Bound concurrent reads to four; refresh on source refresh and poll
  active observations every 15 seconds. Failed reads hide stale healthy badges.
- Bound source-list requests and cache keys to the captured Controller, including
  callers without explicit scope. Processed specs never enter editable data.
- Full frontend suite passed 690 tests/104 files; the subsequent Controller-switch
  test passed separately. Build, lint, E2E types and inventory passed. Six native
  browser checkpoints cover real grant denial/recovery and injected status-read
  failure/recovery. Fixture restoration passed.
- [STATUS-SOURCE-GAP.md](STATUS-SOURCE-GAP.md) is resolved with evidence and limits.
  Kubernetes v5 predates this change. Broader alignment remains active; no Edgion
  changes or push.


### 2026-09-28: ACME trigger authorization and delivery outcome

- Replaced the raw global-proxy trigger with a captured Controller request and
  the existing dedicated operation permission. Pending dispatch prevents repeated
  clicks. Success means queue admission; ambiguous delivery is reported without
  automatic replay. Upstream error details stay out of messages.
- Sixteen focused API/component tests and four native browser/API checkpoints
  pass. Real default denial, simulated stale UI access with actual server 403,
  and native service-unavailable 503 are distinguished. No policy or CA changes.
- Full frontend suite passes 701 tests in 106 files; build, lint and inventory
  pass. Logs are indexed in CURRENT-COVERAGE.md.
  [ACME-MENU-EVIDENCE.md](ACME-MENU-EVIDENCE.md) records remaining lifecycle/expiry
  display work and the separate Kubernetes issuance evidence boundary. Overall
  alignment remains active; no Edgion edits or push.


### 2026-09-28: ACME lifecycle and certificate expiry

- Lifecycle and Conditions now share the same source-priority, version-matched
  runtime observation. Certificate expiry is shown in UTC; missing/invalid values
  stay absent and failed reads hide cached lifecycle/expiry. Horizontal scrolling
  preserves readable dates and actions at narrow viewport widths.
- Full frontend suite passes 706 tests in 107 files. Five new lifecycle tests
  and the existing ten status tests also passed focused checks. Final layout
  checks, build and lint pass. Four browser checkpoints distinguish actual native
  Pending from injected Ready/expiry and denial responses; restoration passes.
- See [ACME-MENU-EVIDENCE.md](ACME-MENU-EVIDENCE.md). Real Kubernetes issuance,
  renewal, Secret publication and Gateway hot reload remain separate work. No
  Edgion changes, resource/policy mutations or push.


### 2026-09-28: Real ACME issuance through Center and upstream clearing gap

- Brought up an isolated Kubernetes Controller leader, native Gateway and Pebble
  with actual HTTP-01 validation and container-local CA trust. Center-created ACME
  resource issued successfully and published its TLS Secret. Gateway served that
  exact certificate without restart; Center displayed its real Ready/expiry.
- Four browser/API/certificate checkpoints pass; a real menu trigger reports only
  queue admission. No private key was read by the proof, and no shared CRD changed.
- Reproduced an upstream contract failure: scheduler clearing sends null while
  the CRD requires object. The real service retries; an exact API dry-run returns
  422. See [ACME-SCHEDULER-CLEAR-GAP.md](ACME-SCHEDULER-CLEAR-GAP.md).
- [ACME-MENU-EVIDENCE.md](ACME-MENU-EVIDENCE.md) records artifacts and retained
  topology. Renewal is not claimed. No production source changes or new matrix
  claim in this pass; no Edgion edits, commits or push. Overall goal stays active.


### 2026-09-28: Gateway configuration list summaries and errors

- Corrected the list's obsolete spec.maxRetries lookup to spec.retry.attempts,
  preserving explicit zero. The editor already used the current nested field.
  List read failures now show the shared error/retry view instead of an empty table.
- Two focused component tests pass, including rejection of a conflicting legacy
  root value and list failure/recovery. Three native browser checks verify actual
  retry values 0/3, injected list failure, and recovery to actual rows.
- Artifacts: `/tmp/ws5-center-gatewayconfig-list-20260928/`; test log:
  `/tmp/ws5-center-gatewayconfig-list-tests.log`. The two owned, unreferenced
  configuration fixtures remain on Controller A. No active Gateway references
  were changed. Full frontend suite remains the prior 706-test checkpoint.
- Final build and lint passed (`/tmp/ws5-center-gatewayconfig-list-build.log`,
  `/tmp/ws5-center-gatewayconfig-list-lint.log`); existing bundle-size warning
  remains. No E2E types or action selectors changed.
