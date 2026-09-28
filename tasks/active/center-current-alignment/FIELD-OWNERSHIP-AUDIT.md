# Current nested field ownership audit

Checkpoint: 2026-09-28, Center source `572cfd8`. This closes specific mutation
ownership reviews, not the complete resource/menu alignment or runtime behavior.
Only current sibling source was inspected; no Edgion history was read.

## Checked: EdgionTls, BackendTLSPolicy, EdgionBackendTrafficPolicy

No additional production repair was required for these three mutation boundaries.
The source inventory and the existing catalog exclusions agree for every serialized
runtime field identified in the root specs and nested configuration types below.

| Resource | Serialized runtime paths, relative to spec | Operator configuration retained |
| --- | --- | --- |
| EdgionTls | currentStatus, secret, clientAuth.caSecret, resolvedListeners, resolvedListenerAttachments | parentRefs, hosts, secretRef, clientAuth mode/reference/depth/SAN/CN constraints, minTlsVersion, ciphers |
| BackendTLSPolicy | currentStatus, resolvedTargetClass, resolvedCaCertificates, resolvedClientCertificate, useSystemCa | targetRefs, validation hostname/CA/SAN/system trust, options |
| EdgionBackendTrafficPolicy | currentStatus, resolvedTargetClass, healthCheck.active.resolvedCaCertificates, healthCheck.active.resolvedClientCertificate, healthCheck.active.resolvedTlsError | targets, loadBalancer/hash/degradeThreshold, active probe configuration including active.tls, outlierDetection durations/thresholds, upstreamAuthority, retryConstraint, circuitBreaker, connection |

### Non-serialized caches are not additional transport fields

- EdgionTls.resolvedListenerAttachmentIndex and resolvedLogLabels are serde(skip)
  Gateway-local caches; AllowedSan.compiledRegex is also serde(skip). They cannot
  arrive in an ordinary Controller JSON response. The existing resolvedLogLabels
  exclusion is conservative and is not evidence that it is transported.
- OutlierDetectionResolved is a separate non-Serialize type. Its ejectionSeconds
  and maxEjectionSeconds are computed from operator ejectionTime/maxEjectionTime;
  do not expose seconds as editable policy fields. Existing defensive exclusions
  for the seconds names do not change the operator duration configuration.
- Load-balancer parsed state and compiled upstream-authority templates are separate
  runtime types, not nested serialized fields of the policy spec.
- OutboundTlsLocal (the health probe active.tls shape) has verify, validation and
  clientCertificateRef only. Resolved probe certificate material belongs on
  active, not on active.tls. OutboundTlsConfig is a different type used for global
  defaults and must be reviewed in its own owning resource.

### Authority inspected

All paths below are relative to ../Edgion/edgion-resources/src/resources/ unless
otherwise stated:

- edgion_tls.rs: ClientAuthConfig and EdgionTlsSpec; allowed_san.rs: AllowedSan;
  common/mod.rs: ParentReference; common/secret_object_ref.rs: SecretObjectReference.
- backend_tls_policy.rs: BackendTLSPolicySpec, target/CA references, validation/SAN
  types and implementation-specific client certificate parsing.
- edgion_backend_traffic_policy.rs: root spec, target/load-balancer/hash, retry
  budget/rate and connection override types.
- health_check.rs: ServiceHealthCheck/ActiveHealthCheckConfig;
  common/outbound_tls.rs: OutboundTlsLocal and validation leaf types;
  outlier_detection.rs, circuit_breaker.rs, upstream_authority.rs and lb_policy.rs.
- Controller handlers edgion_tls.rs::parse, backend_tls_policy.rs::parse and
  edgion_backend_traffic_policy.rs::parse/resolve_health_check_tls clear or recompute
  these resolved values. The shared status_pipeline.rs owns currentStatus capture.
- Center web/src/config/resourceCatalog.ts holds the exact mutation paths;
  resource-document.ts applies them while retaining unrelated operator fields.

### Verification

85 existing tests pass across edgiontls, backendtlspolicy,
edgionbackendtrafficpolicy and resourceCatalog suites. They cover lossless
operator documents, runtime stripping, typed SAN/CA/attachment data, health TLS
references and redacted outcomes, plus target/status ownership.
Log: /tmp/ws5-center-tls-policy-ownership-tests.log.
No test was added merely to duplicate the exclusion list. No new build, browser,
traffic or handshake claim is made by this read-only source audit.

## Checked: Gateway, GatewayClass, EdgionGatewayConfig

Checkpoint: 2026-09-28, Center source `6293eab`. No additional mutation-boundary
repair is required by this current-source inventory.

| Resource | Serialized internal paths, relative to spec | Operator configuration retained |
| --- | --- | --- |
| GatewayClass | currentStatus | controllerName, description, parametersRef group/kind/name/namespace |
| Gateway | currentStatus, resolvedInboundProxyProtocol, resolvedAttachmentProof, tls.backend.resolvedClientCertificate, listeners[].tls.frontendValidation, listeners[].tls.resolvedCertificateRefs, listeners[].tls.resolvedFrontendCaRefs, listeners[].tls.frontendMatcherEligibility | listeners, allowedRoutes namespace selectors/kinds, addresses, serving certificate references/options, Gateway-level backend identity and frontend default/per-port validation |
| EdgionGatewayConfig | currentStatus, outboundTls.resolvedCaCertificates, outboundTls.resolvedClientCertificate | server/timeouts/retry, realIp/trustedIps, forwardedHeaders, securityProtect, requestBody, pluginPolicy/globalPluginsRef, accessLogExtern, preflightPolicy, linkSys limits, loadBalancing, outboundTls references/validation, dnsResolver |

The three root definitions and all configuration structs declared in
`edgion_gateway_config.rs` were inspected, including imported `RealIpConfig`,
`IpGroup`, `PluginPolicyConfig`, `LinkSysRef`, and `OutboundTlsConfig` with its
validation/reference/SAN leaf types. Compiled plugin policies, parsed duration
and request-body limits, and RealIp matchers live in separate runtime types;
they are not additional serialized configuration fields to strip.

Gateway's `allowedListeners`, `infrastructure`, and `defaultScope` illustrate why
schemars(skip) does not alone establish runtime ownership: the root explicitly
retains these unsupported upstream inputs for validation/status before removing
them from conf_sync. Center preserves the draft and reports unsupported fields
in `validateGateway`, rather than silently making the submitted intent disappear.
The existing `enableReferenceGrantValidation` exclusion is removal of an obsolete
field, not a current serialized runtime member.

Controller authority: `handlers/gateway.rs` resets listener runtime values before
`project_standard_frontend_validation`, resolves backend identity and inbound
PROXY policy, and builds the attachment proof. GatewayClass has no nested
resolution fields. `handlers/edgion_gateway_config.rs::parse` clears and resolves
outbound CA material; its inspected body does not resolve the client identity.
The latter remains a runtime behavior boundary, not proof that global outbound
mTLS works. Both resolved secret fields are schema-hidden/redacted in the shared
`OutboundTlsConfig` and must remain excluded from Center mutations.

58 existing tests pass across gateway, gatewayclass, edgiongatewayconfig and
resourceCatalog suites. These verify mutation filtering, operator/unknown-field
preservation, unsupported-input validation and current configuration constraints.
Log: `/tmp/ws5-center-gateway-ownership-tests.log`. No new browser or traffic claim
is made by this ownership check.

## HTTPRoute mirror authorization projection repair

Checkpoint: 2026-09-28, following Center `b9210ad`. The current Controller
recomputes `requestMirror.backendRef.refDenied` in both rule filters and backend
filters (`handlers/request_mirror.rs::resolve_rule`). The shared
`BackendObjectReference` serializes this authorization result despite hiding it
from the schema. Center previously excluded ordinary backend authorization
results but retained the nested mirror results in mutation documents.

The HTTPRoute catalog now excludes both exact mirror paths. Create and update
regressions exercise `toHTTPRouteMutationDocument`, retaining the mirror target,
namespace, port, percentage and an unrelated nested operator `refDenied` field.
The original editor draft remains unchanged.

Verification: 40 tests pass across httproute and resourceCatalog; production
build and lint pass. Logs: `/tmp/ws5-center-mirror-ownership-tests.log`,
`/tmp/ws5-center-mirror-ownership-build.log`, and
`/tmp/ws5-center-mirror-ownership-lint.log`. This is mutation-boundary evidence;
the full nested route inventory and runtime mirroring audit remain open.

## Checked: five route mutation boundaries

Checkpoint: 2026-09-28, following Center `fb9e73e`. The nested current-source
inventory identified one further omission: HTTPRoute ExternalAuth embeds
`ForwardAuthConfig`, whose `resolvedSecrets` and flattened connection's
`tls.resolvedCaCertificates` / `tls.resolvedClientCertificate` are internal,
serialized and redacted. Center now excludes those exact paths in both rule and
backend filters. Create/update regressions retain the TLS references, request
template and decision, preserve an unrelated nested `resolvedSecrets` field,
and prove the original draft is unchanged. This establishes mutation ownership,
not successful inline ExternalAuth secret resolution or authenticated traffic.

| Resource | Serialized internal paths, relative to spec |
| --- | --- |
| HTTPRoute | currentStatus, resolvedStatusController, resolvedHostnames, resolvedListeners, invalidRuleIndices, resolvedRules; rules[].resolvedAiAdmission/resolvedTerminalRouteUid/resolvedTerminalRuleIdentity; backendRefs[].refDenied; rule/backend filter extensionRef.resolvedNamespace, mirror backendRef.refDenied and ExternalAuth resolved material |
| GRPCRoute | currentStatus, resolvedStatusController, resolvedHostnames, resolvedListeners, invalidRuleIndices, resolvedRules; backendRefs[].refDenied and rule/backend filter extensionRef.resolvedNamespace |
| TCPRoute / UDPRoute | currentStatus, resolvedStatusController, resolvedListeners, resolvedListenerAttachments; backendRefs[].refDenied |
| TLSRoute | currentStatus, resolvedStatusController, resolvedListeners (including per-listener hostname intersections); backendRefs[].refDenied |

Source inventory: the five `edgion-resources/src/resources/*_route.rs` specs,
rules, matches, backend references and filter types; shared ParentReference;
HTTPRoute header/redirect/rewrite/CORS/mirror/extension/timeout/retry/session
persistence types; ForwardAuthConfig, ExternalHttpConnection, TlsConfig and
OutboundRequestSpec. The five Controller route handlers own attachment and
authorization projections; request_mirror.rs owns mirror authorization.
Parsed timeouts/retry/mirror tuning, delegation diagnostics and backend TLS
policy caches use serde(skip), so they are not additional transported fields.
The upstream ExternalAuth raw object remains unsupported and must not be
mistaken for a supported Edgion ForwardAuth configuration.

63 tests pass across httproute, routes-roundtrip, grpcroute.filters and
resourceCatalog. Production build and lint pass. Logs:
`/tmp/ws5-center-route-ownership-{tests,build,lint}.log`. These checks retain
operator references, namespace/port/weight, filters and unknown extensions;
they do not close the remaining runtime traffic matrix.

## Still to reconcile in the dedicated ownership pass

HTTP/stream plugin configurations and conditions; LinkSys
variants; AI credential slots; ACME/ConfigData; Kubernetes core resource envelopes.
Earlier repairs and tests remain evidence, but each nested ownership review needs
an explicit current-source inventory before this audit can be closed.
