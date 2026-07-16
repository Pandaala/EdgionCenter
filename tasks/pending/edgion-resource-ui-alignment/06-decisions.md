# Decisions

## 2026-07-15 — Initial architecture

- Use operator-owned projections with per-resource internal-path removal.
- Preserve unknown operator spec fields; do not use generic destructive
  normalization or `removeEmpty` on existing resources.
- Keep Secret and ConfigMap as restricted dependencies.
- Use a complete resource catalog and shared condition diagnostics.

## 2026-07-15 — First independent design review

### Adopted

- Replace the simple Rust-first precedence with operator, server-owned, and
  runtime/internal data channels.
- Define separate view, editable, and mutation documents.
- Preserve `resourceVersion` only as an update concurrency field and strip
  status/server metadata/internal/redacted fields.
- Add an isolated Edgion worktree and record both repository base commits.
- Add task labels, name prefixes, exact cluster-scoped cleanup, and separate
  Controller fixture scopes.
- Expand the resource ledger with versions, sources, operations, access,
  fixtures, and scenarios.
- Add repeatable Playwright E2E with stable case IDs, Controller-state oracles,
  condition polling, screenshots/traces, and pagination-sized fixtures.
- Split typecheck and bundle evidence and include the root runtime matrix.
- Move the task to `tasks/pending`, add the full workflow document set, pause
  early UI-01 expansion, and correct stale web skills before resuming.

### Superseded after second review

- Capability reporting will not overload `server-info`: an explicit policy may
  grant resource writes while denying `server-info`, which would create a false
  read-only UI. `GET /api/v1/access` instead reports only the authenticated
  caller's own effective access behind a dedicated authentication boundary.

### Not adopted

- Reverting the already-created UI-01 foundation files solely because they were
  started early. They remain uncommitted and paused; after the revised design is
  approved they will either be adapted to the mutation envelope or removed.

## Verified implementation facts

- `Role::Restricted` already owns an `Arc<CenterPolicy>`. Access introspection
  snapshots that Arc once and hashes the canonical normalized result, avoiding
  a separate revision that could disagree with the evaluated policy.
- Authorization middleware returns 401 when no role was injected and 403 for a
  classified route denied by an authenticated role.
- The built-in default grants reads on every non-Secret resource, writes only
  for EdgionConfigData, RegionRoute list/failover, and `server-info`.

## 2026-07-15 — Mutation and version handling verification

- Controller create/update handlers deserialize a complete resource and storage
  performs whole-resource writes. The frontend mutation envelope therefore
  contains the complete operator projection, not a merge patch.
- Kubernetes storage fetches and overwrites `resourceVersion` before replace;
  filesystem and etcd do not use the frontend token. The mutation envelope strips
  resourceVersion instead of implying unsupported optimistic concurrency.
- Canonical/alternate pairs verified from current source: TLSRoute `v1` with
  `v1alpha3` conversion, ReferenceGrant `v1` with `v1beta1` conversion, and
  BackendTLSPolicy `v1` with `v1alpha3` conversion. TCPRoute and UDPRoute remain
  `v1alpha2` canonical in the current code.

## 2026-07-15 — Second independent design review

- Add a Controller-side operator/server merge; frontend stripping alone must
  never clear status, finalizers, or owner references in any storage backend.
- Define exact path boundaries and make unknown-field preservation a
  current-backend-accepted frontend guarantee.
- Use authenticated self-access with a content revision and exact compatibility,
  caching, and error rules.
- Implement and test ReferenceGrant v1-to-v1beta1 writer projection.
- Replace ad-hoc cleanup commands with a generated object ledger and exact CRD
  inventory.
- Make Playwright dependencies, scripts, authentication, seeding, oracles,
  action inventory, artifacts, and exit semantics explicit.
- Track skill corrections as a first-class reviewed deliverable.
