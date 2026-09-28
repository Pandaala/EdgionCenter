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

## Still to reconcile in the dedicated ownership pass

Gateway/GatewayClass/global configuration; five route kinds and their nested
filters/references; HTTP/stream plugin configurations and conditions; LinkSys
variants; AI credential slots; ACME/ConfigData; Kubernetes core resource envelopes.
Earlier repairs and tests remain evidence, but each nested ownership review needs
an explicit current-source inventory before this audit can be closed.
