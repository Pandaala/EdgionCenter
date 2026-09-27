---
name: security-resources
description: Security resource development guide — EdgionTls/Secret/BackendTLSPolicy (based on feature-04-06 user documentation)
---

# Security & TLS Resources

## EdgionTls (Pending Development)

```yaml
apiVersion: edgion.io/v1
kind: EdgionTls
metadata:
  name: example-tls
  namespace: default
spec:
  parentRefs:                          # Optional: bind Gateway
    - name: my-gateway
      namespace: default
  hosts:                               # Required: domain list (wildcard supported)
    - "*.example.com"
    - "api.example.com"
  secretRef:                           # Required: server certificate Secret reference
    name: example-cert
    namespace: default                 # Optional
  clientAuth:                          # Optional: mTLS client authentication
    mode: Mutual                       # Terminate (default) | Mutual | OptionalMutual
    caSecretRef:                       # Required when mode=Mutual/OptionalMutual
      name: client-ca
      namespace: default
    verifyDepth: 1                     # Certificate chain validation depth (1-9), default 1
    allowedSans:                       # Optional: allowed client certificate SAN whitelist
      - "client1.example.com"
      - "*.internal.example.com"
    allowedCns:                        # Optional: allowed client certificate CN whitelist
      - "AdminClient"
  minTlsVersion: "TLS1_2"             # Optional: minimum TLS version TLS1_0|TLS1_1|TLS1_2|TLS1_3
  cipherSuites:                        # Optional: custom cipher suites
    - ECDHE-RSA-AES256-GCM-SHA384
    - ECDHE-RSA-AES128-GCM-SHA256
    - ECDHE-RSA-CHACHA20-POLY1305
```

**Development Notes**:
- Namespaced resource, kind: `edgiontls`
- Core fields: hosts + secretRef + clientAuth + TLS configuration
- Form sections:
  - MetadataSection
  - ParentRefsSection (Gateway binding, optional)
  - HostsSection — domain list editing (wildcard supported)
  - SecretRefSection — certificate reference selector
  - ClientAuthSection — mTLS configuration (mode conditionally renders caSecretRef, etc.)
  - TlsVersionSection — minimum version dropdown
  - CipherSuitesSection — cipher suite multi-select
- List page displays: name, namespace, host count, mTLS mode, TLS version

## Secret (Pending Development)

**TLS type**:
```yaml
apiVersion: v1
kind: Secret
metadata:
  name: my-cert
  namespace: default
type: kubernetes.io/tls
data:
  tls.crt: <base64>
  tls.key: <base64>
```

**CA type**:
```yaml
apiVersion: v1
kind: Secret
metadata:
  name: client-ca
  namespace: default
type: Opaque
data:
  ca.crt: <base64>
```

**Development Notes**:
- Namespaced resource, kind: `secret`
- Type enum: `kubernetes.io/tls`, `Opaque`
- **Security sensitive**: tls.key should not be displayed in plaintext on the frontend
- Creation form:
  - type selection (TLS certificate / CA certificate / Generic)
  - File upload (PEM format) or text paste
  - Base64 encoding handled on the frontend
- List page displays: name, namespace, type, data keys, creation time
- View mode: display certificate info (expiry, CN, etc.), **hide** key content
- Association display: EdgionTls/Gateway that reference this Secret

## BackendTLSPolicy

```yaml
apiVersion: gateway.networking.k8s.io/v1
kind: BackendTLSPolicy
metadata:
  name: backend-tls
  namespace: default
spec:
  targetRefs:
    - group: ""
      kind: Service
      name: backend-service
  validation:
    caCertificateRefs:
      - name: backend-ca
        group: ""
        kind: Secret
    hostname: "backend.internal"
```

**Development Notes**:
- Namespaced resource, kind: `backendtlspolicy`
- Defines the gateway → backend mTLS policy
- One target: core Service (optional sectionName) or edgion.io EdgionBackend
  (no sectionName). The form prevents adding a second target.
- Choose same-namespace Secret/ConfigMap CA references or wellKnownCACertificates:
  System, never both. Client certificate option accepts a bare Secret name.
- validation.hostname is a precise, lowercase hostname, at most 253 characters.
- Optional subjectAltNames contains 1–5 typed Hostname or URI entries. Hostname
  permits a leading wildcard; URI must be absolute and at most 253 UTF-8 bytes.
  Each entry carries only the field matching its type. Removing the final form
  entry omits subjectAltNames instead of submitting an invalid empty array.
- Form and YAML submissions share validation and runtime-field stripping.
  Controller validation remains authoritative; CRUD evidence is not handshake
  evidence.
