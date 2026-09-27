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

## LinkSys (Pending Development)

```yaml
apiVersion: edgion.io/v1
kind: LinkSys
metadata:
  name: redis-cluster
  namespace: default
spec:
  type: redis                   # redis | elasticsearch | etcd | webhook
  redis:
    addresses:
      - "127.0.0.1:6379"
    password: "secret"
    database: 0
    clusterMode: false
    tls:
      enable: false
```

**Development Notes**:
- Namespaced resource, kind: `linksys`
- type determines the specific spec structure (conditional rendering)
- Four types: redis, elasticsearch, etcd, webhook
- **Security sensitive**: password field uses a password input
- Form switches between different configuration sections based on type
- List page displays: name, namespace, type, connection address

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
