# AWS Route 53 ambient identity example

This non-default overlay enables Route 53 DNS and hosted-zone lifecycle. AWS credentials come from
the SDK ambient chain. On EKS,
replace the example ServiceAccount annotation with an IRSA role (or remove it when EKS Pod Identity
supplies the role). No AWS access key or secret key is stored in the Kubernetes Secret, ConfigMap,
ProviderAccount, or Center database.

The named Secret contains only Center-local HMAC/revision keys. Create exact 32-byte, non-zero,
mutually distinct values:

```sh
head -c 32 /dev/urandom > revision.key
head -c 32 /dev/urandom > route53-dns-cursor.key
head -c 32 /dev/urandom > route53-dns-mutation.key
head -c 32 /dev/urandom > route53-zone-lifecycle.key
kubectl -n edgion-system create secret generic edgion-center-route53-local-keys \
  --from-file=revision.key \
  --from-file=route53-dns-cursor.key \
  --from-file=route53-dns-mutation.key \
  --from-file=route53-zone-lifecycle.key
```

Create a managed AWS ProviderAccount named `aws-main` with a twelve-digit AWS account scope and
`credentialSource.type: ambient`. At request time Center loads the ambient SDK identity, calls STS
`GetCallerIdentity`, and rejects the operation unless the returned account exactly matches that
scope. Production configuration cannot override STS or Route 53 endpoints.

The overlay enables the services but deliberately does not grant any human or automation caller
access. Apply exact non-resource URL bindings for the required read/write and elevated permissions.
Keep Route 53 zone lifecycle authority separate from ordinary DNS writes.

The IAM role needs `sts:GetCallerIdentity` plus Route 53 inventory/change actions and hosted-zone
create/delete. Restrict resources and tags where AWS supports it.

Render and validate the overlay before applying it:

```sh
kubectl kustomize cicd/deploy/examples/aws-route53-ambient
kubectl apply --dry-run=client -k cicd/deploy/examples/aws-route53-ambient
```

Every service is independently default-off and bounded by one operation timeout plus global and
per-account admission. Writes require a `Managed` AWS ProviderAccount and are never automatically
retried after possible dispatch. A timeout after dispatch returns `unknown_outcome`; read provider
state before deciding whether a retry is safe. Do not rotate a receipt or lifecycle key until all
replicas use the same assignment and outstanding observations are complete.

Optional provider outages do not affect Center readiness. Disabling a section removes its routes
and creates no provider client or background polling. Shutdown may cancel work before dispatch, but
an already-dispatched mutation remains an observation/reconciliation responsibility. The example
does not configure an outbound proxy or custom provider CA: AWS SDK traffic uses public AWS
endpoints and the system trust store. Enforce egress with a workload NetworkPolicy/firewall and
permit only STS and Route 53 endpoints required by the enabled scopes. FIPS mode
is not claimed by this example; use a validated build and crypto policy if it is a deployment
requirement.

This integration remains independent of Edgion Region, Controller, Gateway, and federation
resources.
