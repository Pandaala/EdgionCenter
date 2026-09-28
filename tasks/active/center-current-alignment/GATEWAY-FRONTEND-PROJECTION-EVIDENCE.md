# Gateway frontend TLS operator boundary

Date: 2026-09-28. Overall alignment remains active.

## Current authority and repair

The current Controller gateway handler clears listener `tls.frontendValidation`
on every ingestion and derives it only from `spec.tls.frontend`. See
`../Edgion/edgion-controller/src/conf_mgr/sync_runtime/resource_processor/handlers/gateway.rs`,
`parse` processing near lines 626-642 and `project_standard_frontend_validation`.
The resource field is schema-hidden in `GatewayTLSConfig`; the TLS review guard
`frontend-validation-is-mtls-empty-ca.md` also identifies the sole operator path.
TLS/Terminate remains unsupported, so existing protocol/mode guards are retained.

Center incorrectly offered listener-level mTLS controls, validated that internal
projection as user input, and preserved it in mutations. Removed those controls
and that validation; added the exact path to the catalog mutation exclusions.
Gateway-level defaults and per-port overrides remain editable and preserved.
Read/YAML normalization stays lossless. The infrastructure example now uses the
actual operator API. No Edgion changes were made in this follow-up.

## Verification

- 56 focused tests across Gateway adapters, catalog, observations and TLS forms.
- Build, lint and E2E typecheck pass.
- Four native browser cases pass: auth, Gateway menu actions, full Gateway CRUD,
  and the new frontend policy boundary scenario (22 seconds).
- The new case injects a malformed runtime projection into an editable YAML draft,
  switches to Form, edits the per-port CA reference, inspects the submitted PUT,
  and reads the actual Controller document back. The private field is omitted;
  the default and corrected per-port operator policy survive.
- Passing run: `alignment-gateway-projection-20260928-1790563047179` under
  `/tmp/ws5-center-gateway-projection-20260928/`; log `run-final.log`.
- First run: three passed, the new fixture POST failed because its frontend CA
  reference lacked required group. Fixed the fixture with explicit group; retained
  original log/trace. This was before the editor workflow, not a successful gate.
- Focused logs: `/tmp/ws5-center-gateway-projection-{tests,build,lint,types}.log`.
- Owned processes stopped; 70 unchanged seed files retained, exact mutation fixture
  cleanup succeeded. No TLS handshake claim is made.

## Next concrete audit

Frontend CA references require group, kind and name (dedicated
`FrontendCACertificateRef`), with 1-16 entries. Center still uses its generic
optional-group/optional-kind certificate reference model and preflight. Verify and
align that boundary, including the required frontend default object when starting
with a per-port policy. Existing form tests use some shapes the current Controller
will not deserialize; native validation must remain the authority.

The concrete next audit above is resolved by [frontend CA evidence](GATEWAY-FRONTEND-CA-EVIDENCE.md).
