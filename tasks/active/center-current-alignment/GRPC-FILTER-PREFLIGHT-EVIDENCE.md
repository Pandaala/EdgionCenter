# GRPCRoute filter submission preflight

## Contract and change

Current `Edgion/edgion-resources/src/resources/grpc_route.rs::GRPCRouteFilterType`
contains RequestHeaderModifier, ResponseHeaderModifier and ExtensionRef. Both
rule-level and backend-level filters use it. The previous browser audit observed
an actual HTTP 400 when an invalid RequestMirror fixture reached the Controller;
see [BACKEND-NAMESPACE-EVIDENCE.md](BACKEND-NAMESPACE-EVIDENCE.md).

Center's selector already offered the correct three choices, but YAML drafts and
Form submissions after YAML import could still send unsupported discriminators.
The user then received the Controller's generic rejection instead of a location
they could correct before submitting.

The TypeScript filter union, selector and mutation preflight now share
GRPC_ROUTE_FILTER_TYPES. Both filter locations reject unsupported/missing types,
invalid entries and non-array containers with the exact rule/backend/filter path.
Omitted/null optional filter lists remain accepted. Validation does not mutate
the draft or discard unknown fields of supported filters. Existing delegation,
hostname and retry checks remain in place. Payload semantics and runtime
acceptance still belong to the Controller; this is not a complete CRD validator.

## Verification

- **44 tests passed** across filter preflight, route round trips, editor mutation
  integration and shared filter/policy controls. They cover every HTTP-only
  discriminator plus unknown types at both gRPC locations, malformed containers,
  preservation of supported payloads and unknown operator fields, and unchanged
  drafts after rejection.
- Build, ESLint, E2E TypeScript, strict inventory and diff checks passed. The
  existing bundle-size warning remains. The prior 760-test full frontend baseline
  is separate; this pass does not claim a new full-suite execution.
- **Seven native browser cases passed** in 40.4 seconds: authentication, existing
  gRPC actions/CRUD, and four new combinations of rule/backend filter location
  with Form/YAML submission.
- Each new browser case imports an unsupported RequestMirror draft, verifies the
  exact preflight error and zero create requests, confirms the resource is absent,
  and verifies the draft survives. Correcting it to RequestHeaderModifier causes
  exactly one successful create; actual Controller readback confirms the filter
  payload. Exact cleanup succeeds. No mocked API response is used.

## Artifacts and limits

- `/tmp/ws5-center-grpc-filters-tests.log`, `-build.log`, `-lint.log`, `-e2e-types.log`.
- `/tmp/ws5-center-grpc-filters-20260928/run.cjs` and `run.log`.
- `alignment-grpc-filters-20260928-1790561404689/` beneath that directory contains
  HTML output, success result, private runtime config and the retention ledger.

The isolated native runtime uses its own Center DB, mTLS identities and two
filesystem Controllers with explicit test grants. All 70 seed files remain
unchanged; only owned processes were stopped. Existing environments are untouched.
No Edgion files changed or were committed. This is browser/API persistence evidence,
not gRPC traffic or Kubernetes validation. Overall alignment remains active.
