# BackendTLSPolicy client certificate option

Date: 2026-09-28

Current source authority: `../Edgion/edgion-resources/src/resources/backend_tls_policy.rs`,
`parse_client_certificate_option` and `valid_kube_name`. An absent option disables
this optional reference; an empty or malformed present value is invalid. Parsing
trims the value before checking a 253-byte name and nonempty, at most 63-byte ASCII
lowercase alphanumeric/hyphen labels with alphanumeric edges.

The Center form previously wrote an empty string when cleared. Its truthiness-based
preflight allowed that value and malformed dotted names. Clearing now deletes only
the client certificate key and omits an empty options map. Preflight checks present
values, including non-string YAML values, using the current Controller name rules.
Accepted strings remain unchanged in the mutation document.

Verification:

- 35 focused tests pass across validator and actual editor submission tests.
- Production build (including TypeScript) and ESLint pass.
- Form tests import YAML, switch to Form, clear the field, and inspect the API
  mutation payload. They cover both no remaining options and an unrelated empty
  option, and preserve the trust configuration.
- Validator tests cover empty values, malformed labels, namespace-qualified names,
  label and overall length boundaries, wrong YAML types, and whitespace trimming.
- Logs: `/tmp/ws5-center-client-cert-tests.log`,
  `/tmp/ws5-center-client-cert-build.log`, `/tmp/ws5-center-client-cert-lint.log`.

This is configuration/editor evidence. No new native browser or TLS handshake
claim is made. Edgion was inspected without modification in this follow-up.
