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
| Shared infrastructure | Resource catalog, mutation envelopes, conditions, permissions, Controller switching, pagination | In progress: deployment status display and currentStatus filtering |
| Infrastructure | GatewayClass, Gateway, ReferenceGrant | Pending |
| Routes | HTTPRoute, GRPCRoute, TCPRoute, UDPRoute, TLSRoute | Pending |
| Services | Service, EndpointSlice, EdgionBackendTrafficPolicy | Pending |
| AI backends | EdgionBackend, HTTPRoute references, AiProxy, policy attachments | Catalog/access map, menu, editor, route/plugin/policy wiring added; native browser CRUD passed; advanced attachment and failure workflows pending |
| Security | EdgionTls, BackendTLSPolicy, Secret/ConfigMap restricted dependencies | Pending |
| Plugins | EdgionPlugins, EdgionStreamPlugins, EdgionConfigData | Pending; inspect each plugin configuration against current schema |
| System | EdgionGatewayConfig, LinkSys, EdgionAcme | Pending |
| Controller views | Operations dashboard, user dashboard, topology, RegionRoute | Pending |
| Federation views | Center dashboard, Controllers, registration, counts, proxy, watches, reload | Pending |
| Global views | RegionRoute overrides, global ConfigData inventory and subtypes | Pending |
| Cloud | Provider accounts, Cloudflare DNS, Route53 DNS | Pending |
| Administration | Login/discovery, audit, users, roles, standalone/Kubernetes capabilities | Pending |
| Runtime validation | Local Center/frontend, current Controller, browser workflows, full matrix | Pending |

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
