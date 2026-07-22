# AWS Route 53 ambient identity example

This non-default overlay enables only the AWS-specific Route 53 read Admin API. AWS credentials
come from the SDK ambient chain. On EKS, replace the example ServiceAccount annotation with an IRSA
role (or remove it when EKS Pod Identity supplies the role). No AWS access key or secret key is
stored in the Kubernetes Secret, ConfigMap, ProviderAccount, or Center database.

The named Secret contains only two Center-local keys: the mounted-file revision authority and the
Route 53 inventory cursor HMAC key. Create exact 32-byte, non-zero, distinct values:

```sh
head -c 32 /dev/urandom > revision.key
head -c 32 /dev/urandom > route53-dns-cursor.key
kubectl -n edgion-system create secret generic edgion-center-route53-local-keys \
  --from-file=revision.key \
  --from-file=route53-dns-cursor.key
```

Create a managed AWS ProviderAccount named `aws-main` with a twelve-digit AWS account scope and
`credentialSource.type: ambient`. At request time Center loads the ambient SDK identity, calls STS
`GetCallerIdentity`, and rejects the operation unless the returned account exactly matches that
scope. Production configuration cannot override STS or Route 53 endpoints.

The overlay enables the service but deliberately does not grant any human or automation caller
access. Apply an exact-scope binding based on the `edgion-center-route53-dns-reader` example in
`cicd/deploy/center-kubernetes/access-example.yaml`, replacing its account, Zone ID, and subject.
The caller needs both the exact non-resource GET paths and `route53-dns:read` discovery permission.

The IAM role needs read-only Route 53 discovery for this slice: `route53:ListHostedZones`,
`route53:GetHostedZone`, and `route53:ListResourceRecordSets`, plus `sts:GetCallerIdentity`.
Record mutation and hosted-zone lifecycle permissions are not needed until CLD-35I2/I3 are enabled.

Render and validate the overlay before applying it:

```sh
kubectl kustomize cicd/deploy/examples/aws-route53-ambient
kubectl apply --dry-run=client -k cicd/deploy/examples/aws-route53-ambient
```

The service is independently default-off, bounded by one operation timeout plus global and
per-account admission, and exposes only public hosted zones. It never creates a shared DNS object,
persists an operation, infers a remote-control owner from audit history, or couples to Edgion
Region, Controller, Gateway, or federation resources.
