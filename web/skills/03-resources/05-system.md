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

## EdgionAcme (Pending Development)

```yaml
apiVersion: edgion.io/v1
kind: EdgionAcme
metadata:
  name: lets-encrypt
  namespace: default
spec:
  email: "admin@example.com"                    # Required: ACME account email
  domains:                                       # Required: certificate domains
    - "example.com"
    - "*.example.com"                            # DNS-01 supports wildcards
  server: "https://acme-v02.api.letsencrypt.org/directory"  # Optional
  keyType: "ecdsa-p256"                          # Optional: ecdsa-p256 (default) | ecdsa-p384

  challenge:                                     # Required
    type: http-01                                # http-01 | dns-01
    http01:
      gatewayRef:                                # Required for http-01
        name: my-gateway
        namespace: default
    dns01:                                       # Required for dns-01
      provider: cloudflare                       # cloudflare | alidns
      credentialRef:                             # DNS API credential Secret
        name: cloudflare-api-token
        namespace: default
      propagationTimeout: 120                    # DNS propagation timeout (seconds)
      propagationCheckInterval: 5                # DNS check interval (seconds)

  storage:                                       # Required: certificate storage
    secretName: "acme-cert"
    secretNamespace: default

  renewal:                                       # Optional: renewal configuration
    renewBeforeDays: 30                          # Days before expiry to renew
    checkInterval: 86400                         # Check interval (seconds)
    failBackoff: 300                             # Failure retry delay (seconds)

  autoEdgionTls:                                 # Optional: auto-create EdgionTls
    enabled: true
    name: "acme-lets-encrypt"                    # EdgionTls name
    parentRefs:                                  # Bind Gateway
      - name: my-gateway

status:                                          # Read-only
  phase: Ready                                   # Pending|Issuing|Ready|Renewing|Failed
  certificateSerial: "xxx"
  certificateNotAfter: "2026-07-10T00:00:00Z"
  lastFailureReason: ""
  secretName: "acme-cert"
  edgionTlsName: "acme-lets-encrypt"
```

**Development Notes**:
- Namespaced resource, kind must be added to ResourceKind: `edgionacme`
- Form sections:
  - Basic info (email, server, keyType)
  - Domain list editing
  - Challenge configuration (http-01/dns-01 conditional rendering)
    - http-01: gatewayRef selection
    - dns-01: provider + credentialRef + propagation configuration
  - Storage configuration
  - Renewal configuration
  - AutoEdgionTls configuration (toggle + name + parentRefs)
- Status read-only display: phase status badge, certificate expiry time, failure reason
- **Security sensitive**: DNS API credentials
- Supports manual certificate issuance trigger: `POST /api/v1/services/acme/{namespace}/{name}/trigger`
- List page displays: name, namespace, phase (Tag), domains, challenge type, expiry time
- Needs to add a menu item in the sidebar
