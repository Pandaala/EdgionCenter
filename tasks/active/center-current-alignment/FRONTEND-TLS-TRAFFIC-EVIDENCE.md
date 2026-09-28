# Gateway frontend TLS transition evidence

Date: 2026-09-28. Center source: `36d771d`; production implementation remains
the same as the complete browser regression at `ba012f3`.

The isolated harness `/tmp/ws5-frontend-tls-runtime.cjs` reuses the private
native Center/Controller/Gateway composition from the AI traffic harness,
with newly generated certificates, a separate database and configuration tree.
Center federation uses mTLS; Gateway config sync uses the isolated plaintext
test profile. Center Admin/federation are `17601/17651`, Controller
Admin/config sync are `17701/17751`, Gateway Admin is `17801`, Vite is `17573`,
HTTPS traffic is `18600`, and the local HTTP backend is `18601`.

The fixture uses the operator-owned Gateway `spec.tls.frontend.default/perPort`
API and a normal HTTPS listener with a serving certificate Secret. Both default
and per-port validation initially require the first client CA. Each traffic
probe opens a fresh TLS connection with server certificate/hostname verification
enabled and no reusable agent or session. It probes no client certificate, a
client signed by the first CA, and a client signed by a second CA.

## Observed transitions

| Center operation | No client certificate | First CA client | Second CA client |
| --- | --- | --- | --- |
| Initial strict default and strict port override | Rejected | HTTP 200 | Rejected |
| Form changes port mode to AllowInsecureFallback | HTTP 200 | HTTP 200 | HTTP 200 |
| Form restores AllowValidOnly | Rejected | HTTP 200 | Rejected |
| Form clears port validation, keeping `tls: {}` | HTTP 200 | HTTP 200 | HTTP 200 |
| Form deletes port override, inheriting strict default | Rejected | HTTP 200 | Rejected |
| Center API rotates client CA Secret to second CA | Rejected | Rejected | HTTP 200 |
| Center API restores original client CA | Rejected | HTTP 200 | Rejected |

Every successful request must return the fixture backend body. Rejections must
be TLS protocol/certificate/reset failures, not connection-refused or timeouts.
Each strict-state observation also proves that its trusted client reaches the
backend, so an unavailable listener cannot satisfy a rejection checkpoint.
Bounded polling waits for the whole three-client matrix, not merely Controller
status or a configuration-list entry.

All mode, clear and delete-override operations use the Gateway dashboard form.
The harness checks successful PUTs, raw Controller readback and omission of
the private listener `frontendValidation` projection from submitted documents.
CA replacement goes through Center's authenticated federation proxy API.
No managed Kubernetes API is used, and Gateway is not restarted between states.

## Results

- **9 checkpoints passed**, including seven complete handshake matrices;
  26 matrix observations including convergence polling.
- Result: `/tmp/ws5-center-frontend-tls-1790578399589/result.json`.
- Log: `/tmp/ws5-frontend-tls-runtime.log`; screenshot and private fixtures
  are retained in the result directory.
- Harness exited zero. Its Center, Controller, Gateway and Vite listeners
  were confirmed absent after owned-process cleanup.
- The first attempt passed strict baseline traffic but clicked Ant Design's
  covered combobox input instead of its visible selector. The corrected harness
  clicks the selector. The failed attempt is retained in
  `/tmp/ws5-frontend-tls-selector-attempt.log`; it is not a product failure or a
  passing full run.

This closes the selected frontend TLS transition gap left by the earlier
configuration-only browser cases. It verifies Gateway HTTPS client validation,
default inheritance and CA hot reload; it does not claim every EdgionTls mode,
TLSRoute termination or TLS cipher/protocol combination.
