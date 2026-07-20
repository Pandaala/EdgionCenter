# Cloudflare mounted credential example

This non-default overlay demonstrates the local mounted-file resolver. It does not enable
credential inspection, a cloud API, or provider network traffic. The base deployment and its
RBAC remain unchanged: Kubernetes mounts one explicitly named Secret through the kubelet, and
Center receives no permission to get, list, or watch Secret objects.

Create exact-byte files locally and create the Secret in the deployment namespace:

```sh
head -c 32 /dev/urandom > revision.key
printf %s "$CLOUDFLARE_API_TOKEN" > cloudflare-api-token
kubectl -n edgion-system create secret generic edgion-center-cloudflare-credentials \
  --from-file=revision.key \
  --from-file=cloudflare-api-token
```

The revision key must contain exactly 32 non-zero bytes. Credential material must be non-empty,
no larger than 16 KiB, regular, and not group- or world-writable. Secret volumes are root-owned,
while Center runs as UID 1000. The overlay uses `fsGroup: 1000` to make the two explicitly named
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

Changing this configuration currently only proves that the resolver can be constructed safely;
the resolver is deliberately not connected to `ApiState`, the credential inspector, or a
Cloudflare client in CLD-02A.
