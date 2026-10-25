# Current Edgion revalidation

Date: 2026-10-25. Status: complete for the requested current-source Center page/function debugging and retained preview. September acceptance is historical and does not
establish alignment with the updated Controller.

## Authority and source

- Center starts at `8c6cb74`; Edgion remains at `a59c3da81`.
- Preserve pre-existing Center `tasks/todo/` and Edgion `impl/`. No commit/push is
  authorized by the current request. No Edgion source has been changed.
- Initial OrbStack workloads have image pull failures; none has been changed.
- Current native Controller and Gateway builds pass (2m54s and 3m50s respectively).
  The unchanged Center backend binary is reused; the frontend runs current Vite.
- Fresh source inventory: exactly 22 resource kinds and 55 HTTP plugins on both
  sides. Federation proto differences are comments/spacing only.

## Implemented repairs

- HTTP/Stream ACL forms and templates use allowRefs/denyRefs; RealIp uses
  trustedIpRefs. A shared reference editor replaces the inline IP group editor.
  Mutations exclude IP read proofs, invalidInput and reference-denial markers.
- RequestBody uses maxBodySize and storage.{memoryLimit,maxBufferedSize,
  spillToDisk,operationTimeout}. Capacity/default-inheritance validation follows
  current Rust. Removed fields are rejected rather than silently accepted.
- CredentialSource publication.memoryMaxKeys and obsolete etcd fields/controls
  are removed. etcd validates millisecond deadlines and second-based keep-alive.
- Added RestToGraphql, GraphqlGuard, GraphqlSafelist, GraphqlRateLimit,
  GraphqlOperationRouter, GrpcJsonTranscoder and ResponseCache forms/types/stages.
  AiProxy exposes api/conversion/usageObservation. Current body gates and transported
  descriptor/locality/policy-admission exclusions are aligned.
- Current IP, plugin and LinkSys examples use current schemas. The AI credential
  reference is still flattened secretRef; its Secret key is now api-key. Both
  old preview Controllers reached Accepted=True/ResolvedRefs=True after the key
  was corrected through Center CAS writes.
- Controller test configurations use current named conf_sync cert sets. Native
  browser fixtures use mTLS; explicit plaintext-only test harnesses retain their
  audited transport choice with the new syntax.
- Removed unused Zustand and its Vite chunk entry. Added accessible field paths,
  ordered menu navigation and native admission/version assertions for new plugins.
- Reproduced the final YAML-editor failure as a failed jsDelivr loader request.
  Monaco, YAML/JSON support and workers are now locally bundled and lazy-loaded.
  Vite prebundles these entries to avoid closing dialogs during initial dependency
  discovery. The explicit external-network-blocked regression passes.

## Completed current checks

- Full frontend: 855 tests / 117 files pass, including after local Monaco changes.
  Log: `/tmp/center-local-monaco-frontend-20261025.log`.
- Current build/lint pass: `/tmp/center-local-monaco-{build-20261025-v3,lint-20261025-v2}.log`.
  Lazy editor/vendor bundle-size warnings remain.
- E2E typecheck and strict inventory pass: 22 kinds, six states, 268 actions,
  224 annotated cases. New plugin/menu cases are additional unannotated checks.
- Native federation: 27 lifecycle/CAS/watch/reload and nine mTLS/SPIFFE checks pass.
  Logs: `/tmp/center-current-{federation,mtls}-20261025.log`.
- Backend matrix: fmt, warning-denied Clippy, 876 workspace + 247 no-default-feature
  app tests, dependency purity and offline manifest rendering pass. Matrix exits
  1 at the unchanged root fix-issue-workflow-generic.zh.md English-only baseline.
  The later no-legacy guard and whitespace check pass separately. Web was skipped
  in that matrix because it was verified independently. External MySQL/Kubernetes
  opt-ins were not enabled.
- All seven new plugin browser form/CRUD flows prove current resourceVersion and
  Accepted=True. AiProxy also passed in the unfiltered run after correcting its
  Ant Design selector. Ordered navigation covers 17 Center and 25 Controller
  leaves in each Controller, including disabled DNS capability redirects.
- Offline Monaco and GatewayConfig CRUD focused retry: three checks pass;
  `/tmp/center-local-monaco-browser-20261025-v2.log`.

- Final clean dependency install: `npm ci` passes, followed by 855 frontend
  tests, lint, production build, E2E typecheck and strict inventory. Logs:
  `/tmp/center-review-clean-{install,frontend,lint,build,e2e-types,inventory}-20261025.log`.
  All 50 source hashes remain unchanged. Vite was restored afterward; both
  Gateway traffic paths and all eight owned processes were rechecked.

## Retained failures and evidence limits

- First 176-case run: 171 passed, three real schema mismatches, two Kubernetes-only
  skips. All three corrected cases pass their targeted rerun.
- First 185-case run: 182 passed, two Kubernetes-only skips, one Monaco CDN loading
  failure. Original traces/screenshots remain in web/test-results/center-current-final-20261025.
- New test development retained its original selector/locator failures. A focused
  command without E2E_PLAYWRIGHT_GREP passed four tests but correctly failed the
  full-case ledger gate; the corrected focused invocation exited successfully.
- Browser plugin discovery returned no connections. Existing repository Playwright
  runs use installed Chromium. No interactive Browser-plugin inspection is claimed.
- Native UI/admission evidence does not certify all Gateway protocols/providers.

## Final acceptance and retained environment

The full final standalone browser run exited zero: **184 passed, two
Kubernetes-only skips**, 8.3 minutes, 186 total tests. Its complete annotated
ledger contains 112 passed cases. Source hashes for all 50 task code/test/build
files remained unchanged throughout that run and at final verification.

- Artifact root: `web/test-results/center-review-20261025/`.
- Final browser log: `run.log`; report: `html/index.html`.
- Source/kind/plugin/case verification: `source-and-coverage-audit.json` and
  `source-manifest.json`.
- The completed harness stopped its children. Eight separately started preview
  processes are now verified alive: one Center, two Controllers, two Gateways,
  Vite and two origins. Their identities are in preview-pids.json and
  traffic-preview-pids.json. They run independently of tool-session lifetime.
- Actual preview URL: http://127.0.0.1:15173. The generated account and password
  are in the private preview-access.txt. README.md documents ports, restart,
  examples, ownership and verification limits.
- 122 actual resource YAML files remain, 61 per Controller. Both Controllers
  report online with fresh StatsReport counts of 61. All eight global inventory
  categories have groups with both Controller observations. All eight LinkSys
  and nine ConfigData variants are represented, alongside deliberate rejected,
  unresolved and conflict examples.
- The 90 seed files were retained by the browser harness. Afterward,
  apply-demo.mjs created 32 more resources and performed four CAS updates through
  Center federation only. Their exact identities and versions are recorded in
  demo-resources.json. No managed-cluster Kubernetes API was used.
- Final Controller federation RBAC names concrete kinds/verbs, with no wildcard.
  Both Controllers permit Secret metadata listing and deny Secret value GETs
  with 403. ConfigSync uses a dedicated preview mTLS CA; federation remains
  strict mTLS. Preview certs expire on 2026-11-24.
- traffic-results.json records real HTTP 200 responses through both current
  Gateways: Host review.edgion.test on port 18080 reaches east, and port 28080
  reaches west. The corresponding owned HTTPRoutes have current Accepted=True
  and ResolvedRefs=True. Gateway startup initially failed on the binary's
  default privileged log directory; private configs now use run-owned logs.
- demo-verification.json verifies online/fresh counts, all eight populated global
  types, processed samples (including accepted ConfigData and resolved routes),
  and the metadata/value permission boundary.
- After installing the populated preview and concrete permissions, the ordered
  67-page menu traversal plus offline editor and authentication checks pass
  again (three tests, 58.6 seconds): /tmp/center-review-final-menus-20261025.log.

- Clean-install preview recheck: authentication, ordered 67-page navigation and
  external-network-blocked YAML editor pass again (three tests, 58.2 seconds):
  `/tmp/center-review-clean-final-browser-20261025.log`.

## Objective-to-evidence audit

| User requirement | Current inspected evidence |
| --- | --- |
| Align with updated Edgion | Current source equality for 22 kinds and 55 plugins; current Controller/Gateway builds; requestBody/IP/LinkSys/AI/plugin/projection repairs; no wire-layout divergence |
| Debug every page/function in menu order | 17 Center leaves plus 25 leaves in each Controller, visited in menu order; all 22 browser/API CRUD workflows, shared actions, administration, RegionRoute, typed variants and eight new plugin workflows in the complete regression |
| Remove obsolete code | Inline IP editor/types, old request-body controls/types, obsolete CredentialSource/etcd controls and fields, unused Zustand dependency/chunk entry removed |
| Use multiple Edgion instances for testing | Two current native mTLS Controllers and Gateways; federation lifecycle/security tests and real east/west traffic; existing OrbStack workloads preserved |
| Populate diverse YAML | 122 retained files; all catalog kinds and configuration subtype menus populated; raw files, status sidecars and an additional YAML sample bundle retained |
| Test the implemented behavior | 855 frontend tests; 184 passing browser cases plus two explicit mode skips; 876+247 backend tests; 27+9 native federation tests; current-version plugin admission, offline Monaco, concrete permissions and dual-Gateway traffic evidence |
| Leave a ready preview | Eight live owned processes, successful login/menu requests, both online Controllers, fresh counts, real Gateway responses, private access file and restart scripts |

No requested Center repair or page inventory item remains open. This acceptance
covers the requested Center functionality and selected live routing demonstrations,
not certification of every Gateway protocol or external provider. Validation
limits remain explicit: two Kubernetes-only browser cases and external MySQL/real
cloud opt-ins were not run; the complete backend matrix exits at the preserved
unrelated root English-only baseline despite passing its implementation gates.
No commit or push was performed. Edgion source and pre-existing task artifacts
remain untouched.
