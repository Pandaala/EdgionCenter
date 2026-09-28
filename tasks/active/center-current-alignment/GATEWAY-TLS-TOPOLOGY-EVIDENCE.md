# Gateway TLS dependency topology

Date: 2026-09-28. Overall alignment remains active.

Current Gateway Controller processing resolves the default backend client
certificate from spec.tls.backend.clientCertificateRef and projects frontend
validation from spec.tls.frontend.default/perPort into HTTPS listeners. The
frontend resolver accepts exact empty core group and Secret/ConfigMap kinds;
backend identity resolves a core Secret. Cross-namespace references participate
in the existing ReferenceGrant checks.

The Center topology previously represented only listener serving certificates.
It now adds explicit Gateway-level backend client-certificate, frontend default
CA, and per-port CA dependencies. Frontend references missing required kind/group
or using foreign groups/unsupported kinds remain unknown. Backend identity cannot
become a Service/ConfigMap edge. Namespace defaults use the owning Gateway, and
existing grant projection applies to concrete cross-namespace edges.

Only operator reference paths are inspected. Listener frontendValidation and
resolved certificate material are not traversed. Query functions are unchanged:
Secret and ConfigMap use listKeys, not list/get value access. Edges establish
configured dependencies and existence, not effective listener selection, trusted
certificate contents, or handshake success.

Verification:

- 39 topology graph tests pass, including seven new cases for Gateway-level
  relationships, cross-namespace grant projection, namespace-filter retention,
  private-field exclusion, missing/unavailable distinction and malformed refs.
- Production build and ESLint pass.
- Logs: /tmp/ws5-center-gateway-tls-topology-{tests,build,lint}.log.
- No new browser, backend or handshake claim. Edgion inspected without edits.


## Serving-certificate identity follow-up

Gateway listener serving references accept only Secret with omitted/empty group;
Controller certificate resolution does not accept the core alias here. Gateway
backend client identity uses is_core_api_group and does accept core. The generic
reference parser previously conflated these boundaries, making an invalid serving
ConfigMap/EdgionTls or core-group Secret look resolved when a same-named object
existed. Serving references now remain unknown for those invalid identities.

43 topology tests pass, including three invalid serving identity cases with
same-named target objects and a positive case proving omitted/explicit serving
Secret defaults and the backend core alias. Build and lint pass; logs are
/tmp/ws5-center-serving-ref-topology-{tests,build,lint}.log. No new native browser
or handshake claim is made. Optional namespace/kind form clearing was inspected
and already omits those fields, preserving Controller default semantics.

Corrected stale adapter guidance in web/skills/02-patterns/03-types-and-utils.md:
frontendValidation is a private projection, not operator input. The ownership
rule still requires checking ingestion rather than inferring from schemars alone.
