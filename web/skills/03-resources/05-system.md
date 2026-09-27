---
name: system-resources
description: System configuration resource development guide — EdgionGatewayConfig/LinkSys/EdgionAcme (based on feature-04-06 user documentation)
---

# System Configuration Resources

## EdgionGatewayConfig (Pending Development)

```yaml
apiVersion: edgion.io/v1alpha1
kind: EdgionGatewayConfig
metadata:
  name: default-config
spec:
  # Pingora server configuration
  server:
    threads: 0                              # uint32, default: number of CPU cores
    workStealing: true                      # bool
    gracePeriodSeconds: 30                  # uint64
    gracefulShutdownTimeoutS: 10            # uint64
    upstreamKeepalivePoolSize: 128          # uint32
    enableCompression: false                # bool, downstream response compression
    downstreamKeepaliveRequestLimit: 1000   # uint32, 0=unlimited

  # HTTP timeout configuration
  httpTimeout:
    client:
      readTimeout: "60s"
      writeTimeout: "60s"
      keepaliveTimeout: "75s"
    backend:
      defaultConnectTimeout: "5s"
      defaultRequestTimeout: "60s"
      defaultIdleTimeout: "300s"

  # Maximum retry count (migrated from annotation)
  maxRetries: 3                             # uint32

  # Real IP extraction
  realIp:
    trustedIps: []                          # Trusted proxy IP/CIDR
    realIpHeader: "X-Forwarded-For"         # Header used to extract the Real IP
    recursive: true                         # Traverse right-to-left, skipping trustedIps

  # Security protection
  securityProtect:
    xForwardedForLimit: 200                 # Maximum XFF bytes
    requireSniHostMatch: true               # HTTPS 421 Misdirected Request detection
    fallbackSni: ""                         # Fallback when client sends no SNI
    tlsProxyLogRecord: true                 # Log TLS proxy connection records

  # Global plugin reference
  globalPluginsRef:                         # Global plugins applied to all routes
    - name: "global-cors"
      namespace: "edgion-system"

  # Preflight policy
  preflightPolicy:
    mode: "cors-standard"                   # "cors-standard" | "all-options"
    statusCode: 204                         # Response code when no CORS plugin is present

  # ReferenceGrant validation
  enableReferenceGrantValidation: false     # bool
```

**Development Notes**:
- **Cluster-scoped resource**, uses `clusterResourceApi`, kind: `edgiongatewayconfig`
- apiVersion: `edgion.io/v1alpha1` (note: not v1)
- Associated via GatewayClass.spec.parametersRef
- Typically only one instance (consider a singleton edit page)
- Form sections (grouped by function):
  - **Server** — threads, workStealing, gracePeriod, keepalive, compression
  - **HTTP Timeout** — client(read/write/keepalive) + backend(connect/request/idle)
  - **Max Retries** — global upstream maximum retries
  - **Real IP** — trustedIps list + header + recursive
  - **Security** — XFF limit, SNI/Host matching, fallback SNI, TLS logging
  - **Global Plugins** — global plugin reference list
  - **Preflight** — mode selector + statusCode
  - **ReferenceGrant** — toggle

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
- List displays phase, domains, challenge type, certificate expiry, and conditions.
  Manual issuance uses the Controller service endpoint through Center's proxy:
  `POST /api/v1/services/acme/{namespace}/{name}/trigger`.
