# Cloudflare mounted credential example

This non-default overlay enables the local mounted-file resolver, Cloudflare credential
inspection, read-only DNS inventory, synchronous DNS writes, and Zone WAF inventory/mutations.
The base deployment and its RBAC remain unchanged:
Kubernetes mounts one explicitly named Secret through the kubelet, and Center receives no
permission to get, list, or watch Secret objects.

The configured Cloudflare API token must have the provider permissions needed by the explicitly
enabled operations. DNS record changes require DNS Edit; zone creation requires Zone Edit. Center
does not probe mutation permissions or send provider requests at startup.

Create exact-byte files locally and create the Secret in the deployment namespace:

```sh
head -c 32 /dev/urandom > revision.key
head -c 32 /dev/urandom > cloudflare-dns-cursor-a.key
head -c 32 /dev/urandom > cloudflare-dns-cursor-b.key
head -c 32 /dev/urandom > cloudflare-waf-owner-a.key
head -c 32 /dev/urandom > cloudflare-waf-owner-b.key
printf %s "$CLOUDFLARE_API_TOKEN" > cloudflare-api-token
kubectl -n edgion-system create secret generic edgion-center-cloudflare-credentials \
  --from-file=revision.key \
  --from-file=cloudflare-dns-cursor-a.key \
  --from-file=cloudflare-dns-cursor-b.key \
  --from-file=cloudflare-waf-owner-a.key \
  --from-file=cloudflare-waf-owner-b.key \
  --from-file=cloudflare-api-token
```

The revision, DNS cursor, and WAF ownership keys must each contain exactly 32 non-zero bytes. The
two WAF ownership keys must differ from one another, both cursor keys, the revision key, and the
API token. Every completed rollout stage
must give all Center replicas the same exact key material and assignment; during the Stage-A to
Stage-B rollout, both keys remain present while the active assignment intentionally differs.
Credential material must be non-empty, no larger than 16 KiB, regular, and not group- or
world-writable. Secret volumes are root-owned, while Center runs as UID 1000. The overlay uses
`fsGroup: 1000` to make the six explicitly named `0440` source files readable by a non-root init
container. That container copies them into a memory-backed `emptyDir` it owns and restricts the
staged files to `0400`. The Center container mounts only that staging volume read-only; the resolver
opens its private `0700` child as the directory capability. No root container, added Linux
capability, or Secret API permission is involved.

Staging is idempotent across init-container retries. Each source is copied to a private temporary
file, restricted, and renamed into place; Center cannot start while a staging attempt is incomplete.

The resolver itself also supports Kubernetes projected `..data` symlink rotation when a deployment
mounts a compatible projected volume directly and every target remains beneath the configured
root. This example stages credentials once per Pod, so Secret rotation takes effect on Pod restart.

Update the example account ID and credential reference to exactly match the corresponding
`ProviderAccount`, then render or apply the overlay:

```sh
kubectl kustomize cicd/deploy/examples/cloudflare-mounted-credentials
kubectl apply -k cicd/deploy/examples/cloudflare-mounted-credentials
```

An authorized explicit credential-inspection request now verifies the token against Cloudflare and
performs an account-scoped `zones?page=1&per_page=1` probe. No request is made at startup. An empty
zone page cannot prove account scope and therefore returns `Unknown`, not `Valid`. The production
endpoint is fixed to `https://api.cloudflare.com`; configuration cannot redirect token material.

The DNS service uses the same account-bound token without caching clients. One operation deadline
covers account lookup, concurrency admission, credential reads, every provider page, and authority
rechecks. Zone and record-list HTTP requests execute at most one operation. Record detail retains
a separate authoritative-zone operation before the record operation and can therefore consume two
sequential operation deadlines. `cloudflare-dns:read` is a high-trust permission that can observe
every configured Cloudflare account; it is not scoped to one account.

The write service is independently default-off and uses only the ProviderAccount API-token
binding already shown above. It does not resolve either cursor key or a mutation-token key, create
a durable operation, or retry a provider mutation. The per-account concurrency default is one.
If a response is lost after Cloudflare may have accepted a request, Center returns a fixed unknown
outcome and the caller must read the target state before deciding whether another write is safe.

The `cloudflare_waf` switch independently controls its read and write routes while reusing the
same exact ProviderAccount API-token binding. Each WAF operation has one deadline, one global and
per-account admission slot, and fresh pre/post account and token-revision checks. WAF writes first
observe the exact Zone and entry-point revision, preserve unowned rules, dispatch at most one
Cloudflare Rulesets mutation, and return `unknown_outcome` if a dispatched request or authority
check is ambiguous. The token needs the Cloudflare Zone WAF permissions appropriate to the enabled
operations; Center performs no provider request at startup.

`ownership_key_ref` is a separate active 32-byte HMAC key for WAF rule ownership. Center writes a
versioned compact `ref` binding the Center ProviderAccount, Cloudflare Zone, WAF phase, and the
user-visible reference; a matching prefix alone is never ownership proof. The optional
`ownership_fallback_key_ref` verifies existing bindings during key rotation but never signs new
rules. Existing Center-owned rules signed by either key remain readable; forged, simultaneous
duplicate, wrong-account, wrong-zone, wrong-phase, or opaque rules are never mutable. Cloudflare
WAF writers are trusted co-administrators: a signed reference is a bearer proof of Center origin,
not immutable rule identity, so Center cannot detect a copy transplanted after deleting the
original without a durable ownership registry. Rotate the key in the same three stages as the DNS
cursor keys: add B as fallback, promote B to active with A fallback, then remove A only after all
replicas running A are gone and operators have finished any required WAF inventory refreshes.

The same write switch exposes direct provider-specific RRset PUT and DELETE on
`.../zones/{zone_id}/record-sets/{record_type}?owner=...`. PUT uses either `must_not_exist` or an
exact observed revision; DELETE always requires the exact observed revision. Center refreshes the
Zone and complete canonical RRset before one Cloudflare batch and observes the result afterward.
SOA and apex delegation NS changes remain unavailable. Guard conflicts issue no mutation, and
Center performs no automatic retry when a dispatched result is unknown.

Automation that needs a visible remote-control provenance hint uses the dedicated
`PUT .../record-sets/{record_type}/remote-control?owner=...` route and the independent
`cloudflare-dns:remote-write` permission. Center derives an opaque caller alias from validated
authentication claims and writes its reserved tag in the same record batch. Requests cannot
choose that alias or submit any case-insensitive `edgion-center-` tag. Ordinary guarded PUT
removes an earlier remote marker and returns the RRset to `manual` control. The marker is a display
hint only; it is not provider-resource ownership or authorization evidence.

Pagination cursors bind the page size and a keyed tag of the complete canonical inventory. When
Cloudflare data changes between pages, Center returns `409 pagination_restart_required`; clients
restart from the first page rather than combining different inventory versions. Continuations
remain bounded rescans and are not provider point-in-time snapshots.

`cursor_key_ref` is the only signing key. `cursor_fallback_key_ref`, when present, is
verification-only. New cursors are never signed by the fallback key. Cursors authenticate their
issue and expiry times; this example permits a 900-second lifetime and 30 seconds of clock skew.
The complete key ring is resolved and its credential revisions are checked before and after every
provider operation.

Rotate keys with three deployments, using the same Secret and ConfigMap revision for every replica:

1. Stage A: keep key A active and add key B as fallback. Complete the rollout before promotion.
2. Stage B: set key B active and key A fallback. During this rollout, Stage-A and Stage-B replicas
   can verify cursors produced by one another. Rolling back to Stage A remains safe while both keys
   are present.
3. Stage C: after the last Stage-A Pod has terminated, wait at least
   `cursor_max_lifetime_secs + 2 * cursor_clock_skew_secs` (960 seconds with this example). Then
   remove key A as fallback, remove its mounted binding and file, and complete another rollout.

The wait starts after the last old-active Pod terminates because that Pod can sign with key A until
shutdown. Removing A earlier makes still-valid cursors fail closed and requires clients to restart
pagination. This staged example reads the Secret only in the init container, so changing Secret
data without restarting Pods does not rotate their in-memory key ring. The deployment still uses a
kubelet-mounted, explicitly named Secret and does not add permission to get, list, or watch Secret
objects.
