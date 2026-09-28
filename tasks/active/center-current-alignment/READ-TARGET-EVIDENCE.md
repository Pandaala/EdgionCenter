# Controller-bound metadata and topology reads

## Failure and repair

RestrictedDependenciesPage scoped its cache by route Controller but listKeys still
used the process-global proxy target. Topology had the same mismatch across its
22 inventory kinds. A refresh or delayed query during navigation could therefore
fetch another Controller's data into the route Controller's cache.

The metadata API now accepts an explicit target. Both consumers capture the route
identity with the existing shared hook and use it for cache keys and requests.
Topology passes it through namespaced, cluster-scoped and metadata-only reads.
Secret and ConfigMap reads remain metadata-only; permissions are unchanged.

## Verification

- 38 tests in four files pass, including two new actual-Axios-adapter scenarios.
  They hold the global Controller at an unrelated value, exercise both dependency
  tabs and all 22 topology kinds, switch the route, and verify old rows/nodes do
  not appear under the new Controller. Two API cases cover explicit direct and
  federation targets while preserving pagination options.
- Production build and lint pass. The existing bundle-size warning remains.
  Logs: `/tmp/ws5-center-read-target-{tests,build,lint}.log`.
- Two native browser checks pass against Center 12201 and Vite 15173. On Controller
  e2e-a/alignment-a, the harness deliberately sets the browser's global selection
  to e2e-b/alignment-b. ConfigMap metadata refresh still returns 200 through A;
  all 22 topology inventory requests still address A. The harness restores the
  global selection afterward. No resources, policies or credentials changed.
- Artifacts: `/tmp/ws5-center-read-target-20260928/browser-proof.cjs`, `result.json`
  and `topology.png`. The screenshot was visually inspected. Default Secret
  denial remains visible as partial topology; this run does not claim Secret
  read permission or complete topology availability.

This is targeted verification, not a new full frontend/backend matrix. The wider
alignment audit remains active. No Edgion files changed in this pass.
