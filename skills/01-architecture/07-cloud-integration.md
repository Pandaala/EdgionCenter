# Cloud infrastructure integration architecture

## Product boundary

Cloud integration is an independent EdgionCenter capability for provider accounts, DNS zones,
DNS records, DNS verification, and bounded WAF capability discovery. Provider DNS and WAF
surfaces remain provider-specific; Center does not provide a unified exposure, edge, origin,
certificate, health-check, or cache control plane. It does not depend on Edgion Controllers,
Gateway API resources, RegionRoute, or the federation wire contract.

## Dependency boundary

```text
                         +--------------------------+
                         | center-app Admin API/UI  |
                         +------------+-------------+
                                      |
                         +------------v-------------+
                         | provider-specific direct |
                         | call services            |
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
status Conditions, and retained resource references. It must not depend on cloud SDKs, HTTP,
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

The first model deliberately has no Gateway, HTTPRoute, RegionRoute, cluster, or Controller
reference. Provider-specific configuration is also not accepted as arbitrary JSON or a
string map. A provider feature must first acquire explicit semantics and validation before
it becomes a typed extension.

## Identity, ownership, and deletion

- `CloudResourceId` is a Center-owned stable identifier, independent of a provider ID.
- `ProviderResourceRef` records the external provider identifier only in observed status.
- `owner` is an administrative owner/team label, not proof of provider-resource ownership.
- `Managed` provider-account and DNS resources may be changed only through an explicit,
  provider-specific direct-call API.
- `ObserveOnly` resources can be inventoried and compared but never mutated.
- `DeletionPolicy::Retain` is the default. `DeleteExternal` must be an explicit choice and
  remains subject to ownership proof, impact planning, and authorization.
- Resource references are typed by `CloudResourceKind`; adapters must reject dangling or
  wrong-kind references before provider mutation.

The base model does not claim that a Center record or provider marker alone proves ownership of
an external object. Each mutation API must establish its own exact authority and revision guard.

## Status contract

Observed status uses generation-aware Conditions. Retained services can publish
`Accepted`, `CredentialsValid`, `DNSReady`, `WafReady`, `Programmed`, and `DriftDetected`.
Unknown and stale observations must not be interpreted as
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

Provider adapters own secret resolution and implement `CredentialInspector`. Inspection returns
an internal provider identity, an opaque credential revision, expiration time, and normalized
issues. The runtime validates principal shape but does not classify it as safe for serialization;
only provider and scope can cross the generic Admin boundary. Authentication failure and
permission denial are different issue kinds.
When the revision or resolved identity changes, cached provider clients are rebuilt while the
`ProviderAccount` and every resource reference to it remain unchanged.

ProviderAccount persistence has a provider-neutral desired-state contract. API callers supply
metadata and `ProviderAccountSpec`, but never generation or observed status. A store assigns
generation one on create, increments it only through exact-generation compare-and-swap, and lists
accounts with exact-byte keyset ordering. Persisted adapters must run the shared conformance suite
and revalidate the complete stored shape on read. The first persistence slice is deliberately
`Retain`-only and has no delete operation: account deletion remains unavailable until retained
provider-resource references can be checked safely. Credential sources remain
bounded references or provider identity selectors; resolved secret material is never persisted.

Standalone persists ProviderAccount desired state as an exact binary account ID, a signed positive
generation, and bounded JSON. SQLite and MySQL use the database primary key for atomic create and a
single conditional update for generation CAS; keyset listing compares the binary ID and fetches
one extra row to prove whether a continuation exists. Every stored row is reconstructed through
the core helper and revalidated. Malformed JSON, identity, generation, or desired state is an
adapter failure, never a normal version conflict. The SQL adapter runs the same conformance suite
that the Kubernetes adapter must implement.

Kubernetes persists the same desired state in a namespaced `EdgionProviderAccount`. Its DNS-safe
name is derived from a domain-separated account-ID digest, while the immutable original ID remains
in spec for collision detection. An explicit desired generation changes on every replacement and
must equal the API-server generation; `resourceVersion` provides the write CAS. Metadata-only
conflicts are retried within a fixed bound, but a competing desired-state winner returns the same
typed generation mismatch as SQL. The runtime RBAC grants only get, list, create, and update on the
main resource and grants no ProviderAccount delete, status, watch, or Secret access.

The ProviderAccount Admin surface is mounted at `/api/v1/center/cloud/provider-accounts` only when
the active composition supplies a store. It supports create, exact-byte keyset list, get, and
strong ETag/If-Match replacement; delete remains absent. HTTP request DTOs are independent from the
core serde model and recursively reject unknown fields. New API account IDs use a bounded URL-safe
form, provider-native scope is required and immutable, and deletion policy is fixed to `Retain`.
Mutations require both `provider-accounts:write` and `provider-credentials:use`, while observation
uses the separate high-trust `provider-accounts:read` permission. Responses expose configured
credential references and identity selectors but never resolved material, status authorities, or
provider responses. Existing installations must explicitly grant the new permissions to roles.

Capability snapshot reads are a separate high-trust surface at
`/api/v1/center/cloud/provider-capabilities/accounts/{account_id}` and require
`provider-capabilities:read`. Keeping this outside the ProviderAccount route tree prevents a
Kubernetes non-resource wildcard for account observation from implicitly granting capability
posture access. The query identifies one exact account, region, or provider-resource
scope; the API does not add a broad snapshot-list port. Responses preserve discovery state, typed
six-dimension evidence, reasons, and each observation's validity window, but remove persistence
contract version, credential revision, discovery epoch/token, and adapter diagnostic text/code.
The response compares the snapshot's observed account generation with the current ProviderAccount
and reports stale or unknown authority explicitly. `Observed` and `Complete` never mean usable,
fresh, or allowed: credential authority remains unknown until a later credential-inspection
composition can prove it. Missing snapshots are reported as `not_discovered`. This read slice does
not construct a discoverer or make provider requests.

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

The first static-secret slice is the SDK-free `center-adapter-credential-files` crate. Both
compositions share a strict `mounted_credentials` configuration that defaults off. When enabled,
the process opens one absolute root as a `cap-std` directory capability, validates strict relative
locators, rejects a Unix root writable by group or world, reads a 32-byte non-zero revision key,
and constructs only an optional resolver. Alias
resolution requires an exact credential-reference, ProviderAccount, provider, and closed-purpose
match before file I/O. Credential files retain exact bytes, are limited to 16 KiB, zeroize on drop,
and must be regular and non-empty; Unix group/world-writable files and special files fail closed.
Nonblocking opens prevent FIFO startup hangs. Domain-separated length-framed HMAC-SHA256 revisions
change with binding authority or exact material without exposing a raw secret hash.

Capability-relative symlink traversal permits Kubernetes projected-volume `..data` rotation but
cannot leave the opened root. The base Kubernetes manifests and RBAC remain unchanged. A
non-default example lets a non-root init container read two explicitly named `0440` Secret files
through `fsGroup`, then stages them into a UID-1000-owned, memory-backed `0400` directory before
Center starts. Center gains no Secret API permission. CLD-02A does not inject the resolver into
`ApiState`, build an inspector/provider client, or cause network traffic.

The first provider-specific consumer is the independent `center-integration-cloudflare` crate.
Both binaries share a second strict, default-off `cloudflare_credential_inspection` switch. When
enabled it requires the active mode's ProviderAccount store plus the mounted resolver and installs
the existing credential-inspection service in `ApiState`; the capability bit follows that actual
service value. It accepts only exact Cloudflare `StaticSecret` authority, verifies token state,
then probes `zones` with page/per-page one for the configured provider account. A non-empty exact-
account result proves scope; an empty successful page remains `Unknown`. Production always uses
`api.cloudflare.com` and configuration exposes no endpoint override. Provider diagnostics and
credential details are mapped to fixed typed issues, while the resolver's keyed revision remains
opaque. Credential inspection itself does not mount Cloudflare DNS Admin, run background probes,
or change Kubernetes Secret RBAC.

Read-only Cloudflare DNS production access is a separate strict `cloudflare_dns_read` composition.
It implements the existing four-method Admin port in `center-integration-cloudflare`, constructs a
fresh account-bound client per operation for the fixed production endpoint, and advertises the
route capability only from the actual service value. One operation deadline covers store access,
bounded global/per-account admission, both mounted-key reads, every provider page, and pre/post
authority checks. Account generation/spec plus token, active-cursor, and optional fallback-cursor
revisions must remain unchanged or the observation is discarded. Cursor verification precedes
provider I/O, and provider loops are bounded to 200 zone pages and 20 record pages. Cursor version
4 binds page size, exact list method, keyed canonical inventory, issue time, and expiry. A changed
inventory returns a fixed restart-required response instead of combining pages from different
observations. The service never retries automatically.

The cursor HMAC is an exact 32-byte binding under the closed
`cloudflare_dns_cursor_hmac` purpose. Its reference and material must differ from the API token;
the resolver revision key remains separately protected by path and file identity. Pagination uses
one active signing key and at most one verification-only fallback. Successful fallback
continuations are immediately reissued by active; rotation uses an explicit three-stage rollout
and waits lifetime plus twice the permitted clock skew after the last old-active replica exits.
The stateless protocol fails closed per replica but does not claim distributed configuration
consensus. `cloudflare-dns:read` is a high-trust grant across all configured Cloudflare accounts,
not an account-scoped permission. Record-list handlers
receive the authoritative zone and page from one service operation so cursor validation precedes
all provider I/O; record-detail handlers retain two independently bounded operations. Zone handlers
execute at most one. Base Kubernetes Secret RBAC remains unchanged. Cloudflare exposes numbered
pages rather than a snapshot token, so continuation still performs a bounded rescan. Snapshot
persistence remains deferred until measured scale or quota evidence justifies separate SQL and
Kubernetes implementations.

Cloudflare API Tokens are preferred to legacy global API keys. AWS and Google adapters must
prefer automatically refreshed temporary credentials through the SDK provider chains.

Credential inspection orchestration lives in the provider-neutral runtime. It loads the current
ProviderAccount, resolves an inspector against that exact account authority, coalesces concurrent
requests by account ID and generation, and reuses both successful and fixed failed results for a
short, non-zero cooldown. One total deadline covers account lookup, follower waiting, concurrency
admission, asynchronous resolver work, and inspection; resolver implementations must not perform
blocking I/O. Returned identity must match both the configured provider and provider-native scope,
and a valid expiry must exceed completion time by the configured minimum skew. Cached valid results
stop being reusable before that expiry safety boundary, even when their cooldown is longer. Opaque
credential revision, provider principal, adapter issue codes/messages, credential references, raw responses,
and resolved values remain outside the Admin wire contract; the Admin identity contains only
provider and scope. The explicit refresh route is independently authorized by
`provider-credentials:inspect` and is mounted only when a service is composed. Both production
compositions leave the service absent by default, so this foundation causes no cloud egress and
adds no Kubernetes Secret permission.

## Provider capability contract

Provider capabilities are independent snapshots, not booleans embedded in `ProviderAccount`
status. A closed `ProviderCapability` family identifies portable DNS and WAF behavior. WAF
capabilities cover only managed rules, custom rules, and rate limiting; provider expressions and
protected-target references stay provider-specific. Each `CapabilityRequirement` also names an action
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

## Provider mutations

Retained provider-specific APIs make bounded synchronous direct calls. They keep their own
admission, deadlines, concurrency controls, audit records, exact receipts or tokens, and
`UnknownOutcome` handling. Center has no generic cloud-operation contract, durable operation
store, background worker, shared desired-resource reconciler, or operation CRD. Provider APIs
must not blindly replay an ambiguous external mutation; a later observation or explicit operator
decision establishes its outcome.

## Status and provider errors

Cloud conditions use unique typed condition keys, stable bounded UpperCamelCase reason codes,
bounded human-readable messages, observed generations, and non-decreasing transition times. A
condition is successful only when it is `True` at the exact desired generation; missing, stale, or
`Unknown` conditions fail closed. Semantically unchanged updates retain the original transition
time.

Provider adapters normalize sanitized failures into authentication, authorization, quota,
conflict, validation, not-found, transient, throttled, or unknown-outcome categories. The core maps
those categories to the retained sanitized direct-call `OperationError` projection and validates
again before conversion.
Raw provider bodies, headers, and credential material must not enter either model. Correlation IDs
and bounded time-ordered cloud events exist as core contracts; persistence, retention, API, and
end-to-end linkage between Center correlation IDs and provider request IDs remain future work.
Consumers must evaluate the requested condition at the exact desired generation and
must not treat the top-level observed generation alone as readiness.

## DNS provider contract

Cloud-provider Admin APIs remain provider-specific product surfaces. There is no unified DNS menu
or generic DNS control endpoint: Cloudflare, Route 53, and Google Cloud DNS expose their own
inventory and operation routes, permissions, capabilities, and dashboard entries. A future Region
failover workflow may call a selected provider-specific `switch-target` operation, but that caller
does not change ownership of the provider integration.

The first Cloudflare Admin API slice exposes only sanitized zone inventory through an SDK-free
application port. The port is optional and the capability defaults to disabled in both production
compositions. Until ProviderAccount and credential resolution can construct an account-bound
service, the Cloudflare routes are not mounted and no provider network request is possible.

The Cloudflare adapter exposes a separate account-bound zone-inventory seam because the portable
DNS contract intentionally omits Cloudflare zone kind, status, and authoritative nameservers.
Inventory cursors are authenticated and scope-bound, but their decodable payload contains only
versioned, method-specific, domain-separated HMAC scope tags; Center account IDs,
Cloudflare-native account IDs, zone IDs, and offline-verifiable plain hashes are never serialized
into a cursor. A production application service must still resolve a Center ProviderAccount and
its credential before constructing this adapter.

Cloudflare record inventory is modeled as canonical RRsets identified by owner name and record
type, not by one physical Cloudflare record ID. The provider-specific Admin DTO explicitly
projects typed record values, automatic or explicit TTL, proxy state, CNAME flattening, comment,
tags, all physical object IDs, and the canonical revision. TXT segments and CAA values remain
lossless octets encoded as canonical Base64URL. Record responses are checked against a separately
validated zone projection, preventing a provider service from rebinding a zone ID to another apex
or visibility.

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
canonical fingerprinting remains follow-up work. CLD-14 adds an independent DNS verifier; it is
not composed into a provider adapter or product binary.

The Cloudflare adapter is a separate account-bound crate. `ProviderAccountSpec` carries a typed
provider-native account scope, so a Center resource ID is never sent as a Cloudflare account ID.
Inventory exhausts Cloudflare physical-record pages before grouping by canonical RRset identity;
mixed member TTL, proxy, flattening, comments, or tags fail closed rather than being flattened into
false state. Provider object IDs remain observed metadata and the canonical revision hashes the
ordered complete member representation. HMAC-authenticated local cursors bind the exact Center
account, provider-native account, zone, list method, page size, and complete canonical inventory
using a stable composition-provided key. Cursor v4 additionally authenticates issue and expiry
times. A continuation fails with restart-required when that inventory changes. An optional
fallback key can verify pagination only; change receipts and lifecycle mutation IDs remain
active-only until CLD-35H5 gives them an independent, domain-separated
`cloudflare_dns_mutation_token_hmac` authority. Its active signer plus optional observation-only
fallback is an adapter reseal mechanism, not a production rotation protocol; H5 defines no receipt
time or durable retention semantics. Receipt observation is repeatable and side-effect free; a
receipt must never authorize another provider mutation. CLD-35H5 deliberately does not enable
writes. H5 does not promise payload confidentiality: a lifecycle receipt may authenticate only the
32-character Cloudflare zone ID needed for observation and, for create, the canonical apex. Raw
Center/native account identity, credential authority, requests, approvals, leases, and execution
fences remain forbidden; request and idempotency bindings are keyed tags. The adapter preflights
the worst-case lifecycle receipt size before mutation. Mounted credentials expose a resolved-
authority distinctness helper for file-identity and exact-material checks; exact resolve requests
and composer validation bind purpose and reference. Existing read and synchronous write
compositions resolve no mutation authority. If unattended automation is later required, deferred
CLD-35H6 migrates the minimal lifecycle locator into a
durable recovery descriptor and must first add authenticated receipt lifetime and rotation
retention plus a durable prepare/persist/execute/observe protocol: persist a sanitized recovery
descriptor before the single provider dispatch, reconcile every ambiguous outcome against real
provider state, and commit results only under the exact operation, step, attempt, and execution-token
fence. Mutation-key generations and ProviderAccount authority required by nonterminal operations
cannot be removed or have their native scope replaced.

CLD-35H7 adds a separate, independently default-off synchronous Zone-create composition. It only
accepts `Managed` ProviderAccounts, resolves their existing Cloudflare API token, issues exactly
one public/full Zone-create request, and returns the validated provider observation directly. It
does not resolve cursor or mutation-token keys, create an operation, persist recovery state, or
retry a mutation. A timeout, ambiguous transport result, malformed success, post-dispatch
authority change, or mismatched provider result is returned as `unknown_outcome`; callers must
read Cloudflare state before deciding whether another request is safe. Read and write permissions,
capabilities, concurrency limits, and configuration switches remain independent.

CLD-35H8 extends that same default-off synchronous write composition with provider-specific RRset
PUT and DELETE routes. Each request targets one canonical owner/type identity and carries either a
must-not-exist guard or the exact observed revision. The composition first observes the exact
public Zone and complete canonical RRset; the adapter repeats the fresh guard and submits exactly
one Cloudflare batch. Create/replace returns the authoritative post-mutation RRset, while delete
returns success only after absence is observed. A pre-batch timeout is unavailable; any timeout,
authority change, malformed result, mapping failure, or post-observation failure after batch
dispatch is an unknown outcome and is never retried automatically. SOA and apex delegation NS
remain outside the ordinary RRset authority. Comments and tags are representable only when all
physical RRset members agree. Only the explicitly enabled read inventory and synchronous write
services are composed into the binaries; all other adapter mutation surfaces remain unavailable.

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
Endpoint overrides are loopback-only test seams. Real-account tests remain separate from default
hermetic CI. The SDK-config factory can load the AWS ambient chain or wrap an
externally resolved base configuration in a refreshable AssumeRole provider; it validates all
non-secret role parameters, rejects inherited global endpoint overrides, redacts provider debug
output, and never resolves or persists Center secret references. A resolved external ID remains only
inside the in-memory AWS refresh provider. An ignored live-account suite uses an operator-provided
disposable public zone and performs exact create/replace/delete cleanup plus a deterministic stale
exact-batch race.

The first production Route 53 composition is the independent
`center-integration-route53` read slice. Its strict `route53_dns_read` switch is default-off and
mounts only AWS-specific hosted-zone and RRset list/get routes. It accepts an exact AWS
ProviderAccount with `CredentialSource::Ambient`; standalone uses the standard AWS chain and
Kubernetes uses IRSA or EKS Pod Identity. Every constructed client verifies its twelve-digit
account through STS before Route 53 reads. Static keys, explicit federation, AssumeRole, endpoint
overrides, writes, and lifecycle mutations are not part of this slice. A closed-purpose,
account-scoped `route53_dns_cursor_hmac` mounted key signs inventory cursors. One operation timeout
covers store access, per-account/global admission, local key resolution, SDK/STS construction,
provider reads, and exact account/key authority rechecks. Malformed cursors fail as invalid input;
an inventory-digest change returns restart-required. Alias targets, routing policy, set identifier,
health-check reference, values, TTL, and revision remain in the validated Route 53 record model.
Because Route 53 offers no RRset tag and Kubernetes audit is not revision-queryable, reads report
only `external_or_manual`; audit history is never presented as ownership state and no marker TXT or
Center metadata store is introduced.

The independent `route53_dns_write` slice adds synchronous guarded RRset mutation without making
the read surface or hosted-zone lifecycle implicit. It requires a `Managed` Ambient AWS account
and resolves two distinct Center-local authorities: the inventory cursor key and an RRset mutation
receipt key. A record-only adapter constructor cannot create/delete zones or operate DNSSEC.
Individual routes take owner, type, and optional set identifier from the URL and accept only a
guard plus provider-safe desired state; the zone-wide batch route uses explicit create, replace,
and delete actions and rejects duplicate identities. Before its one atomic Route 53 dispatch, the
adapter re-observes exact raw RRsets, checks revisions, validates the complete resultant affected
groups, and prevents replacement from implicitly changing routing family, Alias shape, or the
health-check reference. Timeout or authority change after possible dispatch returns
`unknown_outcome`; there is no automatic retry. Opaque receipts are HMAC-bound to both accounts and
the zone and are authenticated locally before AWS client construction or STS I/O. Observation
reports only Route 53 `PENDING` or `INSYNC`; authoritative convergence is
always `not_checked` in this slice. The mutation key is single-active: operators do not switch its
active binding until all receipts issued under it have settled or their observation window has
ended; retaining an unreferenced old file does not provide fallback verification.

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
required cache hold; adapters never turn signing off first.

The CLD-14 verifier is a separate adapter with no provider SDK, persistence, Admin API, dashboard,
federation, or Edgion dependency. Requests bind the provider account, zone, zone and record
revisions, expected RRset, exact nameserver set, resolver profile ID and revision, retry/query
budget, and evidence freshness window. Provider completion cannot construct DNS readiness. Public
readiness requires every provider-observed authoritative server to publish the exact A, AAAA,
CNAME, or TXT RRset, every configured recursive view to observe it, and a direct parent-authority
delegation check to return the exact NS set. Parent discovery may use the bound authority resolver,
but delegation evidence is accepted only after the target server proves authority for the parent
with an authoritative SOA response. Delegated-child verification binds a separate child apex and
child nameserver set and cannot update its parent zone's lifecycle readiness. Private and
split-horizon checks use only their explicitly bound resolver profile and never fall back to
ambient system DNS.

Network access is bounded and fail closed. Public authoritative targets are provider- or
delegation-derived nameserver addresses, pinned before connect, restricted to public unicast port
53, and rechecked before UDP and TCP exchanges. Private targets require an explicit CIDR and port
allowlist. Responses must match source endpoint, transaction ID, opcode, question, type, and class;
truncated UDP retries only the same endpoint over bounded TCP. CNAME verification compares the
direct RRset and never follows the target. Query attempts, backoff, per-query time, total time, and
evidence age are bounded. Timeout and budget exhaustion remain per-nameserver evidence rather than
an unscoped network error, and metrics expose only low-cardinality result classes.

DNSSEC validation uses explicit resolver endpoints with system configuration, search domains, and
hosts-file fallback disabled. Hickory validates locally from root trust anchors; a raw AD bit is
never promoted to local-chain evidence. Signed readiness requires the expected RRset and parent DS
set plus a secure local chain. Authenticated DS absence is kept distinct from an empty or failed
answer. DNSSEC validation reserves bounded logical operations and is also constrained by the
resolver's single attempt, per-lookup timeout, and the verifier's total deadline; Hickory's internal
DNSKEY/DS exchanges are not individually exported as propagation-query metrics.

## Historical origin and general-edge design (retired)

The remainder of this section records the removed CLD-23 through CLD-26 design for historical
context only. `OriginPool`, Cloudflare Origin Rules, and Cloudflare Load Balancing are not compiled,
exported, composed, or part of the current product. New work must not depend on these contracts.

`OriginPool` is an independent public-infrastructure resource. It does not contain Controller,
cluster, Edgion Region, Gateway, HTTPRoute, or RegionRoute references. A future integration resource
may attach one of those identities to a pool, but provider adapters and the base pool remain useful
without Edgion.

Each origin has a stable validated name, hostname or IP destination, explicit protocol and port,
independent HTTP Host and TLS SNI values, TLS verification mode, relative weight, priority tier,
and active, draining, or disabled administration state. Non-secret headers may be literal; secret
headers are represented only by a composition-resolved credential reference. Health/status models
never contain either header values or resolved secret material.

Active health intent includes HTTP method/path when applicable, headers, expected status codes and
optional bounded body match, interval, timeout, consecutive healthy/unhealthy thresholds, and a
provider-default or explicit source-region scope. Provider adapters must validate the current
source catalog and entitlement rather than accepting a syntactically valid region as available.
Host/SNI/header fields on non-HTTP or non-TLS protocols fail before provider calls.

Provider health and Center-observed health are separate evidence streams. Observations bind the
endpoint and source, advance monotonically, retain consecutive success/failure counters, and have an
exclusive freshness deadline. Threshold hysteresis controls state transitions; missing, unknown, or
stale evidence never counts as healthy capacity. Portable selection either sends new traffic to all
healthy active origins or to the first priority tier with sufficient healthy capacity. Draining and
disabled origins remain observable but receive no new portable selection.

Cloudflare Origin Rules use the zone `http_request_origin` phase and single-rule `route` mutations.
Center must never update the whole phase ruleset because that would replace omitted user rules.
Owned rules accept a bounded hostname and exact/prefix path match rather than arbitrary expressions,
render Host, SNI, same-account DNS hostname, and port overrides explicitly, and use provider rule ID,
stable ref, scope, version, observed revision, and stored ownership proof together. A marker alone is
not ownership. Overlapping owned rules that write different values fail planning unless the later
rule explicitly names the rule it overrides. Unowned rules remain opaque and retain their relative
order. Delete plans expose whether a prior rule or provider default will become effective and require
explicit acknowledgement when traffic may change. Trace is optional, safe GET/HEAD-only evidence;
local preview reports that it is incomplete whenever opaque provider rules may participate.

Cloudflare Load Balancing maps each portable priority tier to a distinct account-scoped pool because
a Cloudflare pool origin has no priority or independent SNI field. The zone load balancer orders those
pools in `default_pools` and references an explicit fallback pool. Weight conversion is exact or
fails; Host and SNI that differ require an Origin Rule and are not silently collapsed. Provider
monitor, pool, load-balancer, health, region, quota, and entitlement observations stay typed and
account/zone scoped.

The first transport mapping is deliberately HTTPS-only. A fresh scope- and credential-bound proof
must show Zone SSL Routing, Full Strict at the same settings revision, and Always Use HTTPS so a
visitor HTTP request is redirected before origin routing. Full Strict alone verifies certificates
for HTTPS origin connections but does not prove that every request uses HTTPS. Origin Rules cannot
serve as protocol-selection or TLS-verification evidence.

Load-balancer rollout is expand, verify, then contract: create or reuse a content-bound monitor, add
new pools/origins, verify sufficient provider health, attach them without removing old capacity,
then drain and detach old objects. Cloudflare drain requires a proxied load balancer, session
affinity, a drain duration, and disabling the origin; unsupported drain fails closed. Monitor and
pool deletion requires both Center reverse-reference proof and an empty Cloudflare references
response. Provider write ambiguity is `UnknownOutcome`, never an automatic retry. Public API plan,
quota, region, and steering limitations are capability evidence; an unknown create quota cannot be
treated as spare capacity.

## Historical general CloudFront design (retired)

The detailed material below records the removed broad CloudFront planner. The retained adapter is
read-only Distribution inventory plus a private raw-wire round-trip seam. CLD-28F will separately
add a fixed one-origin API Distribution lifecycle, and CLD-29A will add an exact `WebACLId`
association write set. Ordered behaviors, origin groups, failover, ACM/domain orchestration,
invalidations, and general CDN management are not current contracts.

CloudFront is modeled as a distribution rather than a DNS zone. Route 53 owns hosted zones and
alias RRsets; a distribution owns its provider domain, origins, origin groups, ordered cache
behaviors, alternate domain names, deployment state, and invalidations. The first delivery slice
supports public custom HTTP(S) origins. A syntactically valid DNS hostname or an ambient resolver
answer is not proof of that boundary: a plan requires fresh, scope-bound public-origin approval for
the exact provider account, distribution observation, hostname, and protocol intent. Ordinary
public origins and trusted-classified public AWS custom endpoints are allowed; S3/OAC, private and
VPC origins, functions, WAF, continuous deployment, and policy authoring remain outside the
contract and cannot be silently represented as public custom origins. The first slice supports
only `http-only` and `https-only`; `match-viewer` remains unsupported.

Persistent inventory retains a sanitized typed projection, opaque ETag, account/partition and
credential authority, observation freshness, mutation eligibility, and a keyed MAC of the opaque
ETag revision. The MAC is a scope-binding token, not a configuration content hash. Inventory never
persists raw XML, a complete SDK configuration, or Origin Custom Header names or values; only a
redacted count crosses the adapter seam. When a custom
header is an origin access credential, its name and value form one composition-resolved secret;
neither field may enter desired state, status, plans, events, logs, debug output, provider errors,
or API projections. Validation identifies only the secret reference or collection position.
CloudFront updates replace a full configuration, so CLD-28B emits only observation-bound origin and
origin-group fragments with no dispatch authority. CLD-28F re-reads the complete config and ETag
into one bounded in-memory mutation window, overlays an authorized fragment, preserves unsupported
and unowned fields, submits once, and discards the sensitive object.
If unknown fields cannot be detected or safely preserved, the resource is mutation-ineligible.
CLD-28F now performs a private live re-read and an `Enabled`-only overlay preview. The same bounded
GET operation captures its raw response after deserialization. A serializer probe captures the
current and desired SDK request bodies and raises a typed interceptor abort before identity,
signing, or transmit. Strict ordered, namespace-aware comparison rejects any raw/SDK mismatch and
proves that the desired wire changes only the root `Enabled` scalar. A successful proof removes
only that plan's wire-schema and full-config-revision blockers; ownership, approval, reliability,
secret-memory zeroization, and executor blockers remain, and no dispatch method is exposed.
The secret-bearing SDK configuration is consumed and dropped within the planner rather than
returned to its caller. Preview fields are private, and its intent MAC binds the logical provider
account, generation, credential revision, AWS account and partition, distribution, ETag, and
desired enablement state; the MAC is still not mutation authority.
A persistable ownership claim and approval record now bind that composite plan revision to the
exact Center resource, ownership revision, action, risk, AWS scope, and freshness window. A joint
verifier must validate both records from one authoritative snapshot or transaction. The resulting
sealed preauthorization also binds the fresh inventory observation token and earliest evidence
deadline, but remains deliberately non-serializable and non-dispatchable. It removes no planner
blocker; durable storage, one-time approval consumption, and an operation-fence check immediately
before provider dispatch are still required.
A `cfg(test)`-only `UpdateDistribution` protocol harness specifies a separate one-attempt SDK
client and its error contract. Explicit ETag rejection requires a replan; deterministic `4xx`
responses are terminal or explicitly throttled; ambiguous transport failures, `408`, `5xx`,
malformed success, or response identity/config drift are `UnknownOutcome`. A valid response proves
only provider acceptance, never deployment. Production builds contain no CloudFront mutation
client because the SDK request body can contain credential-bearing custom headers and must not be
sent until a secret-safe logging boundary and sealed authority exist. The SDK version is exactly
pinned for reproducibility, but this does not replace runtime full-wire preservation proof and
cannot by itself remove preview blockers; only the live raw-versus-serialized admission can remove
the two wire-related blockers for that exact ETag.
Creation is composed only after a validated origin and default cache behavior exist. Update,
enable, disable, and delete use the latest observed ETag and remain pending until a fresh
observation reports the expected configuration as deployed. Ambiguous writes become
`UnknownOutcome` and are resolved by observation rather than blind replay.

CLD-28B planning consumes a non-deserializable live-inventory handle, not a persisted inventory
DTO. Public-origin resolver/classifier inputs and their approval minter remain sealed inside the
adapter until a trusted composition resolver is wired. CLD-23 contributes only a conservative
endpoint shape: weight one, priority zero, active state, verified TLS, no portable health check,
minimum healthy one, and priority-tier mode. No CLD-23 load-balancing or health semantics are
silently translated to CloudFront.

An origin group has exactly one primary and one secondary origin plus bounded failover status
codes. It is not a weighted load balancer. CloudFront failover applies only to viewer `GET`, `HEAD`,
and cacheable `OPTIONS` requests. AWS may route a mutating request to the primary member without
failing it over, but Center deliberately rejects a behavior that combines an origin group with
`POST`, `PUT`, `PATCH`, or `DELETE`. Plans, APIs, and UI must describe this as a stricter Center
safety policy, not as an AWS API restriction, and must never claim failover for mutating methods.

Reverse-reference inspection for an origin or origin group is impact diagnosis only. Even a fresh
empty result does not prove provider ownership, override `DeletionPolicy::Retain`, authorize the
caller, acknowledge traffic impact, or construct deletion authority. CLD-28F must independently
require all ownership, policy, authorization, freshness, and approval fences before removal.

Cache behaviors retain CloudFront first-match ordering and reference observed AWS-managed or
pre-existing cache, origin-request, and response-header policies. Center does not author those
policies in the first slice. Policy planning authority comes only from a live, scope-filtered List
followed by an exact Get for every referenced policy; the sealed observation binds policy kind,
managed/custom scope, ID, ETag, modification time, AWS account and partition, credential revision,
and freshness. A sanitized policy DTO or persisted inventory cannot authorize a plan.

The initial behavior planner is append-only: it preserves every observed ordered behavior and
places new behavior fragments after them. It exposes no default replacement, existing behavior
replacement/deletion, or reorder operation until CLD-05 ownership/adoption authority exists. An
append can still divert requests that previously reached the default behavior, so this impact is
explicit in the plan and is not mutation approval. Managed path patterns use a conservative exact
or single trailing-wildcard subset. Local preview uses first-match order after RFC 3986 dot-segment
normalization, preserves repeated slashes, is restricted to the observed provider hostname or an
alias, and is always labeled as a local projection. An origin group preview reports its primary
and conditional secondary plus eligible failover codes; it never claims that the secondary was
actually selected. The fragment and plan carry no dispatch authority, and the
`wire_schema_not_lossless` guard remains a mutation blocker until CLD-28F.

Alternate domains consume an externally managed ACM ARN plus fresh, sealed account,
commercial-partition, `us-east-1`, certificate-status/type/key, validity, SAN coverage, exact
effective-hostname-set, and distribution-revision evidence. Center uses ACM
`DescribeCertificate` only and never retrieves certificate material or owns request, import,
renewal, export, rotation, or deletion. The conservative first subset accepts only
`AMAZON_ISSUED` certificates that are not ACM-managed for CloudFront and additive exact-hostname
distribution aliases; wildcard SANs may cover exact hostnames, but wildcard distribution aliases
remain unsupported until inventory can represent them safely. A certificate already used by a
different distribution is rejected until fresh evidence can prove compatible supported HTTP
versions across all consumers. Exact aliases must enter the adapter in lowercase ASCII/A-label
form; implicit Unicode-to-Punycode conversion is not part of the initial contract.

Route 53 aliases use the freshly observed distribution domain and sealed, short-lived evidence
from a composition-owned, versioned AWS endpoint catalog for the CloudFront alias hosted-zone ID.
The initial production source is a strictly parsed, checked-in catalog artifact with a fixed source
identity and an exact-byte SHA-256 revision; ordinary configuration, environment, or Admin input
cannot replace its values. Observation time, rather than the static artifact, mints the five-minute
evidence window.
There is no hard-coded, request-supplied, or distribution-derived fallback. The initial subset is
commercial AWS only and emits simple public alias desired state: A always, AAAA only when the
distribution's observed IPv6 setting is enabled, inherited TTL, empty ordinary values, and
`EvaluateTargetHealth=false`. The Route 53 zone may be in another AWS account, but every requested
alias must have exactly one zone binding. Planning stages distribution attachment, deployed-state
observation, Route 53 submission, `INSYNC` observation, and CLD-14 authoritative/recursive
verification separately. AWS domain-conflict lookup requires the validation Distribution to
already have a certificate covering the queried hostname, so the safe sequence is certificate-only
attachment, deployed-state observation, complete bounded `ListDomainConflicts` scans for every new
exact alias, Alias attachment, and a second deployed-state observation. Any returned item—including
wildcard overlap, Distribution Tenant, or a partially masked cross-account identity—blocks the
plan; this slice never migrates or takes over an Alias. Empty-scan evidence binds the Distribution
ETag and credential authority, certificate ARN, exact new-hostname set, and a five-minute window,
and contains no foreign identifiers. Until ownership/adoption, exact DNS revision, approval, and
both mutation executors are composed, this plan has no dispatch authority. Serialized plans
retain the Route 53 provider-account and hosted-zone scope for every alias group so later ownership
and exact-revision evidence cannot be rebound to another zone.

Invalidations are durable operations with stable CallerReference identities. Accepted and
in-progress states are not completion. Paths, wildcard suffixes, quotas, costs, and broad-impact
approval are validated before submission; ambiguous dispatch is observed by CallerReference or
provider invalidation identity before any replay.

The first invalidation slice is read/plan/reconciliation only. It exposes bounded GET-only List
and Get transport and performs an exact Get for every list summary before sealing a complete
distribution-scoped inventory. Provider-form paths are kept case- and byte-sensitive: Center does
not percent-decode, remove dot segments, collapse slashes, or otherwise rewrite cache identity.
Provider reads preserve bounded external query-string, tag, duplicate, and literal-mid-wildcard
items as opaque observations; desired-state validation is intentionally separate. The conservative
initial desired subset rejects query strings, raw non-ASCII, tilde, unnecessary percent encoding,
and non-suffix wildcards. Targeted paths retain an explicit query-variant coverage blocker until
cache-policy evidence proves completeness; a suffix wildcard is the conservative way to cover
query variants. `/*` is available only through an explicit all-path intent. Canonical sorting and
deduplication happen before the request digest; CallerReference binds distribution, operation
identity, and that digest. Reconciliation accepts a provider invalidation only when both
CallerReference and the complete canonical path vector match. Reconciliation uses the current
fresh account/distribution scope rather than requiring the old plan ETag to remain fresh. A
complete non-snapshot scan that finds no match is still indeterminate under concurrent creation or
provider visibility delay and never authorizes a new CallerReference. Plans always report possible billing and missing ownership, approval,
quota, operation-binding, and executor authority; no CreateInvalidation transport exists in this
slice.

## Historical lifecycle examples (retired)

The examples below describe the removed unified exposure model and are non-normative. Current DNS
and future WAF workflows use their provider-specific APIs and menus directly.

### Import and observe a DNS zone

1. Create a `ProviderAccount` containing only a credential reference.
2. Discover a provider zone and create a `ManagedZone` with `ObserveOnly` and `Retain`.
3. Record its provider ID in status after observation.
4. Do not mutate records until a later explicit adoption plan succeeds.

### Create a standalone public domain

1. Create or adopt a `ManagedZone`.
2. Create an `OriginPool` independent of Edgion.
3. Create an `EdgeApplication` referencing the origin pool.
4. When the selected edge provider requires it, attach an owner-supplied external certificate
   reference and fresh hostname/region compatibility evidence.
5. Create a `DomainBinding` joining the zone, edge application, and pool.
6. A later reconciler plans and applies child provider resources in dependency order without
   taking ownership of certificate lifecycle.

### Delete a domain without deleting shared infrastructure

1. Plan removal and calculate reverse references.
2. Detach the hostname from the edge application without deleting the external certificate.
3. Remove only owned DNS records created for the binding.
4. Retain shared origin pools, external certificate references, zones, and provider accounts.
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
