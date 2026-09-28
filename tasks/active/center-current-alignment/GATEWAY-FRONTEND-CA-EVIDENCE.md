# Gateway frontend CA admission and per-port creation

Date: 2026-09-28. Overall audit remains active.

Current authority is `FrontendCACertificateRef`, `deserialize_frontend_ca_refs`,
`GatewayFrontendTLS` and `GatewayFrontendTLSPerPort` in sibling
`edgion-resources/src/resources/gateway.rs`, plus Controller
`validate_frontend_ca_reference` in the Gateway handler. No Edgion history was read
and no Edgion source was changed in this follow-up.

## Changes

- Validate frontend references separately from serving/client certificate refs:
  group must be present and exactly empty; kind is required with its current
  syntax/63-character bound; name is required and at most 253 UTF-8 bytes;
  optional namespace uses the current DNS-label/63-character rule.
- Enforce 1-16 references and reject explicit invalid validation modes, including
  an empty mode. Error paths identify default/per-port reference indexes.
- Require the frontend default object and each per-port TLS object. Empty objects
  remain legal; drafts are never silently normalized by preflight.
- Creating a per-port policy through Form now writes the required empty default
  object when absent, retaining backend configuration and existing defaults.
  Add controls stop at 16 CA references and 64 port overrides.

## Evidence

- 48 focused tests in four files pass, including actual editor submission tests.
- Build, lint and E2E typecheck pass.
- Four native browser cases pass (29 seconds): auth, Gateway menu, CRUD, and
  frontend policy editing. The last case now also starts with an empty Gateway TLS
  object, adds a port override and CA reference through Form, saves, and verifies
  the explicit default object and full reference using the Controller API.
- Run: `alignment-gateway-ca-20260928-1790563305347`; root `/tmp/ws5-center-gateway-ca-20260928/`.
- Focused logs: `/tmp/ws5-center-gateway-ca-regression-final.log`,
  `/tmp/ws5-center-gateway-ca-{build,lint,types}.log`.
- An expanded first component pass exposed an old valid-fixture assumption:
  missing default and group. Corrected that fixture to current Controller shape;
  retained its failing log `/tmp/ws5-center-gateway-ca-regression.log`.
- Owned runtime stopped; 70 unchanged seed files retained. No handshake claim.

This resolves the concrete CA-reference/default-container follow-up recorded in
GATEWAY-FRONTEND-PROJECTION-EVIDENCE.md. The previous full-suite/browser baseline
still predates these focused repairs; do not add focused counts to its totals.
