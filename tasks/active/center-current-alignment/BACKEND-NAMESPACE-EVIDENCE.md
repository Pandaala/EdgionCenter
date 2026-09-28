# Optional backend namespace clearing

## Finding and contract

HTTP, gRPC and stream backend-reference editors all wrote `namespace: ""` when
their optional namespace field was cleared. The HTTP/stream controls also showed
the route namespace as a value even when it was omitted. Three new component
cases reproduced the empty-string writes before repair.

Current Controller resolution distinguishes absence from an empty namespace:
`edgion-controller/src/conf_mgr/sync_runtime/resource_processor/handlers/http_route.rs`
uses `backend_ref.namespace.as_deref().unwrap_or(route_ns)` in backend validation.
The shared `ref_grant/validator.rs` treats a present namespace different from the
owner as cross-namespace. Empty string therefore does not restore the owner
namespace. The same issue affected HTTP RequestMirror's backend-reference form.

## Change

- All three backend editors omit namespace when cleared and display the owner
  namespace as a placeholder. Their accessible namespace labels use existing
  translations; the HTTP namespace label no longer hard-codes Chinese text.
- HTTP RequestMirror applies the same omission behavior and exposes a distinct
  accessible namespace label. Its target identity and sampling policy remain.
- Narrow edits preserve port, explicit zero weight, filters, unknown operator
  fields and sibling backends. No authorization or resource schema is widened.

## Verification

- Before repair: all three new backend namespace component cases failed.
- Initial focused verification: 17 tests passed across backend editor and route
  integration suites. The subsequent HTTP mirror case is included in the full run.
- Final full frontend: **760 tests / 115 files passed**. Production build, lint,
  E2E types, strict inventory and diff checks pass. The existing bundle-size
  warning remains.
- Final native browser run: **11 passed** in 1.1 minutes. This covers authentication,
  actual CRUD for all five routes and five new backend-clearing workflows.
- The workflows create isolated route fixtures, clear namespace in the actual
  form, inspect Form/YAML round-trip, save, and read back the Controller document.
  All five routes retain port and weight zero. HTTP additionally clears its
  RequestMirror namespace while preserving mirror sampling and other filters.
  Parent references remain unchanged and exact fixture cleanup succeeds.

The first native attempt passed ten cases and failed during creation of the gRPC
fixture with HTTP 400. The harness had incorrectly added RequestMirror to GRPCRoute.
Current `edgion-resources/src/resources/grpc_route.rs::GRPCRouteFilterType` supports
only RequestHeaderModifier, ResponseHeaderModifier and ExtensionRef, matching the
dashboard's existing choices. Corrected the fixture and mirror test scope to HTTP;
the fresh 11-case execution passed. Original failure artifacts are retained.

## Artifacts and limits

- `/tmp/ws5-center-backend-namespace-20260928/run.cjs`, `run.log` and `run-final.log`.
- Initial failed runtime: `alignment-backend-namespace-20260928-1790560846844/`.
- Passing runtime: `alignment-backend-namespace-20260928-1790561045985/`; includes
  HTML output, success result, private runtime configuration and retention ledger.
- `/tmp/ws5-center-backend-namespace-before.log`, `-tests.log`, `-full-final.log`,
  `-build-final.log`, `-lint-final.log` and `-e2e-types-corrected.log`.

The runtime uses a fresh native Center database, two filesystem Controllers,
generated credentials/mTLS and concrete test grants on isolated ports. All 70
seed files are retained unchanged; only owned processes were stopped. Existing
native and OrbStack environments are unchanged. No Edgion files were modified or
committed. This proves configuration persistence, not mirror traffic, a new
ReferenceGrant scenario or Kubernetes execution. The overall audit remains active.
