---
name: infrastructure-resources
description: Infrastructure resource development guide — Gateway/GatewayClass/Service/EndpointSlice/ReferenceGrant (based on feature-04-06 user documentation)
---

# Infrastructure Resources

## Gateway

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
    edgion.io/edgion-stream-plugins: "plugin-name"   # Same-namespace Gateway StreamPlugins
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
          edgion.io/cert-provider: "EdgionTls"       # Exact dynamic provider value; omit for Secret refs

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
- List columns show name, namespace, age, GatewayClass, up to four listeners and status.
- Existing listeners and allowedRoutes fields survive narrow edits; native listener
  status and attachedRoutes are available in read-only details.

**Form Sections**:
- MetadataSection + AnnotationsSection (HTTP/2, HTTPS redirect toggles)
- GatewayClassName selector
- ListenersSection (dynamically add/remove listeners)
  - ListenerEditor (name + protocol + port + hostname + TLS + allowedRoutes)

## GatewayClass

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
- Multiple GatewayClasses may coexist.
- parametersRef associates EdgionGatewayConfig
- List columns show name, age, controllerName, description and status.
- The editor supports structured fields and YAML, including parametersRef.

## Service

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
- CRUD actions are gated by Controller permissions; the default remote policy is read-only.
- List columns show name, namespace, age, type and up to three ports.
- The form covers type, ExternalName, session affinity, selectors and multiple ports.
  Clearing optional targetPort removes the field; never serialize an empty string.
  Current Gateway normalization defaults an absent targetPort to the Service port,
  while an empty string makes the port identity invalid. Numeric and named values
  retain their JSON types, and sibling ports/unknown fields survive the edit.
  Form/YAML conversion preserves additional operator fields through a narrow edit;
  mutation serialization strips status/server metadata and retains update resourceVersion.
- Topology links Routes, Services and associated EndpointSlices. These are declared
  relationships, not proof of successful routing or endpoint health.

## EndpointSlice

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
- CRUD actions are gated by Controller permissions; Form and YAML views are available.
- Address type, ports and endpoints are top-level fields, not fields under spec.
- List columns show associated Service and the count of explicitly ready endpoints.
- The form edits multiple ports/endpoints, addresses, ready, serving and hostname;
  other operator fields remain editable through YAML and survive narrow form edits.
- Current Gateway discovery accepts only `conditions.ready == true` into ready_slots.
  Missing conditions, missing ready, null and false are not ready. Serving alone
  does not make an endpoint eligible; terminating does not override explicit ready.
  The topology backend node follows this exact rule. This is the current Edgion
  implementation, not a generic interpretation of Kubernetes readiness defaults.
- Source readiness is separate from active health checks and outlier detection.
  A ready node does not establish runtime health, port validity or route admission.
- Runtime authority: sibling `edgion-gateway/src/backends/discovery/coordinator.rs`,
  the loop building EndpointOrigin and ready_slots.

## ReferenceGrant

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
- Namespaced resource, kind: `referencegrant`
- Controls cross-namespace resource reference permissions
- Form: from (group + kind + namespace list) + to (group + kind list)
- List page displays: name, namespace, from resource type/namespace, to resource type
- The existing sidebar menu supports Form/YAML editing and preserves multiple from/to entries.


## Resource action permissions

Gateway, GatewayClass and ReferenceGrant list actions use PermissionAwareButton
with their concrete kind and verb. The same applies to route, TLS, ACME and
Gateway configuration lists. Pending or failed authorization disables actions;
confirmed read permission does not enable writes. Use the existing access document
and Center proxy permissions rather than duplicating policy logic in a page.
Server-side authorization remains the execution boundary.
