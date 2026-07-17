# Cloud infrastructure integration architecture

## Product boundary

Cloud integration is an independent EdgionCenter capability for managing the public entry
infrastructure in front of applications. Its initial boundary includes provider accounts,
DNS zones and records, certificates, edge applications, and origin pools. It is not a
general-purpose cloud console and it does not depend on Edgion Controllers, Gateway API
resources, RegionRoute, or the federation wire contract.

Future features may connect a `DomainBinding` to an Edgion region or route, but that is a
separate integration layer. Provider adapters and the base cloud domain must remain useful
without Edgion being installed.

## Dependency boundary

```text
                         +--------------------------+
                         | center-app Admin API/UI  |
                         +------------+-------------+
                                      |
                         +------------v-------------+
                         | cloud reconciliation     |
                         | planning and operations  |
                         +------------+-------------+
                                      |
               +----------------------+----------------------+
               |                                             |
    +----------v-----------+                    +------------v-----------+
    | center-core cloud    |                    | provider adapter crates |
    | intent and contracts |<-------------------| Cloudflare / AWS / GCP  |
    +----------+-----------+                    +------------------------+
               |
      +--------+---------+
      |                  |
+-----v------+    +------v-------+
| SQL store  |    | Kubernetes   |
| standalone |    | CRD adapter  |
+------------+    +--------------+
```

`center-core` contains provider-neutral intent, validated identifiers, lifecycle policy,
status Conditions, and resource relationships. It must not depend on cloud SDKs, HTTP,
SQLx, or Kubernetes libraries. Vendor SDKs belong in separate adapter crates. SQL and
Kubernetes persistence remain separate compositions as required by the existing Center
architecture.

## Resource model

Every cloud resource has common metadata, a typed spec, and common observed status.

| Resource | Responsibility |
|---|---|
| `ProviderAccount` | Selects a provider and references credentials without containing secret material. |
| `ManagedZone` | Represents an imported or Center-created authoritative DNS zone. |
| `DNSRecordSet` | Expresses one DNS owner name and record type with one or more values. |
| `DomainBinding` | Declares that a public hostname is exposed through an optional certificate, edge application, and origin pool. |
| `CertificateBinding` | Describes names, purpose, management mode, and an optional deployment target. |
| `EdgeApplication` | Represents the provider edge/CDN application serving domains and forwarding to an origin pool. |
| `OriginPool` | Describes independently addressable origins and portable active health checks. |

The first model deliberately has no Gateway, HTTPRoute, RegionRoute, cluster, or Controller
reference. Provider-specific configuration is also not accepted as arbitrary JSON or a
string map. A provider feature must first acquire explicit semantics and validation before
it becomes a typed extension.

## Identity, ownership, and deletion

- `CloudResourceId` is a Center-owned stable identifier, independent of a provider ID.
- `ProviderResourceRef` records the external provider identifier only in observed status.
- `owner` is an administrative owner/team label, not proof of provider-resource ownership.
- `Managed` resources may be reconciled after an explicit create or adoption workflow.
- `ObserveOnly` resources can be inventoried and compared but never mutated.
- `DeletionPolicy::Retain` is the default. `DeleteExternal` must be an explicit choice and
  remains subject to ownership proof, impact planning, and authorization.
- Resource references are typed by `CloudResourceKind`; adapters must reject dangling or
  wrong-kind references before provider mutation.

Provider ownership markers and adoption are designed in CLD-05. The base model does not
claim that a Center record alone proves ownership of an external object.

## Status contract

Observed status uses generation-aware Conditions. At minimum, later reconcilers can publish
`Accepted`, `CredentialsValid`, `DNSReady`, `CertificateReady`, `OriginHealthy`,
`Programmed`, and `DriftDetected`. Unknown and stale observations must not be interpreted as
success. Provider errors are summarized into stable reason codes; raw payloads and secrets
must not be stored in status.

## Credential boundary

`ProviderAccount` contains a `CredentialSource`, never resolved credential material. Four
provider-neutral source modes are supported by the core contract:

- `StaticSecret`: a stable `CredentialRef` resolved by the deployment composition.
- `Ambient`: the cloud SDK default credential chain, including instance/container identity.
- `Federated`: workload identity exchange using an ambient or referenced projected token.
- `AssumeIdentity`: role assumption or service-account impersonation from an ambient or
  referenced base identity.

Provider adapters own secret resolution and implement `CredentialInspector`. Inspection
returns only non-secret principal/scope, an opaque credential revision, expiration time, and
normalized issues. Authentication failure and permission denial are different issue kinds.
When the revision or resolved identity changes, cached provider clients are rebuilt while the
`ProviderAccount` and every resource reference to it remain unchanged.

### Standalone mode

- Prefer an SDK ambient chain or workload identity when Center runs on cloud infrastructure.
- Static Cloudflare tokens or legacy provider keys are supplied through an external secret
  mechanism and registered under stable aliases; they are not stored in Center SQL tables.
- The standalone composition resolves aliases and mounts provider adapters. Missing aliases
  fail inspection without exposing the configured location or secret value.

### Kubernetes mode

- Prefer projected ServiceAccount identity and cloud workload identity integrations. AWS and
  Google SDKs use their ambient/web-identity chains, so no long-lived key enters a CRD.
- Static tokens such as Cloudflare API Tokens are mounted from a namespace-scoped Secret or
  external-secret CSI integration. `CredentialRef` selects the configured mount/alias; the
  business API does not read or return Secret data.
- Secret RBAC must avoid list/watch access. A resolver receives access only to explicitly
  configured secret objects or mounted paths.

Cloudflare API Tokens are preferred to legacy global API keys. AWS and Google adapters must
prefer automatically refreshed temporary credentials through the SDK provider chains.

## Provider capability contract

Provider capabilities are independent snapshots, not booleans embedded in `ProviderAccount`
status. A closed `ProviderCapability` family identifies portable DNS, certificate, edge,
health-check, WAF, and cache behavior. Each `CapabilityRequirement` also names an action
(`Observe`, `Create`, `Update`, `Delete`, or `Execute`) so a read-only credential is not mistaken
for write access and delete is not unnecessarily blocked by create quota.

Every requirement evaluates six independent dimensions: adapter implementation, provider API
support, account entitlement, credential access, location availability, and quota. The core
contract accepts only typed three-state observations with dimension-specific evidence. Quota
may be explicitly not applicable for actions that do not consume capacity; unknown quota is
never treated as unlimited. Authentication failure, inability to inspect permissions, and an
authoritative business-permission denial are distinct reasons.

A snapshot is bound to the provider account ID, provider, account generation, credential
revision, typed scope, and a discovery epoch/token fence. Account, region, and resource snapshots
cannot be reused across accounts; resource scope also includes the Center resource kind because
provider external IDs are not globally unique. Snapshot stores perform an exact authority-fence
CAS, rather than sorting opaque credential revisions or trusting probe completion wall time.

`center-runtime` coordinates refreshes without a provider SDK dependency. Requests for the same
account authority and scope share one keyed flight, while a global limit bounds discoveries for
different keys. Provider calls have a timeout; adapter errors and invalid responses become fixed
failed snapshots without copying raw error text. Cache reuse is capped by both the provider
observation window and runtime policy, uses stable early-refresh jitter, and is shorter when the
credential source cannot supply an opaque revision. The credential-rotation cleanup port requires
stores to match the exact stale account generation and credential revision, so a delayed event
cannot delete a newer result. SQLite and the Kubernetes in-memory resource fake pass the shared
conformance contract; real MySQL and real Kubernetes API-server conformance remain required. The
persisted discovery token and credential revision are internal coordination fields and must be
removed from future Admin API DTOs.

Capability snapshots have a 512 KiB canonical JSON persistence budget shared by both modes.
Standalone stores an unambiguous binary scope key, exact binary authority fields, and the committed
snapshot in SQLite or MySQL. Kubernetes stores the immutable full key, active authority, monotonic
epoch, and committed JSON in one namespaced CRD status; resourceVersion is the CAS and no Lease is
needed. A normal refresh advances authority while keeping the last committed snapshot readable so
fresh traffic does not see a capability blackout. The evaluator still checks generation, credential
revision, scope, and TTL. Exact credential invalidation clears matching committed data and revokes
matching in-flight authority, preventing an old probe from restoring invalidated permissions.

Mutation evaluation is fail closed. All required dimensions must be fresh and affirmative (or a
valid quota-only not-applicable observation). Missing, stale, failed, unknown, account/provider/
scope mismatch, credential rotation, and blocking partial-discovery issues produce an
indeterminate result and cannot authorize a plan. The dashboard may use the same result to
explain disabled controls, but the future server-side planner remains authoritative.

## Reconciliation and operations

Cloud mutations are represented by durable `CloudOperation` records rather than being
performed in an Admin API request. An operation targets one resource generation and contains
ordered, independently idempotent apply steps. Compensation steps are rejected until a separate
activation and reverse-order state machine is defined. The provider-neutral store
contract deduplicates operation requests, serializes mutations per resource, and atomically
claims work with a holder, fencing token, monotonically increasing fencing epoch, and lease
expiry. Every state write must present the exact current lease, preventing a stale replica from
committing after ownership has moved.

Claim and dispatch are separate durable transitions. Before a provider call, the store marks
the step `Running`, increments its attempt, and assigns an execution token. Completion must
match both that dispatch fence and the current operation lease. The generic runtime worker
advances at most one step per claim. Provider adapters return a
normalized success, permanent failure, scheduled retry, or unknown outcome. A delayed retry
blocks later steps, and operation deadlines are checked before dispatch. The store calculates a
relative execution budget in its own time domain after reserving a completion margin. Each
provider call is bounded by that budget; long-running provider jobs must be modeled as a short
submit step followed by idempotent observation steps. Timeout, transport ambiguity after
dispatch, or an executor failure becomes `UnknownOutcome`. It is never blindly replayed: a
later observation or explicit operator decision must establish whether the external mutation
took effect.

The contracts live in `center-core`, while the worker lives in `center-runtime`; neither layer
depends on a cloud SDK or Edgion resource. CLD-34 now provides a SQL operation store for
standalone mode and an `EdgionCloudOperation` CRD plus per-resource Lease store for Kubernetes
mode. SQL uses transactional/CAS updates and a database queue order. Kubernetes uses ordered
resourceVersion CAS, exact resource and dispatch fences, monotonic Lease observation, and
conservative recovery of an abandoned `Running` step to `UnknownOutcome`.

The stores are not yet composed into either binary because there is no Admin API, planner, or
provider executor to submit safe work. Kubernetes CRD-plus-Lease updates are compensating
two-object transitions rather than a cross-object transaction. Real MySQL and two-replica
Kubernetes conformance testing remain prerequisites for enabling provider mutations. There is
still no cloud dashboard menu.

## Status and provider errors

Cloud conditions use unique typed condition keys, stable bounded UpperCamelCase reason codes,
bounded human-readable messages, observed generations, and non-decreasing transition times. A
condition is successful only when it is `True` at the exact desired generation; missing, stale, or
`Unknown` conditions fail closed. Semantically unchanged updates retain the original transition
time.

Provider adapters normalize sanitized failures into authentication, authorization, quota,
conflict, validation, not-found, transient, throttled, or unknown-outcome categories. The core maps
those categories to the durable `OperationError` projection and validates again before conversion.
Raw provider bodies, headers, and credential material must not enter either model. Correlation IDs
and bounded time-ordered cloud events exist as core contracts; persistence, retention, API, and
end-to-end linkage between Center correlation IDs, operation IDs, and provider request IDs remain
future work. Consumers must evaluate the requested condition at the exact desired generation and
must not treat the top-level observed generation alone as readiness.

## DNS provider contract

The provider DNS port uses a separate canonical model instead of the CLD-01 `Vec<String>` resource
placeholder. Names are stored as lowercase ASCII IDNA A-labels without a trailing dot; provider
renderers add the final dot when required. Owner names additionally allow underscore labels and a
wildcard only as the first complete label. Record data is typed, and RRset values use set semantics
while TXT character-string segment order is preserved.

Portable record values are separate from typed Cloudflare proxy, Route 53 alias, and Google alias
extensions. Route 53 `SetIdentifier` participates in record identity; Cloudflare member record IDs
remain observed provider-object metadata. Alias records cannot masquerade as CNAME values and have
provider-specific identity and TTL rules: Route 53 aliases inherit TTL, while Google `ALIAS` is an
apex-only provider record type with an explicit TTL. Cloudflare automatic TTL is explicit rather
than portable `Seconds(1)`, and A/AAAA/CNAME records must state `DnsOnly` or `Proxied` intent.

Zone references carry the provider account, provider kind, visibility, ID, and apex; adapters must
compare the declared provider with the resolved provider account before any call. Route 53
weighted, failover, latency, geolocation, and multivalue policies are typed and group-validated so
simple and routed sets, different routing families, duplicate selectors, and inconsistent explicit
TTLs cannot be mixed in one desired batch. Consistent explicit TTL across every routed family is
an Edgion safety policy even where Route 53 requires it only for weighted routing. Country,
US-subdivision, and latency-region strings receive bounded syntax validation in core; the adapter
must validate them against the provider's current catalogs before mutation because those catalogs
can evolve independently of Center. Optional Route 53 health-check identity is typed and
backward-compatible on the serialized extension. Portable SOA uses seven typed fields and remains
distinct from its RRset TTL. The ordinary RRset mutation contract rejects apex SOA and delegation
NS changes for every provider; CLD-13 must use a separate zone-lifecycle authority for those
control-plane operations. Provider adapters must fail closed when an SOA responsible-mailbox name
cannot be represented without losing presentation escapes.

Create uses a must-not-exist guard; replace and delete carry the exact observed canonical revision.
Adapters report whether a guard was atomic or best-effort and must not silently downgrade the
caller's minimum strength. Submission atomicity, operation outcome, and propagation are distinct:
provider-reported application is not `DNSReady`. Pagination tokens are opaque and do not promise a
cross-page consistent snapshot. The shared adapter conformance kit verifies account/zone/provider
scope, deterministic token replay, complete traversal, exact-revision CRUD, guard negotiation,
receipt isolation, and all-or-nothing preflight. The single-receipt port rejects per-change partial
success until an outcome-per-change type exists. Concrete provider adapters must run this suite;
canonical fingerprinting and CLD-14 authoritative verification remain follow-up work.

The Cloudflare adapter is a separate account-bound crate. `ProviderAccountSpec` carries a typed
provider-native account scope, so a Center resource ID is never sent as a Cloudflare account ID.
Inventory exhausts Cloudflare physical-record pages before grouping by canonical RRset identity;
mixed member TTL, proxy, flattening, comments, or tags fail closed rather than being flattened into
false state. Provider object IDs remain observed metadata and the canonical revision hashes the
ordered complete member representation. HMAC-authenticated local cursors bind the exact Center
account, provider-native account, zone, and list method using a stable composition-provided key.
Comments and tags are currently representable only when all physical RRset members agree. The
adapter is not composed into either binary.

The credential-owning HTTP client uses the fixed Cloudflare v4 production endpoint, disables
redirects, applies connection/request and decoded-body bounds, marks Authorization as sensitive,
and returns only sanitized normalized errors. Custom HTTPS endpoints are an explicit
composition-time option; plain HTTP is loopback-only for hermetic tests. Account and zone IDs are
validated as exact Cloudflare 32-hex identifiers before URL construction. Zone inventory always
requests `full,partial,secondary,internal`, because Cloudflare excludes internal zones when the
type filter is absent. List envelopes require matching page, page size, count, and result
information; unsupported record unions and unknown/non-default record settings fail the whole
observation. TXT inventory strictly parses Cloudflare's RFC 1035 quoted character-string sequence
instead of treating presentation quotes or escapes as record bytes. User-token verification
establishes token state only and is never interpreted as DNS Read or DNS Write authorization.

Cloudflare mutation supports only `BestEffort` guards. The adapter rejects `Atomic` before any
HTTP call, performs one complete fresh-inventory preflight for every change, and submits exactly
one provider batch only after all create/replace/delete guards pass. Replacement uses IDs from the
fresh observation, deletes every old physical member, and posts every desired member. Cloudflare
executes the batch in one database transaction in delete/patch/put/post order, but distributed DNS
propagation remains non-atomic. Successful submissions return an HMAC-authenticated synthetic
receipt bound to both account identities, zone, request digest, and guard strength. Mutation
transport has stricter outcome rules than inventory: a timeout, disconnect, 408/5xx, malformed or
incomplete success, or response mismatch after dispatch is `UnknownOutcome` and cannot be blindly
retried. Explicit provider rejections retain their normalized terminal or throttled category.

The Route 53 adapter is likewise an independent account-bound crate, with an SDK-free typed API
seam. Construction requires the injected client identity, already verified through STS, to match
the configured AWS account before any provider call. Inventory is public-zone-only: list filters
private zones, direct access to one is rejected, and linked-service or unknown zone features fail
closed. RRset scans replay Route 53's exact next-name, next-type, and optional next-set-identifier
tuple; Center-facing pagination uses HMAC-authenticated local cursors bound to both account
identities, method, zone, and a canonical inventory digest. A changed inventory invalidates a
continuation instead of silently skipping or repeating resources. Supported simple, alias, routed,
health-check, SOA, and delegation NS data map losslessly to the portable model. Unsupported types,
Traffic Flow, CIDR, geoproximity, or ambiguous routing fields fail the complete observation. Route
53 octal domain and character-string presentation escapes are decoded strictly before canonical
validation. Route 53 exposes no stable RRset object ID, so object IDs remain empty and revisions
hash the complete canonical RRset content.

Route 53 mutation performs one complete fresh observation, retains the exact raw provider RRset for
DELETE, and submits one transactional batch. Create uses `CREATE`; delete uses exact `DELETE`; and
replace uses adjacent exact `DELETE` plus `CREATE`, never `UPSERT`. Server-side all-or-none batch
validation provides an Atomic absence/content guard, although a content revision cannot detect an
ABA transition back to identical content. HMAC-authenticated receipts bind both account identities,
the zone, ordered request digest, provider change ID, and actual guard strength. `GetChange`
also verifies the digest-bearing provider comment and submitted-at timestamp. `PENDING` means
provider propagation remains pending; `INSYNC` means only provider-reported application to Route 53
authoritative servers. ResourceRecord element and presentation-value size quotas are enforced
before dispatch. The AWS SDK transport verifies the caller account through STS, gives reads bounded
retries and timeouts, and disables automatic mutation retries. Ambiguous post-dispatch failures or
malformed success are `UnknownOutcome`; explicit 4xx rejections keep their normalized category.
Endpoint overrides are loopback-only test seams. Real-account tests and binary composition remain
separate from default hermetic CI. The SDK-config factory can load the AWS ambient chain or wrap an
externally resolved base configuration in a refreshable AssumeRole provider; it validates all
non-secret role parameters, rejects inherited global endpoint overrides, redacts provider debug
output, and never resolves or persists Center secret references. A resolved external ID remains only
inside the in-memory AWS refresh provider. An ignored live-account suite uses an operator-provided
disposable public zone and performs exact create/replace/delete cleanup plus a deterministic stale
exact-batch race. Binary composition remains a separate follow-up slice.

The Cloud DNS adapter follows the same independent boundary and binds every provider request,
Center cursor, and change receipt to the configured Google Cloud project and managed-zone ID. It
supports public and private authoritative zones while forwarding, peering, reverse-lookup, and
Service Directory zones fail closed. Static RRsets, apex `ALIAS`, Geo, WRR, Primary/Backup, health
checks, and internal load-balancer targets have typed representations; raw provider extensions are
retained for exact deletion and revision hashing. DNSSEC-incompatible ALIAS and health-target shapes
are rejected before mutation. One Cloud DNS Change carries all additions and exact deletions, so a
server rejection cannot partially apply the collection. The REST transport uses ADC for attached
service accounts, Workload Identity Federation, or service-account files. Safe reads use bounded
retries; a mutation is dispatched once and any ambiguous post-dispatch failure is
`UnknownOutcome`. Production uses the fixed HTTPS endpoint and only a loopback test seam can
override it. Public and private capability profiles are reported separately. An ignored real-project
test is available for a pre-provisioned disposable zone; binary and product-surface composition are
later work.

Zone lifecycle is a separate port from RRset mutation. `ManagedZone` records whether a provider
zone was imported or Center-created and defaults to imported, observe-only, and retain. Cloudflare,
Route 53, and Cloud DNS lifecycle adapters expose provider-assigned nameservers, DNSSEC state, DS
handoff, create/delete receipts, and full provider-state revisions. Provider completion never sets
DNS readiness. An independent authority-verifier port supplies zone- and revision-bound parent NS
and authoritative-resolution evidence; public readiness additionally requires an exact delegated
NS set. Registrar and parent-zone changes are external actions, not implicit side effects.

Zone deletion uses a fail-closed plan. Imported, observe-only, retained, non-empty,
delegated/unverified, or DNSSEC-enabled zones are blockers that acknowledgement cannot override.
Only a fresh safe plan plus a zone- and revision-bound approval can mint the sealed, non-wire
deletion capability accepted by provider adapters, which re-observe provider state before delete.
DNSSEC disable is likewise blocked until an independent parent check proves DS removal and the
required cache hold; adapters never turn signing off first. CLD-14 owns the actual resolver,
evidence freshness window, and parent NS/DS polling.

## Lifecycle examples

### Import and observe a DNS zone

1. Create a `ProviderAccount` containing only a credential reference.
2. Discover a provider zone and create a `ManagedZone` with `ObserveOnly` and `Retain`.
3. Record its provider ID in status after observation.
4. Do not mutate records until a later explicit adoption plan succeeds.

### Create a standalone public domain

1. Create or adopt a `ManagedZone`.
2. Create an `OriginPool` independent of Edgion.
3. Create an `EdgeApplication` referencing the origin pool.
4. Create a `CertificateBinding` for the public hostname.
5. Create a `DomainBinding` joining the zone, certificate, edge application, and pool.
6. A later reconciler plans and applies child provider resources in dependency order.

### Delete a domain without deleting shared infrastructure

1. Plan removal and calculate reverse references.
2. Detach the hostname from the edge application and certificate target.
3. Remove only owned DNS records created for the binding.
4. Retain shared origin pools, certificates, zones, and provider accounts.
5. Delete external resources only when their own deletion policy permits it and no live
   references remain.

## Research basis

- ExternalDNS separates desired sources, plans, an ownership registry, and provider
  adapters: https://kubernetes-sigs.github.io/external-dns/latest/docs/contributing/design/
- ExternalDNS treats ownership as a first-class safety boundary:
  https://kubernetes-sigs.github.io/external-dns/latest/docs/registry/registry/
- Crossplane managed resources establish useful observe/manage and retain/delete lifecycle
  distinctions: https://docs.crossplane.io/latest/managed-resources/managed-resources/
- Cloudflare recommends scoped API Tokens over legacy API keys:
  https://developers.cloudflare.com/fundamentals/api/get-started/
- AWS SDK credential chains support automatically refreshed ambient, web-identity, and
  assume-role credentials:
  https://docs.aws.amazon.com/sdkref/latest/guide/standardized-credentials.html
- Google recommends Application Default Credentials and Workload Identity Federation:
  https://cloud.google.com/docs/authentication/application-default-credentials and
  https://cloud.google.com/iam/docs/workload-identity-federation
- Kubernetes Secret access requires encryption and least-privilege RBAC:
  https://kubernetes.io/docs/concepts/security/secrets-good-practices/
- Kubernetes controllers continuously reconcile desired and observed state:
  https://kubernetes.io/docs/concepts/architecture/controller/
- Azure's asynchronous request-reply pattern motivates accepted operations, status polling,
  durable state, and idempotency-aware retry behavior:
  https://learn.microsoft.com/azure/architecture/patterns/async-request-reply
- Gateway API publishes stable supported-feature declarations rather than requiring clients to
  infer implementation behavior:
  https://gateway-api.sigs.k8s.io/geps/gep-2162/
- AWS IAM simulation is useful evidence but does not exactly reproduce every authorization path:
  https://docs.aws.amazon.com/IAM/latest/APIReference/API_SimulatePrincipalPolicy.html
- Google IAM permission testing returns the permissions held by the caller for a resource:
  https://cloud.google.com/iam/docs/testing-permissions
- Cloudflare token verification proves token state, not every zone/account business permission:
  https://developers.cloudflare.com/api/resources/user/subresources/tokens/methods/verify/

These projects inform the separation and safety rules; EdgionCenter does not adopt their
APIs as its public contract.
