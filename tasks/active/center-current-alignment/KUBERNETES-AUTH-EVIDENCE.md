# Deployed Kubernetes authentication evidence

Checkpoint: 2026-09-28. This closes the deployed authentication/capability slice,
not the complete alignment goal or cross-replica federation proof.

## Runtime and isolation

- Context: `orbstack`; namespace: `eruie2e-c0c61d3a-system`.
- Deployment: `eruie2e-c0c61d3a-center`, two ready replicas, each with the
  canonical pinned OAuth sidecar. Real Pod names/UIDs supply runtime identity.
- Image: `edgion-center-kubernetes:alignment-auth-20260928-v4`, built through
  `cicd/build-image.sh --mode kubernetes`, with the current embedded dashboard.
  Local image ID:
  `sha256:0884d3f79642f41935da0e4c01973f8f6fc6fff5a27a517061c73dbf6247c464`.
- Dex uses HTTPS discovery/JWKS and a dedicated fixture CA. Backend and proxy
  certificate verification remain enabled. The custom browser trusts only the
  fixture leaf SPKI; the repository Kubernetes browser config permits test TLS.
- Browser endpoint: `http://127.0.0.1:14181`; callback: `/oauth2/callback`.
  This isolated HTTP fixture uses a non-Host-prefixed insecure cookie;
  production cookie/TLS defaults are unchanged. OIDC setup accepts the exact
  explicitly configured loopback callback through `E2E_OAUTH_CALLBACK_URL`.
- Existing Controller and ProviderAccount CRDs matched current canonical
  schemas. Only the previously absent ProviderCapabilitySnapshot CRD was
  created, with the run ownership label. Existing shared Gateway CRDs were not
  overwritten. All created bindings, roles and namespace use the run prefix.
- Federation and internal forwarding use distinct test CAs. No Controller has
  yet been attached to this deployment; two ready replicas alone do not prove
  forwarding, ownership takeover, fencing or no-replay behavior.

## Results

Nine live RBAC checks passed: the runtime ServiceAccount can create SARs and
own-namespace Leases, but cannot create default-namespace Leases or list its
Secrets. The viewer can list own-namespace Controllers and read server info,
but cannot list default-namespace Controllers, create Controllers or manage users.

Five browser scenarios passed against the deployed v4 image:

1. OIDC viewer receives actual SAR read permissions and Controller list returns 200.
2. The binary reports Kubernetes capabilities, without SQL administration or
   password login; native RBAC and leader election are enabled.
3. Users, roles and audit menus are hidden, direct APIs return 403/404, and
   direct page navigation redirects home without exposing their actions.
4. Dashboard logout removes the proxy cookie and `/oauth2/auth` returns 401.
5. An authenticated unbound identity has no permissions, Controller reads return
   403, and the dashboard visibly reports lack of fleet access.

The existing repository authentication setup, identity endpoint and Kubernetes
capability test all passed: three tests, no skips. The initial v3 run had a real
blank-route failure; its failure artifacts are retained. The v4 fix adds a
catch-all redirect in the Center route tree, preserving destination/API guards.

Frontend validation: 635 tests in 102 files, build, lint and E2E type checks
passed. The full suite exposed a refresh-test timing race: the failure message
can precede completion of the other initial query. The test now waits for the
button to become clickable before verifying recovery. Production inventory
behavior was unchanged. Backend source was unchanged; the earlier matrix's
unrelated English-only guard limitation remains recorded in the coverage index.

## Artifacts and retained processes

Private runtime directory: `/tmp/ws5-center-kube-auth-20260928-v3`. The directory
name retains the original deployment run ID; its final browser results use v4.
Credentials, private keys, Secrets and browser session state remain local.

- `render.cjs`, `metadata.json`: composition recipe and owned resource metadata.
- `rbac-checks.json`: all nine live authorization requests and expectations.
- `browser-result-v3.json`, `repo-browser/`: original failing route evidence.
- `browser-result.json`, `browser-v4.log`, `viewer.png`, `denied.png`: final
  custom browser results and visible page evidence.
- `repo-browser-v4.log`, `repo-browser-v4/`: final repository browser evidence.
- `/tmp/ws5-center-kube-auth-image-v4.log`: successful image build.
- `/tmp/ws5-center-kube-route-tests-v2.log`: passing full frontend suite.
- `/tmp/ws5-center-kube-route-build.log`, `/tmp/ws5-center-kube-route-lint.log`:
  build and lint output.

At this checkpoint, owned port-forward session 62996 serves Center on 14181;
session 77091 serves Dex on 5556. The previous Center forward was stopped after
its selected Pod was replaced. Revalidate handles before reuse. Native OIDC
fixtures on ports 12201/15173/14180 remain separate and retained.

Next: attach the current Controller and prove actual owner/non-owner forwarding
and fencing. Dependency namespace browser coverage, cross-resource traffic and
other remaining original runtime criteria are still open.
