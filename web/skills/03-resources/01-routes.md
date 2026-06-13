---
name: route-resources
description: Route resource development guide — complete Schema for HTTPRoute/GRPCRoute/TCPRoute/UDPRoute/TLSRoute (based on feature-04-06 user documentation)
---

# Route Resources

## Common Characteristics

All route resources share:
- `spec.parentRefs` — bind Gateway/Listener
- `spec.rules` — routing rules
- Namespaced resource, uses `resourceApi`

## HTTPRoute ✅ Completed

- apiVersion: `gateway.networking.k8s.io/v1`
- Kind: `httproute`
- Reference code: `src/pages/Routes/HTTPRouteList.tsx`, `src/components/ResourceEditor/HTTPRoute/`

Full Schema: see backend documentation: `edgion/skills/02-features/03-resources/04-httproute.md`

Key fields:
- `spec.parentRefs` — Gateway binding
- `spec.hostnames` — hostname matching (wildcard supported)
- `spec.rules[].matches` — match conditions (path/headers/queryParams/method, OR relationship)
- `spec.rules[].filters` — filter chain (RequestHeaderModifier, ResponseHeaderModifier, RequestRedirect, URLRewrite, RequestMirror, ExtensionRef)
- `spec.rules[].backendRefs` — backend references (name/port/weight, supports backendRef-level filter)
- `spec.rules[].timeouts` — request/backendRequest timeout
- `spec.rules[].retry` — attempts/backoff/codes retry policy
- `spec.rules[].sessionPersistence` — session affinity (Cookie/Header)

**Edgion Extension Fields**:
- `extensionRefMaxDepth` — ExtensionRef nesting depth limit
- `sessionPersistence.strict` — strict affinity mode
- RequestMirror extension: `connectTimeoutMs`, `writeTimeoutMs`, `maxBufferedChunks`, `mirrorLog`, `maxConcurrent`

## GRPCRoute (Pending Development)

```yaml
apiVersion: gateway.networking.k8s.io/v1
kind: GRPCRoute
metadata:
  name: my-grpc-route
  namespace: default
spec:
  parentRefs:
    - name: my-gateway
      sectionName: grpc-https
  hostnames:
    - "grpc.example.com"
  rules:
    - matches:
        - method:
            type: Exact                # Exact | RegularExpression
            service: "mypackage.MyService"  # gRPC service FQDN
            method: "GetItem"               # gRPC method name
          headers:
            - type: Exact
              name: x-custom-header
              value: "value"
      filters:
        - type: RequestHeaderModifier
          requestHeaderModifier:
            set: [{ name: x-backend-version, value: "v2" }]
        - type: ResponseHeaderModifier
          responseHeaderModifier:
            add: [{ name: x-trace-id, value: "{{generated}}" }]
        - type: RequestMirror
          requestMirror:
            backendRef: { name: mirror-service, port: 50051 }
            fraction: { numerator: 5, denominator: 100 }
        - type: ExtensionRef
          extensionRef: { group: edgion.io, kind: EdgionPlugins, name: grpc-auth }
      backendRefs:
        - name: grpc-service
          port: 50051
          weight: 100
      timeouts:
        request: "30s"
        backendRequest: "10s"
      retry:
        attempts: 3
        backoff: "500ms"
        codes: [14]  # gRPC status codes (parsed but runtime-ignored)
      sessionPersistence:
        type: Cookie
        sessionName: "GRPC_SESSION"
```

**Development Notes**:
- Very similar to HTTPRoute; the main difference is that matches uses `method` (gRPC service/method) instead of `path`
- **Does not support** RequestRedirect and URLRewrite filters
- `retry.codes` are gRPC status codes (0-16), parsed but **runtime-ignored**
- Automatically detects and supports gRPC-Web requests
- Can heavily reuse HTTPRoute components

**GRPCMethodMatch Structure**:
| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | No | Exact (default) \| RegularExpression |
| `service` | string | Yes | gRPC service full name (e.g., `billing.v1.BillingService`) |
| `method` | string | Yes | gRPC method name (e.g., `CreateInvoice`) |

## TCPRoute (Pending Development)

```yaml
apiVersion: gateway.networking.k8s.io/v1alpha2
kind: TCPRoute
metadata:
  name: my-tcp-route
  namespace: default
  annotations:
    edgion.io/edgion-stream-plugins: "default/my-stream-plugins"  # StreamPlugins binding
    edgion.io/proxy-protocol: "1"       # Proxy Protocol version (1|2)
    edgion.io/max-connect-retries: "3"  # Maximum connection retry count
spec:
  parentRefs:
    - name: my-gateway
      sectionName: tcp-9000
  rules:
    - backendRefs:
        - name: tcp-service
          port: 9000
          weight: 100
```

**Development Notes**:
- apiVersion: `gateway.networking.k8s.io/v1alpha2`
- Simplest route type: **no matches, no hostnames, no filters**
- Only `parentRefs` + `rules[].backendRefs`
- Binds StreamPlugins, Proxy Protocol, and connection retries via annotations
- Form requires annotation editing support
- Use cases: Redis, MySQL, PostgreSQL, MQTT, and other TCP protocols

## UDPRoute (Pending Development)

```yaml
apiVersion: gateway.networking.k8s.io/v1alpha2
kind: UDPRoute
metadata:
  name: my-udp-route
  namespace: default
spec:
  parentRefs:
    - name: my-gateway
      sectionName: udp-5300
  rules:
    - backendRefs:
        - name: udp-service
          port: 5300
```

**Development Notes**:
- Structure is nearly identical to TCPRoute
- Use cases: DNS, log collection, game communications, and other stateless protocols
- UDP is connectionless and does not support StreamPlugins annotations
- Can share editor components with TCPRoute

## TLSRoute (Pending Development)

```yaml
apiVersion: gateway.networking.k8s.io/v1
kind: TLSRoute
metadata:
  name: my-tls-route
  namespace: default
  annotations:
    edgion.io/edgion-stream-plugins: "default/my-stream-plugins"
    edgion.io/proxy-protocol: "2"
    edgion.io/max-connect-retries: "3"
spec:
  parentRefs:
    - name: my-gateway
      sectionName: tls-passthrough
  hostnames:           # SNI matching
    - "secure.example.com"
    - "*.internal.example.com"
  rules:
    - backendRefs:
        - name: tls-backend
          port: 8443
          weight: 100
```

**Development Notes**:
- apiVersion: `gateway.networking.k8s.io/v1` (promoted from v1alpha3 to v1)
- Routes based on the SNI in the TLS ClientHello
- Has `hostnames` (SNI matching) in addition to TCPRoute
- Supports StreamPlugins and Proxy Protocol annotations
- No matches, no filters

## Route Resource Reuse Matrix

| Component | HTTPRoute | GRPCRoute | TCPRoute | UDPRoute | TLSRoute |
|-----------|-----------|-----------|----------|----------|----------|
| MetadataSection | ✅ | Reuse | Reuse | Reuse | Reuse |
| AnnotationsEditor | — | — | New (stream) | — | Reuse (stream) |
| ParentRefsSection | ✅ | Reuse | Reuse | Reuse | Reuse |
| HostnamesSection | ✅ | Reuse | ❌ | ❌ | Reuse |
| PathMatchField | ✅ | ❌ | ❌ | ❌ | ❌ |
| HeaderMatchField | ✅ | Reuse | ❌ | ❌ | ❌ |
| GRPCMethodMatch | ❌ | New | ❌ | ❌ | ❌ |
| BackendRefsEditor | ✅ | Reuse | Reuse | Reuse | Reuse |
| FiltersEditor | ✅ | Reuse (partial) | ❌ | ❌ | ❌ |
| TimeoutsEditor | ✅ | Reuse | ❌ | ❌ | ❌ |
| RetryEditor | ✅ | Reuse | ❌ | ❌ | ❌ |
| SessionPersistence | ✅ | Reuse | ❌ | ❌ | ❌ |

**Conclusion**: Extract shared components into `src/components/ResourceEditor/common/`.
