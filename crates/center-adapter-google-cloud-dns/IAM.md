# Google Cloud DNS IAM boundary

The adapter reads managed zones and RRsets, submits one atomic `Change`, and polls that change. A
least-privilege custom role therefore needs these permissions:

```text
dns.managedZones.get
dns.managedZones.list
dns.resourceRecordSets.list
dns.resourceRecordSets.create
dns.resourceRecordSets.update
dns.resourceRecordSets.delete
dns.changes.create
dns.changes.get
```

Grant the custom role at the project or managed-zone boundary appropriate for the deployment. The
predefined `roles/dns.reader` role is sufficient for inventory-only use. `roles/dns.editor` supports
the adapter's write path but is broader than the custom permission set above. Do not grant a Cloud
DNS service-agent role to the Center workload.

Use Application Default Credentials. On Google Cloud, attach a user-managed service account with
the least-privilege role. Outside Google Cloud, prefer Workload Identity Federation. Service-account
key files remain supported by ADC, but are not the recommended production credential source.

The configured project ID is part of every request path and every signed Center cursor and change
receipt. A quota project can affect billing and quota attribution, but never replaces this resource
project scope.
