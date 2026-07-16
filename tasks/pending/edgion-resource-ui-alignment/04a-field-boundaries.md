# Resource Field Boundaries

## Path notation and common envelope

- Paths are JSON paths relative to the API document. `[*]` matches every array
  entry, `*Plugins` matches each declared HTTP plugin stage, and `**` is a
  recursive descent used only for the named terminal fields in that row.
- Create accepts `apiVersion`, `kind`, `metadata.name`, namespaced
  `metadata.namespace`, `metadata.labels`, `metadata.annotations`, and the
  complete operator `spec` after internal-path removal.
- Update accepts the same paths. Identity (`name`, `namespace`, `kind`) must
  match the URL/current object. Labels and annotations are whole operator maps.
- The Controller preserves current `status` and these protected metadata paths:
  `metadata.uid`, `metadata.resourceVersion`, `metadata.generation`,
  `metadata.managedFields`, `metadata.creationTimestamp`,
  `metadata.deletionTimestamp`, `metadata.deletionGracePeriodSeconds`,
  `metadata.ownerReferences`, `metadata.finalizers`, and backend-specific
  concurrency state. There are no additional operator-visible protected fields:
  filesystem/etcd use the listed JSON metadata/status set, while Kubernetes uses
  `metadata.resourceVersion` as the sole replace concurrency token.
- The frontend always removes `status` and the protected metadata paths. It
  rejects a read redaction sentinel at every sensitive/internal path.

## Exact per-resource internal paths

An empty internal-path cell means the current serialized resource has no
internal `spec` field. The adapter still applies the common envelope rules.

| Resource | Internal paths removed from mutation | Sensitive paths |
|---|---|---|
| GatewayClass | none | none |
| EdgionGatewayConfig | none | none |
| Gateway | `spec.tls.backend.resolvedClientCertificate`; `spec.listeners[*].tls.secrets`; `spec.listeners[*].tls.resolvedFrontendCaSecrets` | the same resolved certificate/secret paths |
| HTTPRoute | `spec.resolvedHostnames`; `spec.resolvedListeners`; `spec.invalidRuleIndices`; `spec.resolvedRules`; `spec.delegationIssues`; `spec.rules[*].parsedTimeouts`; `spec.rules[*].parsedRetry`; `spec.rules[*].parsedForwardRawPath`; `spec.rules[*].parsedAllowNonIdempotentRetry`; `spec.rules[*].backendRefs[*].backendTlsPolicy`; `spec.rules[*].backendRefs[*].refDenied` | `spec.rules[*].backendRefs[*].backendTlsPolicy` |
| GRPCRoute | `spec.resolvedHostnames`; `spec.resolvedListeners`; `spec.invalidRuleIndices`; `spec.resolvedRules`; `spec.delegationIssues`; `spec.rules[*].parsedTimeouts`; `spec.rules[*].parsedRetry`; `spec.rules[*].parsedAllowNonIdempotentRetry`; `spec.rules[*].backendRefs[*].backendTlsPolicy`; `spec.rules[*].backendRefs[*].refDenied` | `spec.rules[*].backendRefs[*].backendTlsPolicy` |
| TCPRoute | `spec.resolvedListeners`; `spec.rules[*].backendRefs[*].refDenied` | none |
| UDPRoute | `spec.resolvedListeners`; `spec.rules[*].backendRefs[*].refDenied` | none |
| TLSRoute | `spec.resolvedListeners`; `spec.effectiveHostnames`; `spec.rules[*].backendRefs[*].refDenied` | none |
| Service | none | none |
| EndpointSlice | none | none |
| EdgionTls | `spec.clientAuth.caSecret`; `spec.secret`; `spec.resolvedListeners`; `spec.resolvedLogLabels` | `spec.clientAuth.caSecret`; `spec.secret` |
| ReferenceGrant | none | none |
| BackendTLSPolicy | `spec.resolvedCaCertificates`; `spec.resolvedClientCertificate`; `spec.useSystemCa` | resolved certificate paths |
| EdgionPlugins | Below each declared plugin-stage entry: terminal fields `refDenied`, `resolvedAuthHeader`, `resolvedPullSecret`, `resolvedUsers`, `resolvedKey`, `resolvedGroupsByIss`, `resolvedCredentials`, `resolvedCaSecrets`, `resolvedOidcClientSecret`, `resolvedSessionSecret`, `resolvedCaCertificates`, `resolvedClientCertificate`, `resolvedCredential`, `resolvedKeys`, `resolvedSecretValues`, `resolvedSecrets`, `valuesSet`, `wildcardPatterns`, `compiledRegex`, `compiledPatterns`, `compiledOriginsRegex`, `compiledTemplates`, `compiledTimingRegex`, `compiledTransformPatterns`, `originsCache`, `ipMatcher`, `allowMatcher`, `denyMatcher`, `intervalDuration`, `effectiveSlots`, `rateBytesPerSecond`, `requestTimeoutDuration` | every resolved credential/certificate/secret terminal in the internal list |
| EdgionStreamPlugins | Below `spec.plugins[*].config`: terminal fields `refDenied`, `ipMatcher`, `allowMatcher`, `denyMatcher`, `intervalDuration`, `effectiveSlots`; legacy flattened `ipSource`, `message`, `status` are rejected/migrated rather than silently retained | none |
| EdgionConfigData | none; `EdgionConfigDataRef.refDenied` is stripped only in consumer plugin/stream-plugin paths listed above | none |
| EdgionAcme | none; challenge tokens/key authorization are status-only and removed by the common `status` rule | none in operator spec |
| LinkSys | `spec.config.resolvedSecrets`; each Redis/Elasticsearch/etcd `auth.secret`; Kafka `sasl.password.secret`; each top-level variant TLS `resolvedCaCertificates`/`resolvedClientCertificate`; HTTPDNS `connection.tls` resolved certificate fields | all listed SecretSlot/TLS runtime material and redaction sentinels |
| EdgionBackendTrafficPolicy | none | none |
| ConfigMap | none | existing `data`/`binaryData` values are never loaded; create/replace starts from an empty write-only draft |
| Secret | none | `data`; `stringData`; all returned value-bearing fields |

Plugin terminal-name rules are applied only below the four declared HTTP plugin
stage arrays (`requestPlugins`, `upstreamResponseFilterPlugins`,
`upstreamResponseBodyFilterPlugins`, `upstreamResponsePlugins`) or the stream
`plugins[*].config` subtree; they cannot remove a same-named operator field in a
different resource. Every terminal is backed by a fixture copied from the
current Rust serde shape. A generated source audit extracts public fields with
`schemars(skip)`, `serde(skip)`, or a redaction serializer and fails until every
serialized field is mapped here; fields that are `serde(skip)` are recorded but
need no request stripping fixture because they cannot occur on the wire.

## Version projection

| Resource | Frontend mutation version | Controller/Kubernetes projection |
|---|---|---|
| TLSRoute | preserve input `v1` or accepted `v1alpha3` | writer uses the detected served GVK and existing conversion |
| BackendTLSPolicy | preserve input `v1` or accepted `v1alpha3` | writer uses the detected served GVK and existing conversion |
| ReferenceGrant | preserve input `v1` or accepted `v1beta1` | new writer projection rewrites canonical v1 request GVK to detected v1beta1 before create/replace; read conversion returns canonical operator shape |

ReferenceGrant alternate support is gated on create/get/update/delete tests with
discovery serving only `gateway.networking.k8s.io/v1beta1`.

## Fixture mapping

- `P-*-CURRENT` contains every operator path represented by current Rust/CRD.
- `P-*-INTERNAL` contains every internal/protected path in this document and
  asserts it is absent from the frontend request.
- Storage preservation fixtures start from `P-*-INTERNAL`, apply a narrow
  operator mutation, and assert current status/protected metadata survive while
  internal spec fields do not.
- `P-*-UNKNOWN` uses a named, Controller-accepted preserve-unknown location
  rather than a fabricated top-level spec property. Applicable paths are
  `Gateway.spec.listeners[*].tls.options`,
  `Gateway.spec.listeners[*].allowedRoutes.namespaces.selector`,
  `EdgionConfigData.spec.data.config` for the `Misc` variant, and the explicit
  flattened `unknownFields` capture in Stream connection-IP restriction rules.
  LinkSys Webhook compatibility `auth`, `requestMethod`, and `defaultHeaders`
  values are separate accepted-shim fixtures, not generic unknown-field proof.
  Other resources mark `P-UNKNOWN` not applicable rather than inventing a field.
