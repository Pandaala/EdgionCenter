# AWS Route 53 ambient identity example

This non-default overlay enables the AWS-specific Route 53 read and synchronous RRset write Admin
APIs. AWS credentials
come from the SDK ambient chain. On EKS, replace the example ServiceAccount annotation with an IRSA
role (or remove it when EKS Pod Identity supplies the role). No AWS access key or secret key is
stored in the Kubernetes Secret, ConfigMap, ProviderAccount, or Center database.

The named Secret contains only three Center-local keys: the mounted-file revision authority, the
Route 53 inventory cursor HMAC key, and the purpose-separated RRset mutation-receipt HMAC key.
Create exact 32-byte, non-zero, mutually distinct values:

```sh
head -c 32 /dev/urandom > revision.key
head -c 32 /dev/urandom > route53-dns-cursor.key
head -c 32 /dev/urandom > route53-dns-mutation.key
kubectl -n edgion-system create secret generic edgion-center-route53-local-keys \
  --from-file=revision.key \
  --from-file=route53-dns-cursor.key \
  --from-file=route53-dns-mutation.key
```

Create a managed AWS ProviderAccount named `aws-main` with a twelve-digit AWS account scope and
`credentialSource.type: ambient`. At request time Center loads the ambient SDK identity, calls STS
`GetCallerIdentity`, and rejects the operation unless the returned account exactly matches that
scope. Production configuration cannot override STS or Route 53 endpoints.

The overlay enables the services but deliberately does not grant any human or automation caller
access. Apply exact-scope bindings based on the `edgion-center-route53-dns-reader` and
`edgion-center-route53-dns-writer` examples in `cicd/deploy/center-kubernetes/access-example.yaml`,
replacing their account, Zone ID, record type, and subject. The minimal writer deliberately does not
receive the zone-wide atomic batch path or hosted-zone lifecycle authority.

The IAM role needs `route53:ListHostedZones`, `route53:GetHostedZone`,
`route53:ListResourceRecordSets`, `route53:GetChange`, and
`route53:ChangeResourceRecordSets` restricted to the intended hosted zones, plus
`sts:GetCallerIdentity`. Hosted-zone lifecycle permissions are not part of CLD-35I2.

Render and validate the overlay before applying it:

```sh
kubectl kustomize cicd/deploy/examples/aws-route53-ambient
kubectl apply --dry-run=client -k cicd/deploy/examples/aws-route53-ambient
```

Both services are independently default-off and bounded by one operation timeout plus global and
per-account admission. Writes require a `Managed` AWS ProviderAccount, use one atomic Route 53
change batch, and are never automatically retried after possible dispatch. Do not replace or switch
the active mutation-receipt key or binding until every receipt issued with it has reached `INSYNC`
or its observation window has ended. Merely keeping an old file mounted does not provide fallback;
this slice intentionally has no multi-key receipt rotation. It never creates a
shared DNS object, persists an operation, infers a remote-control owner from audit history, or
couples to Edgion Region, Controller, Gateway, or federation resources.
