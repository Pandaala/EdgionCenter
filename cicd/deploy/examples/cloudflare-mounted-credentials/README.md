# Cloudflare mounted credential example

This non-default overlay enables the local mounted-file resolver, Cloudflare credential
inspection, and read-only DNS inventory. The base deployment and its RBAC remain unchanged: Kubernetes mounts one explicitly
named Secret through the kubelet, and Center receives no permission to get, list, or watch Secret
objects.

Create exact-byte files locally and create the Secret in the deployment namespace:

```sh
head -c 32 /dev/urandom > revision.key
head -c 32 /dev/urandom > cloudflare-dns-cursor.key
printf %s "$CLOUDFLARE_API_TOKEN" > cloudflare-api-token
kubectl -n edgion-system create secret generic edgion-center-cloudflare-credentials \
  --from-file=revision.key \
  --from-file=cloudflare-dns-cursor.key \
  --from-file=cloudflare-api-token
```

The revision and DNS cursor keys must each contain exactly 32 non-zero bytes. The cursor key must
be different from both the revision key and API token and identical on every Center replica.
Credential material must be non-empty,
no larger than 16 KiB, regular, and not group- or world-writable. Secret volumes are root-owned,
while Center runs as UID 1000. The overlay uses `fsGroup: 1000` to make the three explicitly named
`0440` source files readable by a non-root init container. That container copies them into a
memory-backed `emptyDir` it owns and restricts the staged files to `0400`. The Center container
mounts only that staging volume read-only; the resolver opens its private `0700` child as the
directory capability. No root container, added Linux capability, or Secret API permission is
involved.

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
rechecks. Zone HTTP requests execute at most one operation. Record HTTP requests deliberately run
an authoritative-zone operation before the record operation and can therefore consume two
sequential operation deadlines. `cloudflare-dns:read` is a high-trust permission that can observe
every configured Cloudflare account; it is not scoped to one account.

This first slice has one active cursor key and no fallback key. Coordinated replacement must update
all replicas together and invalidates cursors issued with the previous key; clients then restart
pagination from the first page. Overlap-window cursor-key rotation is tracked separately.
