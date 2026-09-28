# Route parent-reference clearing

## Finding and change

HTTPRoute still imported a private copy of ParentRefsSection, while GRPCRoute,
stream routes and EdgionTls used the shared component. The private copy wrote
empty strings when optional namespace/sectionName inputs were cleared and used
hard-coded bilingual labels instead of the current locale. A new component
regression reproduced the two empty fields before the repair.

Current Edgion `edgion-resources/src/resources/common/mod.rs::ParentReference`
represents these fields as optional. Its `build_parent_key` falls back to the
route namespace only when namespace is absent, not when it is an empty string.
Omitted sectionName removes that explicit listener restriction; port, when
present, still narrows selection.

HTTPRoute now imports `ResourceEditor/common/ParentRefsSection`. The unused
185-line private implementation is deleted. This reuses the existing omission
behavior and translated controls, retaining reference identity, advanced fields,
sibling references and the existing last-reference removal policy.

## Verification

- Before repair: the new component regression failed because namespace and
  sectionName were both empty strings. The existing annotation test passed.
- After repair: **17 tests passed** across HTTPRouteForm, EdgionTlsForm and
  route editor integration suites. The new case checks optional-field omission,
  unknown-reference-field preservation, sibling references and the English label.
- Production build, ESLint, E2E TypeScript, strict inventory and diff checks pass.
  The existing bundle-size warning remains. No new full frontend-suite result is
  claimed; the prior 755-test checkpoint is separate.
- **Nine native browser cases passed** in 34.6 seconds: authentication, HTTPRoute
  actions/CRUD/policy clearing, and one new parent-clearing workflow for each of
  HTTPRoute, GRPCRoute, TCPRoute, UDPRoute and TLSRoute.
- Each new workflow creates a labeled fixture through the Controller proxy,
  clears namespace/sectionName in the actual form, checks Form/YAML round-trip,
  saves and verifies exact parentRefs through API readback. Gateway name, port
  and all route rules are preserved. Exact cleanup succeeds.

The first browser attempt had four passing cases and five selector failures:
both the Gateway and backend reference cards expose the resource namespace as a
placeholder. Scoping the selector to the Gateway card fixed the harness. The
failure traces/screenshots remain separate from the fresh passing execution;
there was no further production-code change.

## Artifacts and limits

Root: `/tmp/ws5-center-http-parent-20260928/`.

- `run.cjs`, `run.log` (initial selector failures), `run-final.log` (nine passed).
- Failed runtime: `alignment-http-parent-20260928-1790560406136/`.
- Passing runtime: `alignment-http-parent-20260928-1790560486875/`; includes HTML
  report, success result, private configs and verified retained-fixture ledger.
- `/tmp/ws5-center-http-parent-before.log`, `-tests.log`, `-build.log`, `-lint.log`,
  `-e2e-types.log` and `-e2e-types-final.log` retain the respective static results.

The passing run retains all 70 seed files unchanged. Only owned fixture resources
and processes were cleaned. Existing native/OrbStack environments were untouched.
No Edgion files were changed or committed. This proves editor/API persistence,
not data-plane traffic or a new Kubernetes runtime check. The overall audit
remains active.
