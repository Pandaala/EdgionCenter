---
name: dashboard-backend-traffic-policy
description: EdgionBackendTrafficPolicy editor boundaries and verification
---

# Backend traffic policy editor

The Controller-scoped menu is Services → Backend Traffic Policies, at
`/controller/:controllerId/services/backend-traffic-policies`.
The page uses the ordinary Controller proxy and resource authorization.
The list identifies the configured probe protocol and each configured resilience
section, including retry constraints, circuit breaking and connection overrides.
These tags describe configuration presence; they are not live health results.

The schema authority is the sibling Edgion implementation:
`edgion-resources/src/resources/edgion_backend_traffic_policy.rs`, with
`skills/02-features/03-resources/20-edgion-backend-traffic-policy.md` as its guide.
Health-check and upstream-authority structs are shared with other Edgion schemas;
load those source definitions when changing their fields.

The editor supports homogeneous Service or EdgionBackend target references and
the current load-balancer, active-health-check, outlier, retry-budget,
circuit-breaker, connection and upstream-authority sections. AI targets support
outlier detection, retry constraints, circuit breaking and connection settings;
loadBalancer, healthCheck and upstreamAuthority are rejected. The form prevents
adding those unsupported sections, retains existing values, and allows their
explicit removal. The list never invents a RoundRobin default for an AI target.
Controller target capability validation and resolved references remain authoritative.

Active probe modes include http, https, tcp, grpc and grpcs. Encrypted modes expose
the independent OutboundTlsLocal document through a JSON field, including
validation.hostname, CA/SAN settings and a client-certificate reference. Invalid
TLS JSON and malformed HTTP status drafts jointly block submission. Basic TLS
presence/verification requirements are checked locally; Controller identity,
CA-resolution and same-namespace enforcement remain authoritative.

The mutation adapter removes healthCheck.active.resolvedCaCertificates,
resolvedClientCertificate and resolvedTlsError at their exact paths. Operator
TLS references and unrelated fields must survive. HTTPS native browser CRUD
proves round-trip configuration only; it does not prove successful TLS probes.

Normalization preserves unknown operator fields and omitted defaults. Before
rendering, it rejects invalid document/section containers, malformed target
references, non-array health-check status lists and non-string authority fields
used by validation. These structural guards protect the editor; they do not
replace Controller semantic validation. Optional null sections remain lossless,
and a null healthCheck does not enable active probes for an AI target.

Mutation documents use the shared resource adapter to remove status and
Controller-owned fields. Round-trip, form and editor tests cover preservation
and validation. Native browser actions/CRUD establish control-plane editing,
not live load balancing, health probes, ejection or retry behavior.
