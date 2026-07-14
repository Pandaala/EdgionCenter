# Persistence and platform adapters

Persistence is selected at compile time by the binary composition.

## Standalone SQL

`crates/center-adapter-sql/` implements controller directory, users/roles, and audit storage
over sqlx. Migrations live under its `migrations/sqlite/` and `migrations/mysql/` trees.
`bins/edgion-center-standalone/src/config/mod.rs` selects SQLite or MySQL. The standalone
binary requires `database.enabled = true`; connection and migration failures abort startup.
SQLite is suitable for a single process, while MySQL supports externally managed durable
storage. SQL audit records are queryable through the Admin API.

## Kubernetes-native

`crates/center-adapter-kubernetes/` stores durable Controller directory information in
Controller CRDs and session ownership in coordination Leases. Status projection uses
resourceVersion-aware updates and observed generation. SubjectAccessReview is the
authorization source and audit events are structured JSON on stdout for the cluster logging
pipeline. There is no SQL store, password database, or audit-query endpoint in this binary;
capability discovery tells the dashboard which features to expose.

The adapter dependency graphs are guarded by the integration matrix: standalone must not
contain `kube`/`k8s-openapi`, and Kubernetes must not contain `sqlx` or the SQL adapter.
