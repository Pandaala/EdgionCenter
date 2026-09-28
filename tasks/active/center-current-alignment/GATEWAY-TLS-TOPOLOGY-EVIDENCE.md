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
