---
name: route-resources
description: Current dashboard editing contracts and examples for HTTPRoute, GRPCRoute, TCPRoute, UDPRoute and TLSRoute; resource schemas remain authoritative in Edgion.
---

# Route Resources

## Common Characteristics

All route resources share:
- `spec.parentRefs` — bind Gateway/Listener
- `spec.rules` — routing rules
- Namespaced resource, uses `resourceApi`

HTTPRoute, GRPCRoute and the three stream-route forms use the same
`ResourceEditor/common/ParentRefsSection`. Clearing optional namespace or
sectionName omits the field, restoring the owner namespace or listener selection
without an explicit section restriction. Port and other reference fields remain
unchanged. Narrow edits preserve sibling references and unknown operator fields.
The HTTPRoute-specific duplicate was removed; use the shared translated controls
for subsequent attachment changes.

The shared backend reference editor must preserve an explicit `weight: 0`.
Clearing the input omits weight; entering zero must not omit it, because the
Gateway defaults an absent weight to one. Zero excludes a backend from weighted
selection; HTTP named-jump behavior is a separate contract.

Ordinary backend references in all five route forms omit namespace when the
optional input is cleared, restoring the route namespace. HTTP RequestMirror
backend references use the same omission semantics. Ports, explicit zero weights,
filters, mirror sampling and unrelated references remain intact. Empty-string
namespace is not an alias for omission in the Controller's reference resolution.
Current Edgion GRPCRoute filters are RequestHeaderModifier,
ResponseHeaderModifier and ExtensionRef; RequestMirror is not supported there.
The GRPCRoute type, filter selector and mutation preflight share
`GRPC_ROUTE_FILTER_TYPES`. Form and YAML submissions validate both rule filters
and backend filters, report the exact unsupported-type path, and retain the
draft for correction. Supported filters keep unknown operator fields; Controller
validation remains authoritative for payload semantics and runtime acceptance.

The shared mutation hostname validator follows the current Controller parser and
vendored Gateway API v1.6.2 CRDs: HTTPRoute/GRPCRoute allow an omitted or empty
list, with at most 16 entries; TLSRoute requires 1 through 1024 entries. Names
must use lowercase DNS labels, optionally prefixed with `*.`, and fit 253 ASCII
characters. IP literals, bare wildcards, trailing dots and malformed labels
are rejected. Single-label names and wildcard numeric suffixes remain valid.
Validation never trims, lowercases or deletes operator values; invalid drafts
remain editable and Form/YAML submissions use the same mutation guard.

All five route menus have standalone browser list-action and CRUD coverage.
These checks establish editing and Controller persistence, not data-plane
forwarding or cross-namespace authorization. Schema examples below must be
checked against current Edgion source before extending a form.

## HTTPRoute

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
- HTTP retry codes are integers from 400 through 599. The form, schema and
  mutation boundary share this range; informational, successful and redirect
  responses cannot be configured as retry triggers. gRPC uses its separate
  0–16 status-code range.
- HTTP retry attempts must be an integer of at least one, and response codes
  must be unique. Omission uses the inherited/default policy; attempts zero is
  not a supported HTTPRoute setting.
- `spec.rules[].sessionPersistence` — session affinity (Cookie/Header)

**Edgion Extension Fields**:
- `sessionPersistence.strict` — strict affinity mode
- RequestMirror transport tuning uses route annotations, including
  `edgion.io/mirror-connect-timeout-ms`, `edgion.io/mirror-write-timeout-ms`,
  `edgion.io/mirror-max-buffered-chunks`, `edgion.io/mirror-log`, and
  `edgion.io/mirror-max-concurrent`; these are not inline RequestMirror fields.

The shared HTTP/gRPC policy editor removes optional text fields when cleared,
including request/backend timeouts, retry backoff, session name and absolute
timeout. It retains the surrounding configured block: removing the entire
block would change delegation inheritance. Current Edgion does not implement
session `idleTimeout`; the form offers only explicit removal of an existing
value, preserving it during unrelated edits. Strict affinity is FileSystem/etcd
only; standard Kubernetes Gateway API schemas prune `strict`. A Permanent
cookie still requires an absolute timeout according to Controller validation.

## GRPCRoute

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
- Current filters are RequestHeaderModifier, ResponseHeaderModifier and
  ExtensionRef. RequestMirror, RequestRedirect and URLRewrite are not supported.
- `retry.codes` are gRPC status codes (0-16), parsed but **runtime-ignored**
- Automatically detects and supports gRPC-Web requests
- Can heavily reuse HTTPRoute components

**GRPCMethodMatch Structure**:
| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | No | Exact (default) \| RegularExpression |
| `service` | string | No | gRPC service full name (e.g., `billing.v1.BillingService`) |
| `method` | string | No | gRPC method name (e.g., `CreateInvoice`) |

An explicit method matcher requires at least one non-empty service or method.
Exact names must be valid; an explicit empty string is not the same as omission.
The form removes cleared optional fields. Clearing both removes the outer
method predicate while retaining header conditions and unknown sibling fields.
An omitted method predicate is unconstrained. The last match can be removed;
new matches start without invalid empty method names.

## TCPRoute

```yaml
apiVersion: gateway.networking.k8s.io/v1
kind: TCPRoute
metadata:
  name: my-tcp-route
  namespace: default
  annotations:
    edgion.io/edgion-stream-plugins: "default/my-stream-plugins"  # StreamPlugins binding
    edgion.io/tcp-keepalive-time: "60"
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
- apiVersion: `gateway.networking.k8s.io/v1`
- Simplest route type: **no matches, no hostnames, no filters**
- Only `parentRefs` + `rules[].backendRefs`
- Supports StreamPlugins and TCP keepalive annotations. The form does not offer
  TLSRoute-only Proxy Protocol and connection-retry controls for TCPRoute.
- Use cases: Redis, MySQL, PostgreSQL, MQTT, and other TCP protocols

## UDPRoute

```yaml
apiVersion: gateway.networking.k8s.io/v1
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
- UDPRoute supports the StreamPlugins annotation for Stage-1 checks at new
  session admission. Established sessions do not re-run it for every packet.
  Gateway-level StreamPlugins do not protect UDP listeners; attach the policy
  to the UDPRoute. UDP has no TCP keepalive, Proxy Protocol or TLS-route stage.
- Can share editor components with TCPRoute

## TLSRoute

```yaml
apiVersion: gateway.networking.k8s.io/v1
kind: TLSRoute
metadata:
  name: my-tls-route
  namespace: default
  annotations:
    edgion.io/edgion-stream-plugins: "default/my-stream-plugins"
    edgion.io/proxy-protocol: "v2"
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
- Supports StreamPlugins, upstream TCP keepalive, Proxy Protocol `v2` and
  connection-retry annotations. Other Proxy Protocol values are not enabled.
- No matches, no filters

## Route Resource Reuse Matrix

| Component | HTTPRoute | GRPCRoute | TCPRoute | UDPRoute | TLSRoute |
|-----------|-----------|-----------|----------|----------|----------|
| MetadataSection | ✅ | Reuse | Reuse | Reuse | Reuse |
| StreamAnnotationsSection | — | — | Shared | Shared (plugins only) | Shared |
| ParentRefsSection | ✅ | Reuse | Reuse | Reuse | Reuse |
| HostnamesSection | ✅ | Reuse | ❌ | ❌ | Reuse |
| PathMatchField | ✅ | ❌ | ❌ | ❌ | ❌ |
| HeaderMatchField | ✅ | Reuse | ❌ | ❌ | ❌ |
| GRPCMethodMatch | ❌ | Implemented | ❌ | ❌ | ❌ |
| BackendRefsEditor | ✅ | Reuse | Reuse | Reuse | Reuse |
| FiltersEditor | ✅ | Reuse (partial) | ❌ | ❌ | ❌ |
| TimeoutsEditor | ✅ | Reuse | ❌ | ❌ | ❌ |
| RetryEditor | ✅ | Reuse | ❌ | ❌ | ❌ |
| SessionPersistence | ✅ | Reuse | ❌ | ❌ | ❌ |

Common metadata, parent, hostname and backend controls live in
`src/components/ResourceEditor/common/`. StreamRouteForm serves all three L4
route menus and preserves multiple rules and unknown fields during edits.
