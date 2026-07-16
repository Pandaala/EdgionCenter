# Resource Contract and Coverage Ledger

## Ledger rules

- Operator fields are the structural CRD `spec` plus operator metadata. For
  Gateway API resources without a repository-owned CRD, use the corresponding
  Rust resource structure after excluding `schemars(skip)` fields.
- Server fields are `status` and Kubernetes-generated metadata.
- Internal fields are every `schemars(skip)`, parsed, compiled, resolved,
  redacted, or Controller-only marker path, including nested reference fields.
- Exact JSON paths and wildcard semantics are normative in
  `04a-field-boundaries.md`; this table is a readable summary only.
- `R` means readable by the built-in Center policy, `W` means writable by that
  policy, and `D` means denied. Explicit Controller policy may replace these.
- Generic Controller CRUD handler support is separate from federation access.

## Sources

All `Edgion/...` paths are repository-relative and resolve in the isolated
companion worktree `../Edgion-resource-ui`, not the user's original checkout.

| Code | Source |
|---|---|
| `DEF` | `Edgion/edgion-resources/src/resource/defs.rs` |
| `KIND` | `Edgion/edgion-resources/src/resource/kind.rs` |
| `POL` | `Edgion/edgion-controller/src/fed_sync/default_policy.rs` |
| `NSAPI` | `Edgion/edgion-controller/src/api/namespaced_handlers.rs` |
| `CAPI` | `Edgion/edgion-controller/src/api/cluster_handlers.rs` |
| `R/<file>` | `Edgion/edgion-resources/src/resources/<file>` |
| `CRD/<file>` | `Edgion/config/crd/edgion-crd/<file>` |

## Resource ledger

All rows also use `DEF`, `KIND`, and `POL`.

| Resource | Canonical API; accepted alternate | Authority | Scope/API operations | Operator/internal boundary | Status | Default access | Fixture/scenario families |
|---|---|---|---|---|---|---|---|
| GatewayClass | `gateway.networking.k8s.io/v1`; none | `R/gateway_class.rs` | cluster; generic CRUD | operator `controllerName/description/parametersRef`; no internal spec fields | conditions/supportedFeatures read-only | R, no W | `P-gatewayclass-*`, `CRUD/STATUS/AUTH-gatewayclass` |
| EdgionGatewayConfig | `edgion.io/v1alpha1`; none | `R/edgion_gateway_config.rs`, `CRD/edgion_gateway_config_crd.yaml` | cluster; generic CRUD | structural spec; strip any future runtime-only serde fields identified by CRD diff | no current status contract | R, no W | `P-edgiongatewayconfig-*`, `CRUD/AUTH-edgiongatewayconfig` |
| Gateway | `gateway.networking.k8s.io/v1`; none | `R/gateway.rs` | namespaced; generic CRUD | structural spec; nested resolved/denial/runtime paths stripped by adapter | listener conditions/addresses read-only | R, no W | `P-gateway-*`, `CRUD/STATUS/REL/AUTH-gateway` |
| HTTPRoute | `gateway.networking.k8s.io/v1`; none | `R/http_route.rs` | namespaced; generic CRUD | operator rules; strip `resolvedRules`, `refDenied`, `parsed*`, resolved TLS and Controller copies | parent conditions read-only | R, no W | `P-httproute-*`, `CRUD/STATUS/REL/AUTH-httproute` |
| GRPCRoute | `gateway.networking.k8s.io/v1`; none | `R/grpc_route.rs` | namespaced; generic CRUD | operator rules; strip `resolvedRules`, `refDenied`, `parsed*`, resolved backend fields | parent conditions read-only | R, no W | `P-grpcroute-*`, `CRUD/STATUS/REL/AUTH-grpcroute` |
| TCPRoute | `gateway.networking.k8s.io/v1alpha2`; none | `R/tcp_route.rs` | namespaced; generic CRUD | operator parentRefs/rules; strip resolved/denial/runtime reference fields | parent conditions read-only | R, no W | `P-tcproute-*`, `CRUD/STATUS/AUTH-tcproute` |
| UDPRoute | `gateway.networking.k8s.io/v1alpha2`; none | `R/udp_route.rs` | namespaced; generic CRUD | operator parentRefs/rules; strip resolved/denial/runtime reference fields | parent conditions read-only | R, no W | `P-udproute-*`, `CRUD/STATUS/AUTH-udproute` |
| TLSRoute | `gateway.networking.k8s.io/v1`; `v1alpha3` accepted/converts | `R/tls_route.rs` | namespaced; generic CRUD | operator parentRefs/hostnames/rules; strip `resolvedListeners`, `effectiveHostnames`, denial/runtime reference fields | parent conditions read-only | R, no W | `P-tlsroute-*`, including `ALT-v1alpha3`; `CRUD/STATUS/AUTH` |
| Service | `v1`; none | Kubernetes core type, `resource/meta/impls.rs` | namespaced; generic CRUD | Kubernetes operator metadata/spec; runtime backend state is not part of mutation | none | R, no W | `P-service-*`, `CRUD/REL/AUTH-service` |
| EndpointSlice | `discovery.k8s.io/v1`; none | Kubernetes discovery type, `resource/meta/impls.rs` | namespaced; generic CRUD | Kubernetes operator metadata/addressType/endpoints/ports; strip runtime resolution | none | R, no W | `P-endpointslice-*`, `CRUD/REL/AUTH-endpointslice` |
| EdgionTls | `edgion.io/v1`; none | `R/edgion_tls.rs`, `CRD/edgion_tls_crd.yaml` | namespaced; generic CRUD | structural parent/host/secret/clientAuth/TLS fields; strip resolved Secret material/denials | conditions read-only | R, no W | `P-edgiontls-*`, `CRUD/STATUS/REL/AUTH-edgiontls` |
| ReferenceGrant | `gateway.networking.k8s.io/v1`; `v1beta1` accepted/converts | `R/reference_grant.rs` | namespaced; generic CRUD | `from`/`to` operator policy only | none | R, no W | `P-referencegrant-*`, including `ALT-v1beta1`; `CRUD/REL/AUTH` |
| BackendTLSPolicy | `gateway.networking.k8s.io/v1`; `v1alpha3` accepted/converts | `R/backend_tls_policy.rs` | namespaced; generic CRUD | operator targets/validation/options; strip resolved CA/client certificate, `useSystemCa`, denial/runtime fields | ancestor conditions read-only | R, no W | `P-backendtlspolicy-*`, including `ALT-v1alpha3`; `CRUD/STATUS/REL/AUTH` |
| EdgionPlugins | `edgion.io/v1`; none | `R/edgion_plugins/*`, `CRD/edgion_plugins_crd.yaml` | namespaced; generic CRUD | structural stages/entries; strip resolved credentials, compiled configs, denial markers and redaction sentinels | conditions read-only | R, no W | `P-edgionplugins-*`, `CRUD/STATUS/REL/AUTH-edgionplugins` |
| EdgionStreamPlugins | `edgion.io/v1`; none | `R/edgion_stream_plugins/*`, `CRD/edgion_stream_plugins_crd.yaml` | namespaced; generic CRUD | structural Stage 1/2 entries; strip resolved ConfigData/LinkSys/denial fields | conditions read-only | R, no W | `P-edgionstreamplugins-*`, `CRUD/STATUS/REL/AUTH` |
| EdgionConfigData | `edgion.io/v1`; none | `R/edgion_config_data/*`, `CRD/edgion_config_data_crd.yaml` | namespaced; generic CRUD | envelope and tagged data are operator fields; `refDenied` is internal wherever a ref is embedded | conditions read-only | R and W | `P-edgionconfigdata-*`, `CRUD/STATUS/REL/AUTH` |
| EdgionAcme | `edgion.io/v1`; none | `R/edgion_acme.rs`, `CRD/edgion_acme_crd.yaml` | namespaced; CRUD plus trigger | structural account/challenge/cert/autoTLS fields; strip resolved provider/credential material | conditions and certificate expiry read-only | R, no W | `P-edgionacme-*`, `CRUD/STATUS/REL/AUTH/OP-edgionacme` |
| LinkSys | `edgion.io/v1`; none | `R/link_sys/*`, `CRD/link_sys_crd.yaml` | namespaced; generic CRUD | structural tagged config; strip clients, health/runtime state and resolved secrets | conditions read-only | R, no W | `P-linksys-*`, `CRUD/STATUS/REL/AUTH-linksys` |
| EdgionBackendTrafficPolicy | `edgion.io/v1`; none | `R/edgion_backend_traffic_policy.rs`, `CRD/edgion_backend_traffic_policy_crd.yaml` | namespaced; generic CRUD | targets/LB/health/outlier/authority operator fields; strip resolved targets/runtime compiled policy | conditions read-only | R, no W | `P-edgionbackendtrafficpolicy-*`, `CRUD/STATUS/REL/AUTH` |
| Secret | `v1`; none | Kubernetes core model, `NSAPI` | restricted namespaced dependency; UI action inventory is exactly `list-keys/create/update`; value view, delete, and batch delete are deliberately hidden even if a Controller advertises generic delete | write-only/redacted data plus operator metadata; never preserve redaction sentinels as values | none | D | `P-secret-INTERNAL`, typed dependency `AUTH/REL` only |
| ConfigMap | `v1`; none | Kubernetes core model, `NSAPI` | restricted namespaced dependency; UI action inventory is exactly `list-keys/create/update`; value view, delete, and batch delete are a deliberate restricted-dependency safety exception | controlled replacement keys/metadata; existing values are never loaded into the editor | none | R, no W | `P-configmap-*`, typed dependency `CRUD/AUTH/REL` |

## Tagged-union cardinality gates

### EdgionPlugins — 39 current variants

`RequestHeaderModifier`, `ResponseHeaderModifier`, `RequestRedirect`,
`UrlRewrite`, `RequestMirror`, `BasicAuth`, `Cors`, `Csrf`, `IpRestriction`,
`JwtAuth`, `JweDecrypt`, `HmacAuth`, `HeaderCertAuth`, `KeyAuth`, `LdapAuth`,
`Mock`, `FaultInjection`, `DebugAccessLogToHeader`, `ProxyRewrite`,
`RequestRestriction`, `ResponseRewrite`, `RateLimit`, `RateLimitRedis`,
`CtxSet`, `RealIp`, `ForwardAuth`, `OpenidConnect`, `BandwidthLimit`,
`DirectEndpoint`, `AllEndpointStatus`, `DynamicInternalUpstream`,
`DynamicExternalUpstream`, `Dsl`, `RegionRoute`, `TraceContext`, `ExtProc`,
`GlobalAccessControl`, `Canary`, and `Wasm`.

The nested `ExtensionRef` variant does not exist and must not be offered.

### EdgionStreamPlugins

- Stage 1: `IpRestriction`, `GlobalConnectionIpRestriction`,
  `ConnectionRateLimit`.
- Stage 2: `IpRestriction` only.
- Serialized entry shape: flattened `enable + type + config`.

### EdgionConfigData

`KeyList`, `IpList`, `Selector`, `RegionRouteOverride`, and `Misc`.

### LinkSys

Redis, etcd, Elasticsearch, Webhook, Kafka, and HTTPDNS.

## Cross-cutting completion ledger

| Area | Baseline | Required evidence |
|---|---|---|
| Conditions | inconsistent; some hard-coded Active | shared component on all condition-bearing rows plus `STATUS-*` cases |
| Authorization | mostly server-side denial | live Controller resource access, Center permission intersection, `AUTH-*` cases |
| Topology | primarily Route to Service | every relationship in design has source parsing and `REL-*` cases |
| Dashboard | missing new resource and stale ACME field | count/expiry/conflict tests and browser cases |
| Bulk actions | inconsistent | catalog-declared support and `BATCH-*` cases |
| Skills | stale contracts and paths | both repository skill diffs reviewed and checked |
