# EdgionTls Gateway attachment editing

## Current contract and finding

Current `Edgion/edgion-resources/src/resources/edgion_tls.rs` exposes optional
parentRefs with at most 32 entries. Each reference carries Gateway identity,
optional namespace, listener sectionName and port. The Controller handler in
`edgion-controller/src/conf_mgr/sync_runtime/resource_processor/handlers/edgion_tls.rs`
warns that an object without parentRefs applies to no Gateway and resolves
attachments only against HTTPS/TLS listeners.

Center preserved parentRefs in YAML but did not render an attachment form,
despite its resource guide listing that section. A new certificate configured
entirely through the form could therefore remain unbound.

## Change

- EdgionTlsForm now reuses ParentRefsSection. Users can edit Gateway, namespace,
  listener, port, group and kind, with the resource namespace used for additions.
- EdgionTls opts into removing the last optional reference and omits parentRefs
  when none remain. The add control stops at 32. Existing references and unknown
  operator fields survive narrow edits; read-only controls remain disabled.
- Shared namespace/sectionName clearing now omits the optional value instead of
  writing an empty name. The namespace input displays omission via its placeholder.
  GRPCRoute and stream-route forms share this control; their default behavior of
  retaining the last reference is preserved.
- The security guide now describes attachment removal and the actual metadata-only
  Secret/ConfigMap workflow. It no longer proposes an existing-secret value viewer
  or describes that implemented page as pending.

## Verification

- Seven focused component/adapter checks passed, including four new form cases:
  lossless multiple-reference edits, optional-field omission, last-reference
  removal/addition, limits/read-only behavior and the existing shared route default.
- Full frontend: **755 tests / 114 files passed**. The final form-only rerun covers
  the last reference-copy adjustment. E2E types, lint, strict inventory and final
  production build passed; the known bundle-size warning remains.
- The first build found a TypeScript mismatch between ParentReference and the
  extensible EdgionTls ObjectRef. Copying each complete reference gives the form
  the correct structural type without dropping unknown keys. The failed initial
  build log and passing final build log are both retained.
- **12 native browser cases passed** in 1.4 minutes: authentication, action and
  actual CRUD cases for EdgionTls/GRPCRoute/TCPRoute/UDPRoute/TLSRoute, and the new
  attachment-specific workflow.
- The new workflow creates an isolated EdgionTls, adds a Gateway/listener/port
  binding through the form, checks Form/YAML round-trip, saves and reads it back
  through the real Controller proxy. It then clears namespace/sectionName,
  verifies omission, removes the final reference, and verifies parentRefs is
  absent while hosts and certificate identity remain. Exact cleanup succeeds.

This proves configuration editing and persistence, not successful TLS handshakes
or certificate selection by a Gateway. The fixture's referenced listener is TLS;
no live data-plane behavior is inferred from the save. No Kubernetes execution
of this new case is claimed.

## Runtime and artifacts

The isolated native Center, Vite and two filesystem Controllers use the previous
regression's port layout (16201/16251, 16173, 16301/16401) with a fresh SQLite DB,
generated credentials, mTLS and explicit concrete-kind test grants. Existing
native/OrbStack environments and their policies are unchanged. No Edgion files
were changed or committed in this pass.

- `/tmp/ws5-center-tls-attachments-20260928/run.cjs` and `run.log`.
- Its `alignment-tls-attachments-20260928-1790560008173/` directory contains HTML
  output, success result, private runtime configs and the fixture ledger.
- `/tmp/ws5-center-tls-attachments-tests.log`, `-full.log`, `-final-form.log`,
  `-lint.log`, `-e2e-types.log`, `-build.log` and `-build-final.log`.

All 70 labeled seed files are retained and verified unchanged. Temporary CRUD
resources were deleted exactly; only this run's processes were stopped. The
broader alignment audit remains active.
