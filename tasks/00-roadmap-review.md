# EdgionCenter Task Roadmap Review

## Meta

| Key | Value |
|---|---|
| Reviewed | 2026-07-14 |
| Status | pending decisions |
| Type | investigation |
| Scope | `tasks/01` through `tasks/06`, verified against current EdgionCenter and Edgion source |

## Executive summary

Tasks 01–05 form one database-optional/stateless access-control program. Task 06 is an independent P2, cross-repository product initiative and must not be treated as the final step of the stateless migration.

The current task files are useful intent notes but are not implementation-ready. Several assumptions are stale or incomplete:

- The live controller registry is already in memory; only the Admin controller endpoints remain DB-only.
- OIDC validation already preserves full claims, and locally issued login JWTs are already validated statelessly.
- The `api_tokens` table exists, but no token issuance, listing, revocation, or token-auth API exists. Task 04 would add a new service-token product rather than migrate a working one.
- Center federation currently watches one hard-coded resource kind (`EdgionConfigData`). Task 06 requires a multi-kind federation-watch refactor before it can sync a new CRD.
- A database-less replica owns only the controllers connected to that process. Multiple replicas cannot expose one complete global in-memory view without affinity, replication, or shared state.
- The skills and architecture documents still contain stale `PluginMetaData` and SQLite-only descriptions that must be corrected during verification.

## Baseline verification

Run on 2026-07-14 before implementation:

- `cargo test --all-targets`: passed, 296 tests.
- `./cicd/checks/check_english_only.sh`: passed.
- `./cicd/checks/check_no_legacy_pm.sh`: passed for the paths covered by that guard.
- The build refreshes `Cargo.lock` because the local `Edgion/edgion-resources` dependency now brings in `edgion-dsl` and additional transitive dependencies. The diagnostic lockfile change was not retained; dependency-lock alignment must be handled deliberately in the first implementation increment.

## Program boundaries

### Program A: Optional-database Center

Includes Tasks 01–05. The intended outcome should be defined as one of:

1. Keep SQL as an optional backend while making every core runtime function work without it; or
2. Remove SQL-backed controller history, DB users/RBAC, audit querying, and related dashboard functions entirely.

The current code and task language mostly imply option 1, while Task 05's “clean up obsolete database components” can be read as option 2. This must be resolved before cleanup begins.

### Program B: Declarative E2E probing

Task 06 spans both repositories and a new prober runtime:

- `Edgion`: shared resource type, CRD, Controller ingestion/status/lifecycle, federation authorization.
- `EdgionCenter`: multi-kind watch, aggregation, distribution, result ingestion, API/authz.
- Prober: execution engine, deployment, task acquisition, result reporting, and metrics.

Treat it as a separate feature program with its own design and acceptance matrix.

## Task 01 review: stdout structured audit logging

### Current state

- `AuditSink::spawn` accepts only `Arc<Store>` and writes asynchronously to SQL.
- Startup disables audit when no `Store` exists.
- `AuditRecord` is not serializable yet.
- The audit-list Admin API and dashboard require SQL-backed history.
- `retention_days` is already documented as having no scheduled runtime effect.

### Required design corrections

1. Use an explicit resolved backend model. A plain `Db | Stdout` enum cannot express “DB when available, stdout otherwise” during field-level serde defaulting. Prefer `Auto | Db | Stdout`, with `Auto` as the backward-compatible default and one post-parse resolver.
2. Define the stdout contract as one stable JSON object per line. Include an event discriminator and schema version so audit collectors can distinguish it from ordinary process logs.
3. Decide whether stdout uses the tracing pipeline or a dedicated writer. Logging serialized JSON as a tracing message can produce nested JSON; direct stdout bypasses tracing filters and formatting.
4. Define explicit-backend failure behavior. An explicit `db` backend with no usable Store should either fail startup or disable audit; it must not silently change destinations.
5. Define `GET /admin/audit-logs` behavior under stdout: unsupported response, empty list, or removal/hiding in the dashboard.
6. Clarify that `retention_days` applies only to the DB backend.
7. Make the writer injectable so JSON output can be tested without replacing process stdout globally.

### Suggested subtasks

- 01A: Config schema and backend resolution.
- 01B: Serializable versioned audit envelope and writer abstraction.
- 01C: DB/stdout sink implementations and startup wiring.
- 01D: API/dashboard capability behavior.
- 01E: unit and integration tests plus config docs.

## Task 02 review: database-less controller registry

### Current state

- `ControllerRegistry` already retains `RegisterRequest` metadata, online state, and monotonic `last_seen`.
- `ResourceAggregator` duplicates controller metadata and online state and additionally stores stats.
- `GET /api/v1/controllers` already uses memory.
- `GET` and `DELETE /api/v1/center/admin/controllers` still require SQL.
- Disconnect handling already skips SQL when no Store exists.

### Required design corrections

1. Add a stable registry snapshot API instead of exposing internal session handles. Do not create a third controller-state owner.
2. `Instant` cannot produce the Admin DTO's epoch `last_seen_at`. Store or update a wall-clock timestamp alongside the monotonic heartbeat timestamp.
3. Define list ordering and process-restart semantics. Database-less mode can only show controllers seen since the current process started.
4. Choose fallback based on resolved Store availability versus configured `database.enabled`. These differ when an enabled SQLite store fails to open.
5. Define DELETE as an eviction operation. “No-op or remove” is not an acceptance criterion.
6. Active-session deletion needs explicit cancellation/connection termination. Removing the registry sender makes the session stale, but the lifecycle and client reconnect behavior must be intentional and tested.
7. Keep cascade cleanup consistent across registry, aggregator, watch caches, metadata store, pending commands, and pending proxy calls.

### Suggested subtasks

- 02A: Registry snapshot DTO and wall-clock tracking.
- 02B: Admin GET backend selection and deterministic output.
- 02C: Unified eviction operation for DB and memory modes.
- 02D: registration, heartbeat, disconnect, delete, reconnect, and restart-semantics tests.

## Task 03 review: declarative RBAC

### Current state

- Authorization is already abstracted behind `AuthzStore` with AllowAll and DB implementations.
- The existing permission catalog uses keys such as `controllers:read`; the task example uses incompatible dot-form keys such as `controller.list`.
- The middleware receives only subject/provider in `Principal`; full OIDC claims remain in `UnifiedAuthClaims` but are not forwarded to authz.
- User/role handlers are DB CRUD endpoints and return 503 without a Store.

### Required design corrections

1. Define whether the mode is named `file_rbac` or `file`, and whether mode selects policy semantics or storage backend.
2. Reuse and validate against the canonical permission catalog. Reject unknown permission keys, duplicate roles/subjects, references to undefined roles, and malformed documents at startup/reload.
3. Namespace subject bindings by provider and preferably OIDC issuer. A bare `sub` or email can collide across identity providers.
4. Separate static subject bindings from claim-to-role mappings. The former belongs here; the latter can be layered in Task 04.
5. Define path resolution relative to the Center config file or working directory.
6. Use last-known-good atomic reload semantics: parse and validate a complete replacement, swap only on success, and keep the previous policy on reload failure.
7. Choose a reload trigger that works with Kubernetes projected ConfigMap symlink swaps. A simple watch of the file inode is insufficient.
8. Define read-only Admin API behavior. Mutations can return a stable error, but decide whether GET users/roles exposes the static policy or is unavailable.
9. Expose access-control capabilities in `server-info` so the dashboard can hide or render DB-only management features correctly.

### Suggested subtasks

- 03A: File schema, validation rules, and config model.
- 03B: Immutable compiled policy and `FileAuthzStore`.
- 03C: Startup wiring and access-axis validation.
- 03D: Atomic reload controller with ConfigMap-safe tests.
- 03E: Admin API and dashboard capability behavior.
- 03F: authorization, malformed-policy, and last-known-good tests.

## Task 04 review: OIDC claims and stateless service tokens

### Current state

- OIDC signature/time/audience/issuer validation is already stateless and returns the full claims JSON.
- Local/DB password login already issues HS256 JWTs that unified auth validates without a per-request DB token lookup.
- DB RBAC still resolves permissions from SQL by subject.
- An `api_tokens` migration table exists, but there is no implemented API-token feature to migrate.

### Required design corrections

1. Split this task into two independently shippable features: claims-to-role authorization and service tokens.
2. Add claims to the authz principal without coupling authentication to a concrete authz implementation.
3. Make claim extraction configurable: claim names, string versus array shapes, normalization, and claim-value-to-local-role mappings.
4. Define how direct subject roles and claim-derived roles combine.
5. Decide whether service tokens are actually required. If yes, specify issuance/list/revocation endpoints and permission catalog entries; this is new functionality.
6. Use a distinct token type, issuer, audience, and preferably signing key from browser session JWTs to prevent token-class confusion.
7. Define key rotation (`kid`/key ring), maximum TTL, clock skew, scope format, and how token scopes intersect with file RBAC permissions.
8. Accept the revocation trade-off: a fully stateless token cannot be revoked immediately without a denylist/shared state. Short TTL or signing-key rotation are the available coarse controls.
9. Define who may issue a token for which subject/scopes and whether a token is shown only once.
10. Remove the unused `api_tokens` table only after deciding that DB-backed opaque tokens will not be supported.

### Suggested subtasks

- 04A: Claims-bearing principal and configurable role mapping.
- 04B: Claims authorization tests across issuers and claim shapes.
- 04C: Service-token product and threat-model decision.
- 04D: Typed token format, signing-key configuration, and validator.
- 04E: Issuance API/authz/audit integration if service tokens are approved.
- 04F: rotation, expiry, scope, confusion, and no-revocation tests.

## Task 05 review: verification and cleanup

### Required design corrections

1. Build a mode matrix rather than one “stateless” scenario: DB on/off, audit auto/db/stdout, allow-all/file/DB RBAC, OIDC/local/DB authn, and supported invalid combinations.
2. Multi-replica behavior is not currently globally consistent. Each replica sees only its own controller sessions and metadata. Choose one acceptance model:
   - documented per-replica partial views plus controller/admin affinity;
   - controller fan-out to every Center replica;
   - inter-replica replication;
   - or shared state, which weakens the database-less goal.
3. ConfigMap reload tests must cover Kubernetes atomic symlink replacement and invalid-to-valid recovery.
4. Container stdout verification is automatable; systemd/syslog verification needs an explicit supported artifact and environment or should be documented as a manual runbook.
5. “Cleanup obsolete database components” must not delete optional DB features unless Program A chooses full DB removal.
6. Update stale skills/docs (`PluginMetaData`, SQLite-only `CenterDb` language, current MySQL support, and the one-kind watch limitation).
7. Define full validation commands and external prerequisites for MySQL, Kubernetes, mTLS, and browser/frontend checks.

### Suggested subtasks

- 05A: Unit/config compatibility matrix.
- 05B: Database-less process integration harness.
- 05C: Kubernetes ConfigMap reload verification.
- 05D: Multi-replica test after its consistency model is chosen.
- 05E: container/standalone operational verification.
- 05F: conditional cleanup, docs, skills, examples, and dashboard updates.

## Task 06 review: declarative E2E probing

### Current state and contradictions

- The roadmap summary says route annotations while the design selects a Policy CRD.
- The task describes a prober and result reporting but has no implementation steps for either.
- Center's watch loop and `CenterSyncClient` support only one hard-coded `EdgionConfigData` kind.
- No `EdgionE2EProbe` resource, CRD, ResourceKind, Controller handler, status writer, API, or prober exists.
- The Center and Controller copies of `fed_sync.proto` are functionally aligned but already have a comment-only drift, showing the need for an explicit synchronization check if the protocol changes.

### Missing product and protocol decisions

1. Define CRD group/version, namespaced scope, structural schema, status conditions, targetRef rules, and whether `sectionName` is supported.
2. Define how a target route becomes an executable URL: Gateway address, listener, scheme, port, hostname, path/query, and multi-parent/multi-listener selection.
3. Define request capabilities and limits: body, timeout, redirects, TLS verification, client certificates, gRPC service/method, response assertions, and payload size.
4. Do not put reusable bearer credentials directly into distributable probe specs. Define Secret references, resolution boundary, redaction, and prober authorization.
5. Reconsider automatic ownerReference injection. Policy attachment does not inherently imply lifecycle ownership; mutating a user-created policy to make route deletion garbage-collect it may be surprising. If retained, define same-namespace and existing-owner behavior.
6. Define result transport from prober to Center and from Center to the owning Controller for status writes. The current protocol has no probe-result message.
7. Define status aggregation for multiple probe locations, replicas, and attempts. One `phase` field is insufficient for distributed probing.
8. Define task distribution semantics: snapshot/pagination, ETag/version, long polling or watch, task leases, location selectors, duplicate execution, and stale task removal.
9. Define prober authentication/authorization and Center permission keys.
10. Define multi-prober scheduling and high-availability semantics.
11. Bound Prometheus label cardinality; route/name/namespace/cluster/location labels must use a documented budget.
12. Decide the prober's repository/location and language. It is a deliverable, not an external assumption.

### Suggested feature split

- 06A: Product requirements, threat model, and CRD/status design.
- 06B (`Edgion`): Resource structs, CRD, ResourceKind/meta registration, validation, Controller ingestion, RBAC, and status writer.
- 06C (both): Generalize federation reverse-watch from one hard-coded kind to typed multi-kind state, with protocol parity checks.
- 06D (`EdgionCenter`): Probe aggregation and versioned task-distribution API.
- 06E (prober): Runtime, protocols, limits, metrics, deployment, and auth.
- 06F (both/prober): Result-ingestion and CRD status feedback loop.
- 06G: Unit, contract, integration, Kubernetes GC/lifecycle, reconnect, and multi-prober tests.
- 06H: User docs, skills, manifests, dashboards/observability, and operational runbooks.

## Decision queue

Resolve these in order because later answers depend on earlier ones:

1. Is SQL retained as an optional feature, or removed from Center?
2. What consistency model is acceptable for multiple database-less Center replicas?
3. Should audit backend default be `auto`, and what happens when explicit `db` is unavailable?
4. What must Admin DELETE do to a currently connected controller?
5. Should file RBAC expose read-only users/roles through the existing Admin APIs?
6. Which reload mechanism and failure policy are required for `rbac.yaml`?
7. How should OIDC claim roles combine with subject bindings?
8. Is a new stateless service-token issuance feature required, despite the lack of immediate revocation?
9. Is Task 06 in the same completion commitment as Tasks 01–05, or a later independent milestone?
10. For Task 06, is the probe CRD independently managed or lifecycle-owned and garbage-collected with its target route?

## Recommended execution order

1. Resolve decisions 1–8 and rewrite Program A acceptance criteria.
2. Implement 01 and 02 as independent increments, validating each before continuing.
3. Implement 03, then claims mapping from 04.
4. Implement service tokens only if approved.
5. Complete the Program A verification matrix and conditional cleanup in 05.
6. Resolve decisions 9–10 and complete 06A design before writing Task 06 production code.
7. Execute 06B–06H in contract-first order, validating Edgion and EdgionCenter separately at every cross-repository boundary.

At each increment: analyze, finalize acceptance criteria, implement, run targeted tests, review the diff, run repository checks, update task state, and continue until the program's completion criteria are satisfied.
