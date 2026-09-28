---
name: system-resources
description: System configuration resource development guide — EdgionGatewayConfig/LinkSys/EdgionAcme
---

# System Configuration Resources

## EdgionGatewayConfig

- Cluster-scoped `edgion.io/v1alpha1`, catalog key `edgiongatewayconfig`.
  GatewayClass.spec.parametersRef selects the configuration. Do not impose a
  singleton: separate GatewayClasses may select separate configurations.
- Canonical schema: sibling
  `Edgion/edgion-resources/src/resources/edgion_gateway_config.rs`.
- Center implementation: `src/types/edgion-gateway-config/index.ts`,
  `src/utils/edgiongatewayconfig.ts`, and
  `src/components/ResourceEditor/EdgionGatewayConfig/`.

### Current operator fields

| Field | Editor contract |
| --- | --- |
| server | enableCompression and downstreamKeepaliveRequestLimit only; process threads/shutdown/pool settings are not resource fields |
| httpTimeout | client read/write/keepalive and backend connect/request/idle duration strings |
| retry.attempts | Integer 0..2147483647; replaces the removed root maxRetries |
| forwardedHeaders.remoteIpHeader | Outbound header name; validate token syntax and reserved-header restrictions |
| requestBody | enabled, defaultMemoryBufferSize, maxMemoryBufferSize, defaultMaxBodySize, maxBodySize, storageOperationTimeout |
| pluginPolicy | Qualified plugin allow/deny entries, defaultAction, deniedAction, blockStatus; absent allow differs from an empty allow list |
| realIp | trustedIps is a list of named groups with cidrs, not a string array; realIpHeader, recursive, optional maxTrustedHops |
| securityProtect | xForwardedForLimit, requireSniHostMatch, fallbackSni, tlsProxyLogRecord, allowLoopbackUpstream |
| tcpTimeout | idleTimeout and connectTimeout |
| loadBalancing | degradeThreshold |
| globalPluginsRef | Namespaced plugin references |
| accessLogExtern.unmaskedKeys | header, respHeader, query, cookie and ctx arrays |
| preflightPolicy | cors-standard or all-options, statusCode 200..599 |
| linkSys | webhookMaxResponseBytes and maxInstancesPerKind (1..10000) |
| outboundTls | verify, validation and clientCertificateRef; preserve reference group/kind/namespace |
| dnsResolver | linkSysRef, servers and cacheTtl |

### Editing and validation

- Defaults belong only in create drafts/placeholders. Preserve explicit zero,
  false, empty lists, and unknown operator fields when reading or editing.
- Do not restore removed maxRetries, process-level server controls,
  rejectDuplicateHost or enableReferenceGrantValidation from historical examples.
- RealIp requires at least one trusted group when configured; its inbound header
  semantics differ from forwardedHeaders, which writes an outbound header.
- Outbound TLS resolved CA/client-certificate material and currentStatus are
  Controller-owned and must be stripped by the mutation boundary. Custom CA
  references may coexist with System in this global policy; do not copy the
  BackendTLSPolicy mutually-exclusive rule into this resource.
- Form and YAML submit share the adapter's validation. Native CRUD establishes
  persistence and round-trip behavior, not DNS resolution, TLS verification,
  plugin execution or request-body behavior at the Gateway.

## LinkSys

- Namespaced `edgion.io/v1`, catalog key `linksys`; uses the flat tagged envelope
  `spec: {type: redis, config: {...}}`. The type's fields live under config,
  never under a repeated provider name.
- Current upstream also includes OTLP/gRPC and credentialSource. The dashboard
  implements all eight: redis, elasticsearch, etcd, webhook, kafka, httpdns, otlp,
  and credentialSource. Provider-specific runtime workflows remain separate checks.
- OTLP config fields: endpoint, timeoutMs (1..300000, default 10000), optional
  auth.secretRef, and optional tls. Endpoints are HTTP(S) origins; no URI
  credentials, path, query, or fragment. TLS enabled gates local policy; HTTPS
  still selects encrypted transport when local policy is disabled.
- Enabled OTLP TLS requires HTTPS, verification, and core Secret CA/client
  references; hostname and nonempty subjectAltNames overrides are unsupported.
- Auth and TLS fields reference Secrets. Resolved credential/certificate material
  is Controller-owned and excluded from mutation payloads by the shared catalog.
- Preserve inactive TLS configuration and provider drafts while editing; only
  validate enabled local TLS policy. List summaries include OTLP endpoint.

### Redis and Kafka

- Redis timeout.connect and timeout.command are positive GEP-2257 strings,
  bounded at 10s and 30s. Database is 0..255, pool size is 1..64, and cluster
  maxRedirects is 0..64. Removed retry/observability, timeout.read/write,
  pool.minIdle, and readFromReplicas are not editable controls.
- Redis standalone requires exactly one endpoint; cluster requires one or more
  and its cluster settings may be absent. Sentinel requires only its nested
  sentinels and masterName, with no top-level endpoints or cluster settings.
  Switching topology clears conflicting known fields; other config survives.
- Kafka exposes channelSize, maxTopics, maxPendingRecords, maxPendingBytes, and
  lingerMs. Capacities are positive integers; zero linger is valid. Defaults
  appear as placeholders rather than being injected into existing documents.

### Credential source

- Provider shape is flat: `provider: {type: oauth2ClientCredentials, tokenEndpoint,
  clientAuthentication, scopes, tls}`. Only clientSecretBasic is supported.
- Bootstrap credentials remain Secret references (activeSecretRef and optional
  previousSecretRef). The Controller owns acquisition and rotation; the Gateway
  never contacts the issuer, and Center never fetches credential values.
- Rotation fields use bounded GEP-2257 durations. Egress blockPrivate and
  publication persist default true; memoryMaxKeys defaults 10000. Preserve false
  and omitted values while editing.
- Provider TLS is nested at config.provider.tls; exclude its resolved CA/client
  certificate fields at the same mutation boundary as ordinary LinkSys TLS.

## EdgionAcme

- Namespaced `edgion.io/v1`, catalog key `edgionacme`, System → ACME.
- Current schema: sibling `Edgion/edgion-resources/src/resources/edgion_acme.rs`.
  Built-in issuance supports HTTP-01 only. DNS-01 and wildcard certificates require
  an external issuer whose TLS Secret is referenced by EdgionTls.
- Challenge shape is flat: `challenge: {type: http-01, gatewayRef: {name: gateway}}`.
  Preserve Gateway reference namespace, sectionName, port, group, and kind.
- Account credentials use privateKeySecretRef; optional externalAccountBinding uses
  keyId plus keySecretRef. Never expose resolved account or HMAC key material.
- Renewal uses GEP-2257 strings: renewBefore (720h), checkInterval (24h),
  failBackoff (5m). Do not materialize defaults while editing existing resources.
- Keep storage secretName/secretNamespace and autoEdgionTls enabled/name/parentRefs.
- Draft serialization must allow incomplete forms; submission validates email,
  domains, HTTP-01 scope, and renewal durations before sending through the tunnel.
- Strip status, currentStatus, resolvedListenerAttachments, notifyAfterPublish, and
  server-owned metadata on mutation. Preserve resourceVersion for update CAS.
- List displays phase, certificate expiry in UTC, domains, challenge type, and
  conditions. `AcmeLifecycle` shares `ResourceStatus` identity/version checks and
  processed fallback; it preserves source status and hides cached lifecycle on
  failed reads. Missing/invalid expiry stays absent. Pending is not evidence that
  the issuer is running. Native UI checks are separate from real issuance proof.
- Manual trigger uses `AcmeTriggerButton` and the dedicated `acme.trigger`
  operation, not the resource update permission. Its HTTP request captures the
  page Controller: `POST /api/v1/services/acme/{namespace}/{name}/trigger`.
  A successful response means a check was queued, never that issuance completed.
  Do not replay uncertain requests or expose raw upstream errors. Disable repeat
  clicks while pending. The actual service runs only on a Kubernetes leader;
  native FS validation does not issue certificates.
