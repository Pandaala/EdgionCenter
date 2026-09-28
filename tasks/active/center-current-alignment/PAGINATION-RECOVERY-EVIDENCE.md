# Resource-list stale-cursor recovery

## Reproduction and repair

The Controller maps StalePagination to HTTP 410 in
`edgion-controller/src/api/common.rs`. The dashboard recognized that result but
called removeQueries, which removed the cache entry without restarting the
mounted infinite-query observer. Both namespaced and cluster-scoped reproductions
kept the old row instead of loading the new first page. Their failing baseline
is `/tmp/ws5-center-pagination-before.log` (two tests failed).

The shared hook now resets only the exact current query. This clears old pages
and continuation tokens and starts a new first-page fetch. Recovery honors the
enabled gate; switching Controller starts an independent recovery window.
Persistent first-page failure remains visible without an automatic loop, including
after the five-second notice window. Manual retry is available. The bilingual
notice now says the list is refreshing rather than claiming it already refreshed.

## Evidence and limits

- Six focused tests initially passed: four pagination cases plus existing access
  gating and Controller-switch tests. They cover both resource scopes, persistent
  failure/manual retry, Controller isolation and preservation of unrelated cache.
- The complete frontend suite passed 716 tests in 110 files. After the final
  persistent-error guard was tightened, all four pagination tests passed again,
  including manual retry beyond the notice window. Logs:
  `/tmp/ws5-center-pagination-full.log`, `pagination-tests.log` and
  `pagination-final-tests.log`, each with the same `/tmp/ws5-center-` prefix.
- Final production build and lint pass. Logs:
  `/tmp/ws5-center-pagination-final-build.log` and
  `/tmp/ws5-center-pagination-final-lint.log`. The existing bundle-size warning remains.
- Two browser assertions pass on the native Center/Vite runtime: an injected
  50-row first page and expired continuation automatically recover to the actual
  Controller A Service rows, and the list error clears without manual retry.
  The screenshot was visually inspected. This is an injected expiry response,
  not proof that the live Controller's cursor naturally expired.
- Artifacts: `/tmp/ws5-center-pagination-20260928/browser-proof.cjs`, `result.json`
  and `recovered.png`. Existing generic request-error toasts can briefly coexist
  with the recovery notice; the final list is the real native response.

No resources, policies, credentials or Edgion files changed. Backend gates were
not rerun for this frontend-only repair. The broader alignment audit is active.
