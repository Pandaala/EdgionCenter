# Architecture Design

## 1. Design principles

1. Keep binaries as composition roots only. They parse platform configuration, construct adapters, start the shared runtime, and contain no business logic.
2. Keep domain and application behavior independent of Axum, Tonic, SQLx, and Kube where practical.
3. Prefer small capability-specific ports over one large `ControlStore` interface. Kubernetes and SQL do not have identical capabilities, and a monolithic trait would accumulate unsupported methods.
4. Select the platform at compile time by binary package, not through pervasive runtime `if database.enabled` branches.
5. Keep connection-local state in memory. Persist only durable/queryable facts through platform adapters.
6. Make unsupported management surfaces explicit through capability metadata and route composition.

## 2. Target workspace

```text
EdgionCenter/
├── Cargo.toml                         # virtual workspace manifest
├── Cargo.lock
├── crates/
│   ├── center-core/                  # domain types, capability ports, errors
│   │   └── src/
│   │       ├── controller.rs
│   │       ├── audit.rs
│   │       ├── authz.rs
│   │       ├── capabilities.rs
│   │       └── lib.rs
│   ├── center-runtime/               # shared server/application behavior
│   │   └── src/
│   │       ├── api/
│   │       ├── auth/
│   │       ├── federation/
│   │       ├── aggregator/
│   │       ├── watch_cache/
│   │       ├── metadata_store/
│   │       ├── commander/
│   │       ├── proxy/
│   │       ├── observe/
│   │       └── lib.rs
│   ├── center-adapter-sql/           # SQLite/MySQL and DB management
│   │   └── src/
│   │       ├── controller_repository.rs
│   │       ├── audit.rs
│   │       ├── users.rs
│   │       ├── authz.rs
│   │       ├── migrations/
│   │       └── lib.rs
│   └── center-adapter-kubernetes/    # Kube API, CRDs, SAR, Lease
│       └── src/
│           ├── controller_repository.rs
│           ├── authorization.rs
│           ├── coordination.rs
│           ├── crd/
│           └── lib.rs
├── bins/
│   ├── edgion-center-standalone/
│   │   └── src/main.rs
│   └── edgion-center-kubernetes/
│       └── src/main.rs
├── proto/                            # federation wire source, built by runtime
├── config/
│   ├── common.example.yaml
│   ├── standalone.example.yaml
│   └── kubernetes.example.yaml
├── cicd/
├── web/
├── skills/
└── tasks/
```

The initial extraction may keep a few modules grouped differently to minimize churn, but dependency direction must remain:

```text
                    center-core
                   /     |      \
                  /      |       \
        center-runtime  sql-adapter  kubernetes-adapter
               |          |             |
               +----------+-------------+
                          |
                  selected binary root
```

Adapters may depend on `center-core`. `center-core` must never depend on adapters or runtime. `center-runtime` may depend on core but not on either adapter. Each binary depends on runtime plus exactly one primary platform adapter.

## 3. Binary contracts

### `edgion-center-kubernetes`

- Requires access to a Kubernetes API.
- Uses OIDC for human identity; no Center-managed password database.
- Maps validated identity and groups to Kubernetes authorization attributes.
- Uses `SubjectAccessReview` for Center Admin API authorization.
- Stores controller directory/status as Kubernetes resources.
- Relies on kube-apiserver audit for declarative resource mutations.
- Emits runtime/federation administrative actions as structured stdout audit events.
- Uses Lease resources for singleton coordination and ownership timeout.
- Does not link SQLx, SQLite, MySQL, or bcrypt-based DB-user management.

### `edgion-center-standalone`

- Runs on VM, Docker, bare process, or Kubernetes when the operator explicitly wants SQL management.
- Uses SQLite or MySQL for durable controller history, users, roles, and queryable audit logs.
- Supports OIDC, local admin, and DB-user authentication as currently implemented.
- Does not link Kube client/runtime dependencies.
- Retains the current Admin user/role management experience.

## 4. Platform ports

Do not define one `ControlStore` with every method. Define narrow ports owned by `center-core` and consumed by application services.

### Controller directory

```rust
#[async_trait]
pub trait ControllerDirectory: Send + Sync {
    async fn upsert_registration(&self, registration: ControllerRegistration) -> Result<()>;
    async fn mark_offline(&self, id: &ControllerId, observed_session: &SessionId) -> Result<()>;
    async fn list(&self) -> Result<Vec<ControllerRecord>>;
    async fn evict(&self, id: &ControllerId) -> Result<EvictionOutcome>;
}
```

The in-process `ControllerRegistry` remains the authority for live stream handles and takeover safety. `ControllerDirectory` stores a queryable projection, not `mpsc::Sender` or `Instant` values.

### Audit

```rust
pub trait AuditWriter: Send + Sync {
    fn record(&self, event: AuditEvent);
}

#[async_trait]
pub trait AuditReader: Send + Sync {
    async fn query(&self, filter: AuditFilter, page: Page) -> Result<AuditPage>;
}
```

Both modes provide `AuditWriter`. Only standalone SQL necessarily provides `AuditReader`; Kubernetes mode uses cluster audit infrastructure and advertises that Center-local audit history is unavailable.

### Authorization

```rust
#[async_trait]
pub trait Authorizer: Send + Sync {
    async fn authorize(&self, principal: &Principal, action: &Action) -> Result<Decision>;
}
```

- SQL adapter resolves existing Center permission keys from DB roles.
- Kubernetes adapter translates `Action` to Kubernetes resource or non-resource attributes and submits `SubjectAccessReview`.
- AllowAll remains a small core/runtime implementation for explicitly selected simple standalone deployments.

### User administration

Keep `UserAdmin` and `RoleAdmin` as standalone capabilities rather than mandatory platform ports. Kubernetes mode delegates identity and role binding management to the Kubernetes/IdP control plane and does not emulate SQL CRUD endpoints.

### Coordination

```rust
#[async_trait]
pub trait Coordinator: Send + Sync {
    async fn acquire(&self, role: CoordinationRole) -> Result<Leadership>;
}
```

Kubernetes uses Lease objects. Standalone initially uses single-process ownership; a future SQL-backed coordinator is possible without changing runtime business logic.

## 5. Capabilities and API composition

Expose resolved capabilities in `GET /api/v1/server-info`:

```json
{
  "mode": "kubernetes",
  "capabilities": {
    "userAdmin": false,
    "roleAdmin": false,
    "auditQuery": false,
    "controllerHistory": true,
    "nativeRbac": true,
    "leaderElection": true
  }
}
```

Handlers must not inspect `Option<Store>`. The composition root either:

- mounts a capability-backed route; or
- mounts a stable unsupported response when API compatibility requires the path to exist.

The dashboard consumes capabilities and hides SQL-only management pages in Kubernetes mode. It must not infer behavior from `database.enabled` or binary names.

## 6. Kubernetes resource model

### `EdgionController`

Use a namespaced management-cluster CRD as the durable projection of a connected Controller.

```yaml
apiVersion: center.edgion.io/v1alpha1
kind: EdgionController
metadata:
  name: <stable-dns-safe-id>
spec:
  controllerId: cluster-a/controller-0
  cluster: cluster-a
  env: [prod]
  tags: [region-east]
status:
  phase: Online
  sessionId: <opaque-id>
  connectedReplica: center-7cc8f
  lastSeenTime: "2026-07-14T12:00:00Z"
  observedGeneration: 1
  conditions: []
```

The raw controller id may not be a valid Kubernetes object name. Generate a deterministic DNS-safe metadata name and retain the canonical id in `spec.controllerId`; guard collisions with a digest suffix.

The CRD is a directory/status projection. Federation stream handles, pending commands, and proxy response channels remain local memory and are never serialized.

### RBAC mapping

Map Center actions to native Kubernetes attributes, preferably real management resources:

| Center action | Kubernetes authorization attribute |
|---|---|
| list controllers | `list` `edgioncontrollers.center.edgion.io` |
| read controller | `get` `edgioncontrollers.center.edgion.io` |
| evict controller | `delete` `edgioncontrollers.center.edgion.io` or an explicit subresource/non-resource action |
| read audit history | unsupported in Kubernetes mode; cluster audit system owns it |
| manage Center users/roles | not exposed; manage OIDC and Kubernetes RBAC externally |

Use OIDC `iss/sub` plus validated groups as the principal. Never trust user/group headers from the client.

### Audit boundary

- Mutations performed through Kubernetes APIs are covered by kube-apiserver audit policy.
- Runtime actions that do not correspond to a Kubernetes API mutation (federation connect/disconnect, command dispatch, proxy operation) are emitted through the structured `AuditWriter` to stdout.
- Do not duplicate the same mutation into a local SQL-like audit store in Kubernetes mode.

### Multi-replica behavior

- Every replica can rebuild the controller directory and declarative state by watching Kubernetes resources.
- A federation stream is owned by one replica at a time; `status.connectedReplica` and a Lease/heartbeat establish ownership.
- Abrupt replica loss is detected by Lease expiry, after which status transitions offline/stale and Controllers reconnect through the service.
- Global read APIs use Kubernetes projections rather than only the local connection registry.
- Commands and HTTP proxying route to the owning replica through authenticated internal forwarding; ownership and fencing are established through the coordination adapter.

Kubernetes mode is “no separate product database,” not stateless in the literal sense: durable state is delegated to the Kubernetes API/etcd.

## 7. Configuration boundaries

Keep bootstrap configuration separate from managed runtime resources.

### Shared bootstrap

- listener addresses and TLS paths;
- federation mTLS and peer identity;
- OIDC validation settings;
- observability settings;
- web asset source.

### Standalone-only

- database backend and connection details;
- DB-user login and bootstrap admin;
- SQL audit retention.

### Kubernetes-only

- management namespace;
- CRD names/version policy;
- service account and SubjectAccessReview settings;
- Lease names/durations;
- replica identity and internal forwarding configuration.

Reject unknown or cross-mode fields rather than silently ignoring them.

## 8. Migration sequence

1. Convert the root to a Cargo workspace without moving behavior; keep the existing binary as the temporary compatibility target.
2. Extract `center-core` domain types and narrow ports with unit tests.
3. Extract `center-runtime` and make the current SQL implementation satisfy the new ports.
4. Introduce `edgion-center-standalone`; prove behavior parity with the existing 296-test baseline and integration harness.
5. Remove `Option<Store>` and platform booleans from shared API state; switch to capabilities and ports.
6. Add Kubernetes CRD types and adapter tests using a mock API server where practical.
7. Add `edgion-center-kubernetes` and Kubernetes manifests/RBAC/Lease configuration.
8. Add controller projection, SAR authorization, structured runtime audit, and capability-driven dashboard behavior.
9. Add Kubernetes integration tests for restart reconstruction, invalid RBAC, audit boundaries, Lease expiry, and multi-replica ownership.
10. Update docs/skills and retire the original single binary only after both new binaries meet acceptance criteria.

At every step, keep an executable binary and avoid a repository-wide “big bang” move.

## 9. Open decisions

1. Should command/proxy requests in Kubernetes multi-replica mode be internally forwarded to the owning replica, or should a leader own all federation streams?
2. Should `EdgionController` be operator-deletable as the eviction API, or should eviction use a dedicated subresource/command to avoid confusing desired state with observed registration?
3. Should the Kubernetes binary expose read-only projections of Role/RoleBinding for the dashboard, or simply link operators to native Kubernetes management?
4. Are both binaries released in one image, or in separate minimal images?
5. During migration, does the current `edgion-center` binary alias standalone or Kubernetes mode?
