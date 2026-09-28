# Clearing Service targetPort

## Current contract and repair

Gateway `backends/discovery/coordinator.rs::normalize_service_port` treats absent
targetPort as the Service port number, accepts nonempty names, and rejects empty
strings. The Center form converted a cleared targetPort input to an empty string,
which could invalidate an otherwise usable Service port.

Clearing now removes the optional property. Numeric and named values retain their
types. Three added Form-to-YAML-to-Form submission regressions cover clearing,
number and name edits, retained sibling ports/appProtocol/nodePort/unknown fields,
update resourceVersion and removal of status.

## Verification

13 editor/core-resource adapter tests pass; production build and lint pass.
The existing bundle-size warning remains. Logs:
`/tmp/ws5-center-service-port-{tests,build,lint}.log`.
This is focused verification, not a new full frontend count.

The completed native browser run passes three checks:

1. An external harness puts targetPort="" on the owned Service and observes actual
   Gateway 503, reproducing the old form's runtime effect.
2. The actual Center Service form clears the field and saves successfully through
   the federation proxy. Controller readback proves targetPort absent; Gateway
   traffic returns 200.
3. Original Service configuration and the exact Controller config text are restored,
   and Center's temporary Service update permission is confirmed revoked.

The fixture is the previously owned `center-region-flow/center-endpoint-ready`
Service, labeled `alignment-run: endpoint-ready-20260928`. The test temporarily
adds only the concrete Service update verb to the existing effective default
permissions; no wildcard or Secret access is granted. Policy changes use only
Controller A's private test config. Native Center/Vite and Gateway ports remain
12201/15173/18200. No repository config, CRD or user resource changed.

Artifacts: `/tmp/ws5-center-service-port-20260928/proof.cjs`, `result.json` and
`saved.png` (visually inspected). Three earlier harness attempts are failed runs:
the first did not wait for reload completion and received 503, requiring separate
policy recovery; the next two used an incorrect exact accessible button name
(the icon contributes to that name). The final harness waits for changed server_id
and ready state, uses the existing stable edit test ID, and restores policy even
if Service cleanup fails. Failed-attempt records are retained separately.

The broader menu/resource audit remains active. No Edgion code changed in this pass.
