# Legacy resource-list action permissions

## Scope and repair

Eleven active resource lists still used ordinary buttons for view/create/edit/
delete/refresh and, where present, batch delete: Gateway, GatewayClass,
ReferenceGrant, HTTPRoute, GRPCRoute, TCPRoute, UDPRoute, TLSRoute, EdgionTls,
EdgionAcme and EdgionGatewayConfig. Default Controller read-only permissions
therefore left write buttons apparently usable until the server rejected them.

These actions now use the existing PermissionAwareButton, specifying the exact
resource kind and verb. The shared component combines Center proxy permissions
with the current Controller access document and fails closed during authorization
refresh/failure. No backend policy or permission definition changed. Server-side
authorization remains authoritative, including races after a dialog opens.

The only remaining ordinary action buttons found in these resource-page folders
belong to the unused SecretList module, which App does not route to. Active Secret/
ConfigMap management uses the already permission-gated metadata-only page.

## Verification

The eleven new page tests exercise read-only access, pending authorization,
separately granted create/update versus delete, complete grant and revocation.
They use the real shared button component, verify concrete-kind matching, and
include selected-row batch controls where available. Disabled clicks never open
confirmation dialogs. The complete frontend suite passes 737 tests in 111 files.

Native browser evidence on Center 12201 / Vite 15173 covers all eleven menus with
the actual default Controller A policy, plus the dedicated ACME Controller's
explicit writable policy (12 checkpoints). Read controls remain enabled, write
controls disabled under the default policy; ACME write controls become enabled
where actually authorized. No proxy mutations were sent and no policies changed.

Gateway, GatewayClass, ReferenceGrant, HTTPRoute, ACME and GatewayConfig had actual
rows in this native run. GRPC/TCP/UDP/TLSRoute and EdgionTls were empty, so their
native evidence covers toolbar actions; their row/batch grant and revoke behavior
is covered by component tests rather than invented native rows.

Artifacts: `/tmp/ws5-center-list-actions-20260928/proof.cjs`, `result.json`,
`authorized-acme.png` (visually inspected). Logs:
`/tmp/ws5-center-list-actions-tests.log`, `/tmp/ws5-center-list-actions-full.log`,
`/tmp/ws5-center-list-actions-build.log`, `/tmp/ws5-center-list-actions-lint.log`.
Build/lint pass with the existing bundle warning. No backend matrix rerun.

## Gateway guide follow-up

Source/annotation review also corrected the Gateway form's StreamPlugins hint:
Gateway attachment requires a same-namespace name, unlike route annotations that
can accept namespace/name. The infrastructure guide now uses the exact dynamic
certificate provider value EdgionTls, removes stale pending-development notes,
and describes actual list/editor behavior. These are documentation/input-hint
corrections, not new runtime support. Final build after the hint edit is recorded
in `/tmp/ws5-center-list-actions-final-build.log`.

The broader alignment audit remains active. No Edgion files changed in this pass.
