# Provider account Kubernetes browser evidence

Checkpoint: 2026-09-28. Current dashboard source ran through Vite on loopback
15174 against the retained two-replica v5 Kubernetes Center deployment, through
its OAuth sidecar. The embedded v5 dashboard predates these frontend repairs;
this is current-source browser proof, not a claim that the image was rebuilt.

## Repairs

The ProviderAccount API defaults to 50 items and returns an opaque continuation
token. `cloudApi.listAccounts()` previously ignored the token, hiding later
accounts in the account table and both DNS account selectors. It now retrieves
all pages, fails the entire read if a later page fails, and rejects repeated
cursors instead of looping. The existing client-side table pagination is retained.

Account list errors now show a sanitized failure warning with cached data
retained. Capability loading, read failure, and a successful `not_discovered`
response have distinct states. A failed refresh warns that displayed evidence
may be stale and does not claim that the snapshot is absent.

The existing provider-account CRUD/CAS E2E case now runs for both store modes.
The Kubernetes fixture has the same namespace-scoped provider persistence
permissions as the canonical deployment and specific account API/credential-use
grants for its test user. Its preflight requires all three Center CRDs. It adds
no Secret access, provider account deletion or external provider mutation rights.

## Real backend and browser results

The isolated namespace is `eruie2e-c0c61d3a-system` on context `orbstack`.
Provider adapters remain disabled; every fixture uses an unresolved credential
reference. No cloud credential was supplied or inspected and no DNS call ran.

- Read-only user: account menu visible, create button absent, POST returns 403.
- Account-write without credential-use: create button remains absent and POST
  still returns 403, proving both authorization boundaries are required.
- With both grants: the repository browser case passed creation, API label
  preservation, concurrent revision 412, draft retention and a fresh successful
  edit. It used the actual Kubernetes store, not browser response mocks.
- Created 51 additional metadata-only fixtures, yielding 52 real
  EdgionProviderAccount CRDs. The server returned 50 items plus a continuation
  token; the dashboard fetched the next API page and showed the last account
  on table page 3.
- Revoked the actual SAR list grant while the page was open. Refresh showed
  the failure warning and retained cached rows; restoring the grant and
  refreshing recovered the list.
- Revoked capability-read access after a successful no-snapshot read. The
  drawer showed a failure warning and removed the no-snapshot claim. Restoring
  access and refreshing returned to the actual no-snapshot state.
- A credential-value field injected into a create request returned 400; the
  account remained absent and the sentinel value was not persisted in any CRD.
- Deleted all three temporary grant roles and bindings. Both replicas return
  403 for account reads again; the menu is hidden and its deep link redirects
  home. The original viewer authorization is restored.

Fixtures are retained in the run-owned namespace; account deletion is not an
Admin API operation. The new account list helper also serves DNS selectors,
but this run did not enable or claim real cloud DNS mutations.

## Validation and artifacts

642 frontend tests in 102 files passed. Build, lint, E2E TypeScript, inventory
(22 kinds, 266 actions, 224 cases) and shell syntax checks passed. The first
focused run had one test locator mismatch (`reload Refresh` versus `Refresh`),
corrected before the passing full suite. No backend Rust source changed.

The updated real-cluster preflight passed API discovery, including all three
Center kinds, then correctly exited 1 on six existing shared Edgion schema
differences: EdgionBackend, EdgionConfigData, EdgionGatewayConfig, EdgionPlugins,
EdgionStreamPlugins and LinkSys. No shared CRD was overwritten. The full gateway
Kubernetes suite remains distinct from this isolated Center-only browser run.
Log: `/tmp/ws5-center-provider-kube-preflight.log`.

Private artifact directory: `/tmp/ws5-center-provider-kube-20260928`.

- `permission-read.json`, `permission-write.json`: staged permission checks.
- `repo-browser.log`, `repo-browser/`: passing repository CRUD/CAS browser case.
- `pagination-recovery-result.json`: six actual API/browser scenarios.
- `crd-summary.json`: retained CRD names/count, without credential material.
- `account-read-denied.png`, `capability-read-denied.png`: visible failure states.
- `restored.json`: permission restoration on both replicas and route/menu proof.
- `vite.log`: retained current dashboard process, session 97217 on port 15174.
- `/tmp/ws5-center-provider-pages-full.log`: 642 passing frontend tests.
- `/tmp/ws5-center-provider-build.log`, `/tmp/ws5-center-provider-lint-v2.log`,
  `/tmp/ws5-center-provider-e2e-types-v2.log`, `/tmp/ws5-center-provider-inventory.log`:
  build and tooling gates.

This does not establish external credential resolution, discovered provider
authority or live provider mutations. Those remain separate from metadata CRUD.
