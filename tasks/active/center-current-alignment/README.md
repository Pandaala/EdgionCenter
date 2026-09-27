# Center alignment with current Edgion

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
