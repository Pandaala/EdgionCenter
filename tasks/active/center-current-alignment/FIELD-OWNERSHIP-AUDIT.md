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

## Checked: LinkSys provider mutation boundaries

Checkpoint: 2026-09-28, Center `5e1773e`. All eight current SystemConfig variants
were inventoried through their configuration structs and imported SecretSlot,
TlsConfig, ExternalHttpConnection and OutboundRequestSpec shapes. Existing
catalog exclusions cover the serialized internal material; no Center production
change is needed for this ownership pass.

| Provider | Internal material paths relative to spec.config |
| --- | --- |
| Redis, Etcd, Elasticsearch, OTLP | auth.secret; tls.resolvedCaCertificates / resolvedClientCertificate |
| Kafka | sasl.password.secret; tls.resolvedCaCertificates / resolvedClientCertificate |
| Webhook | resolvedSecrets; tls.resolvedCaCertificates / resolvedClientCertificate |
| HttpDns | connection.tls.resolvedCaCertificates / resolvedClientCertificate |
| CredentialSource | provider.tls.resolvedCaCertificates / resolvedClientCertificate (defensive exclusion; current Controller clears these before transport and keeps resolved material in its manager) |

The root currentStatus is excluded. unknownFields is serde(skip), and parsed
Redis/OTLP endpoints are separate runtime types. Redis/Etcd/OTLP auth wrappers
and Elasticsearch auth variants flatten SecretSlot, so the internal path is
auth.secret rather than auth.slot.secret. CredentialSource active/previous
bootstrap references are operator fields, not resolved credential slots.
Controller authority is handlers/link_sys.rs::drive_standard,
resolve_for_spec and resolve_credential_source.

37 tests pass across linksys, linksys-redis, linksys-credential-source and
resourceCatalog. Log: `/tmp/ws5-center-linksys-ownership-tests.log`. This checks
the mutation/configuration boundary; persistent-provider global TLS lifecycle
validation remains separate from this inventory.

## Checked: AI credentials, ACME, ConfigData and core envelopes

Checkpoint: 2026-09-28, Center `6cc1ca5`. No additional ownership-path repair was
needed in this pass. Current source was traced through nested types and the
mutation adapters, including variant-specific filtering outside the catalog.

- EdgionBackend: currentStatus and ai.credentialPool.credentials[].secret are
  serialized internal fields. AiCredential flattens SecretSlot; secretRef,
  alias, weight, limits, models/pricing, provider and defaults remain authored.
  reconcileOutcome is serde(skip); compiled model catalogs and computed pricing
  live in separate runtime types. The Controller clears and resolves every slot
  before producing its current-generation outcome.
- EdgionAcme: currentStatus and resolvedListenerAttachments are serialized
  internal fields. notifyAfterPublish is serde(skip). HTTP-01 ParentReferences,
  account/EAB Secret references, renewal durations, storage and automatic TLS
  settings remain authored. Active challenges and scheduler state belong to
  status, not the mutation spec. The old renewBeforeDays exclusion remains a
  stale-field removal, not a current runtime property.
- EdgionConfigData: currentStatus is excluded and unknownFields is serde(skip).
  All nine supported data variants were inventoried, including imported
  RegionDef, ProxyProtocolTrustPolicy, UrlAccessCandidate and ConditionSet.
  The type-specific adapter removes resolvedValues/resolvedCredentials/
  resolvedIps/refDenied under URL-list conditions while preserving Misc payloads.
  Condition matchers/descriptors are serde(skip). WAF bundle content is authored
  even though its processed-view serializer redacts it: it must not be stripped
  from a mutation. The ConfigData list/editor uses the storage-backed namespaced
  API through useResourceList; cache_list_resources is the separate redacted
  processed-status path.
- ReferenceGrant: from/to group, kind, source namespace and optional target name
  are authored; its current spec has no internal projection.
- Core envelopes: Service retains spec; EndpointSlice retains addressType,
  endpoints and ports; Secret retains data/stringData/type/immutable; ConfigMap
  retains data/binaryData/immutable. Shared mutation serialization excludes status
  and server metadata and retains resourceVersion for updates. Secret/ConfigMap
  remain restricted dependencies with explicit replacement flows; this pass
  does not authorize adding them to a global resource read model.

Authority: current shared edgion_backend.rs, edgion_acme.rs,
edgion_config_data/*, conditions.rs, proxy_protocol.rs and reference_grant.rs;
Controller handlers edgion_backend.rs / edgion_acme.rs and namespaced_handlers.rs;
Center resourceCatalog.ts, resource-document.ts and the per-resource adapters.
62 tests pass across edgionbackend, edgionacme, edgionConfigData, core-resources,
secret and resourceCatalog. Log:
`/tmp/ws5-center-remaining-resource-ownership-tests.log`. This is ownership and
adapter evidence, not new AI-provider traffic or ACME issuance evidence.

## HTTP plugin opaque-data preservation repair

Checkpoint: 2026-09-28, following `5d433a5`. Recursive terminal-name filtering
deleted authored Mock header names, Wasm plugin/vm JSON, ProxyRewrite JSON Patch
literal keys and Canary profile names when they matched names such as
resolvedSecrets. The regression failed for both create and update before repair
(`/tmp/ws5-plugin-opaque-field-regression.log`).

The catalog now scopes HTTP exclusions to known config fields, condition trees,
RequestAccessPolicy rule containers, TLS blocks, Wasm source resolution and
typed references. It does not recursively descend into arbitrary plugin payloads
or operator-named maps. WAF policy/selector reference denial markers are now
excluded at their exact paths as well. Authored data and source drafts remain
unchanged, while the Wasm pull-auth/TLS material and selector denial marker are
removed from mutation output.

49 tests across resourceCatalog, edgionplugins and edgionstreamplugins pass;
production build and lint pass. Logs:
`/tmp/ws5-plugin-scoped-fields-{tests,build,lint}.log`. The remaining imported-type
review must still verify the narrowed paths cover every serialized runtime field;
these tests alone do not close the full plugin ownership inventory.

### Body and dye condition follow-up

The first full frontend run after narrowing found three failures: two exposed
missing body-condition paths (dye conditions were also identified by source
inspection), and one asserted recursive deletion through an invented extAuth
entry shape absent from the current resource model. The body and both dye
directions now use explicit condition paths. The obsolete fixture was replaced
with the real AiGuard evaluator/TLS nesting and an authored extension sibling.
Four stage regressions cover dye conditions for create and update.

The final full frontend run passes all 846 tests in 117 files:
`/tmp/ws5-center-plugin-audit-final-full-tests.log`. Build and lint pass in
`/tmp/ws5-plugin-dye-fields-{build,lint}.log`. The first failed full run remains
at `/tmp/ws5-center-plugin-audit-full-tests.log`.

## Checked: HTTP and Stream plugin ownership inventory

Checkpoint: 2026-09-28, following `790f3c7`. Stream filtering had the same map-key
collision: a GlobalConnectionIpRestriction profile named allowMatcher or
refDenied was removed. It now uses explicit root, profile-member, rate-limit and
reference paths. Create/update regressions preserve both profile names and an
operator extension while removing the actual matcher and reference denial fields.
59 tests pass across resourceCatalog, resource-document and edgionstreamplugins;
build/lint pass. Logs: `/tmp/ws5-stream-scoped-fields-{tests,build,lint}.log`.

The inventory covers the current EdgionPlugin, EdgionStreamPlugin and
TlsRouteStreamPlugin enums, their configuration structs, all four HTTP entry
types, BodyRequirement, DyeSuite, ConditionSet/HmacMatchConfig, shared outbound
HTTP/TLS types and EdgionConfigDataRef. Serialized internal members group as:

| Owner | Internal fields and placement |
| --- | --- |
| HTTP auth configs | resolvedUsers, resolvedKey(s), resolvedCredential(s), resolvedGroupsByIss, resolvedCaSecrets, resolvedOidcClientSecret, resolvedSessionSecret; LDAP/OIDC TLS material on their config |
| HTTP external calls | ForwardAuth/Guardrail resolvedSecrets and shared tls material; AiGuard evaluator.resolvedSecrets/tls; ExtProc grpcService.tls |
| Wasm source | source.fetch.resolvedAuthHeader/tls and source.oci.resolvedPullSecret |
| Conditions | resolvedValues, resolvedIps, HMAC resolvedCredentials and reference refDenied under entry/body/dye conditions and RequestRestriction/RequestAccessPolicy conditions |
| RequestAccessPolicy | config.resolvedProfiles, rule config.resolvedCandidates, direct IP expansion and typed IP/URL/selector reference denial markers |
| Other typed references | HTTP/Stream allowRefs/denyRefs, selectors/overrides, stream profile references and RequestMirror backendRef.refDenied |
| Logical WAF | resolvedPolicy, selectedProfile, resolvedRefIndices, resolvedBundles, resolutionErrors, ownerNamespace and policy/selector reference denial markers |

The root currentStatus is excluded. Compiled regex/IP/GeoIP matchers, parsed
durations, body caches, unknown-field diagnostics and WAF status-only flags use
serde(skip) or separate runtime types. Defensive exclusions for these names do
not establish that they are transported. Arbitrary Mock headers, Wasm JSON,
JsonSchemaValidation schema, ProxyRewrite literals and operator-chosen map keys
are configuration, not runtime state. Stream entries have enable plus the tagged
config; they do not inherit HTTP entry/body/dye conditions. GeoIpLocation shares
typed rule groups with HTTP, with a serde-skipped CIDR matcher.

Controller ownership was cross-checked against edgion_plugins.rs resolution of
credentials, WAF, access profiles and condition references, and
edgion_stream_plugins.rs reference-marker recomputation. The mutation ownership
pass now covers all catalog kinds. This closes that bounded source/adapter
review only. The later final reconciliation of broader runtime and menu
requirements is recorded in COMPLETION-AUDIT.md.
