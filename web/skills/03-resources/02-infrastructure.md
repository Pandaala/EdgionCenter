---
name: infrastructure-resources
description: Infrastructure resource development guide — Gateway/GatewayClass/Service/EndpointSlice/ReferenceGrant (based on feature-04-06 user documentation)
---

# Infrastructure Resources

## Gateway (Pending Development)

```yaml
apiVersion: gateway.networking.k8s.io/v1
kind: Gateway
metadata:
  name: my-gateway
  namespace: default
  annotations:
    edgion.io/enable-http2: "true"                   # HTTP/2 support (default true)
    edgion.io/http-to-https-redirect: "true"         # HTTP→HTTPS automatic redirect
    edgion.io/https-redirect-port: "443"             # HTTPS redirect port
    edgion.io/edgion-stream-plugins: "ns/name"       # Gateway-level StreamPlugins
spec:
  gatewayClassName: edgion                           # Required: associate GatewayClass
  listeners:
    - name: http
      port: 80
      protocol: HTTP                                 # HTTP | HTTPS | TCP | TLS | UDP
      hostname: "*.example.com"                      # Optional: hostname filter
      allowedRoutes:
        namespaces:
          from: Same                                 # Same | All | Selector
        kinds:
          - group: gateway.networking.k8s.io
            kind: HTTPRoute

    - name: https
      port: 443
      protocol: HTTPS
      tls:                                           # Required for HTTPS/TLS protocol
        mode: Terminate                              # Terminate | Passthrough
        certificateRefs:
          - name: my-cert-secret
            namespace: default                       # Cross-namespace requires ReferenceGrant
        frontendValidation:                          # Optional: client certificate validation
          caCertificateRefs:
            - name: client-ca
        options:
          edgion.io/cert-provider: "edgion-tls"      # "secret" (default) | "edgion-tls"

    - name: tcp-redis
      port: 6379
      protocol: TCP

    - name: tls-passthrough
      hostname: "secure.example.com"
      port: 8443
      protocol: TLS
      tls:
        mode: Passthrough

    - name: udp-dns
      port: 5353
      protocol: UDP

  addresses:                                         # Optional
    - type: IPAddress
      value: "10.0.0.1"

status:                                              # Read-only
  addresses: [...]
  conditions:
    - type: Accepted
      status: "True"
  listeners:
    - name: http
      attachedRoutes: 3
      conditions: [...]
```

**Development Notes**:
- Namespaced resource, kind: `gateway`
- Core is `listeners` array management
- Protocol enum: HTTP, HTTPS, TCP, TLS, UDP
- TLS configuration only appears for HTTPS/TLS (conditional rendering)
- annotations control HTTP/2, HTTPS redirect, and StreamPlugins
- status is read-only display (listener status, attachedRoutes, addresses)
- List page highlights: name, namespace, listener count/ports, attached route count

**Form Sections**:
- MetadataSection + AnnotationsSection (HTTP/2, HTTPS redirect toggles)
- GatewayClassName selector
- ListenersSection (dynamically add/remove listeners)
  - ListenerEditor (name + protocol + port + hostname + TLS + allowedRoutes)

## GatewayClass (Pending Development)

```yaml
apiVersion: gateway.networking.k8s.io/v1
kind: GatewayClass
metadata:
  name: edgion
spec:
  controllerName: edgion.io/gateway-controller       # Required
  parametersRef:                                      # Optional: associate EdgionGatewayConfig
    group: edgion.io
    kind: EdgionGatewayConfig
    name: default-config
  description: "Edgion Gateway Controller"            # Optional
```

**Development Notes**:
- **Cluster-scoped resource**, uses `clusterResourceApi`, kind: `gatewayclass`
- Simple structure: controllerName + parametersRef + description
- Typically only one instance
- parametersRef associates EdgionGatewayConfig
- List page displays the count of associated Gateways
- Primarily YAML editing + basic information display

## Service (Pending Development — Read-only)

```yaml
apiVersion: v1
kind: Service
metadata:
  name: backend-service
  namespace: default
spec:
  type: ClusterIP
  selector:
    app: backend
  ports:
    - name: http
      port: 80
      targetPort: 8080
      protocol: TCP
```

**Development Notes**:
- Namespaced resource, kind: `service`
- **Read-only display** (Service is managed by K8s or created by the user via YAML)
- List page displays: name, namespace, type, ports (Tags), selector
- Supports YAML viewing and editing
- Association display: Routes that reference this Service, associated EndpointSlices

## EndpointSlice (Pending Development — Read-only)

```yaml
apiVersion: discovery.k8s.io/v1
kind: EndpointSlice
metadata:
  name: backend-service-abc
  namespace: default
  labels:
    kubernetes.io/service-name: backend-service
addressType: IPv4
ports:
  - name: http
    port: 8080
    protocol: TCP
endpoints:
  - addresses: ["10.0.0.1"]
    conditions:
      ready: true
      serving: true
```

**Development Notes**:
- Namespaced resource, kind: `endpointslice`
- **Purely read-only display**
- List page displays: name, namespace, associated service, endpoint count, ready status
- Detail view shows endpoint list

## ReferenceGrant (Pending Development)

```yaml
apiVersion: gateway.networking.k8s.io/v1
kind: ReferenceGrant
metadata:
  name: allow-gateway-secret
  namespace: security          # Must be in the target resource's namespace
spec:
  from:
    - group: gateway.networking.k8s.io
      kind: Gateway
      namespace: gateway-system     # Allowed source namespace
  to:
    - group: ""                     # core/v1
      kind: Secret                  # Allow referencing Secret
```

**Development Notes**:
- Namespaced resource, kind: `referencegrant` (must be added to ResourceKind)
- Controls cross-namespace resource reference permissions
- Form: from (group + kind + namespace list) + to (group + kind list)
- List page displays: name, namespace, from resource type/namespace, to resource type
- Needs to add a menu item in the sidebar
