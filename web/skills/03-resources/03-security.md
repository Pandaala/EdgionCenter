---
name: security-resources
description: Security resource development guide — EdgionTls/Secret/BackendTLSPolicy
---

# Security & TLS Resources

## EdgionTls

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
      - type: DNS
        value: "client1.example.com"
      - type: URI
        match: Prefix
        value: "spiffe://internal/"
    allowedCns:                        # Optional: allowed client certificate CN whitelist
      - "AdminClient"
  minTlsVersion: "TLS1_2"             # Optional: minimum TLS version TLS1_0|TLS1_1|TLS1_2|TLS1_3
  ciphers:                        # Optional: custom cipher suites
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
  - Cipher controls write spec.ciphers (not cipherSuites)
- List page displays: name, namespace, host count, mTLS mode, TLS version
- allowedSans entries use DNS/URI/Email/IP/OtherName with match
  Exact/Prefix/Suffix/Contains/RegularExpression, value and optional ignoreCase.
  OtherName requires oid; other types forbid it. Omitted match defaults to Exact.
  Rust regex compilation remains Controller-owned; browser checks are preflight.
- At most 16 hosts and 32 parent references. Reference edits preserve group/kind,
  namespace, sectionName and port where supported by the reference schema.
- Strip resolved certificate material and Controller runtime fields on mutation.
  Native configuration CRUD does not establish certificate or handshake success.

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
