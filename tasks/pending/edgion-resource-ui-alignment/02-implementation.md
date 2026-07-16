# Implementation Plan

## Phase gates

Every increment follows the same gate:

1. Confirm the Rust/CRD wire contract and add preservation fixtures.
2. Implement types, adapters, form components, list/status behavior, and i18n.
3. Run targeted unit tests, typecheck, lint, and production build.
4. Exercise real CRUD and diagnostic states against two Controllers.
5. Exercise every affected visible button in the browser.
6. Obtain independent subagent review and resolve all material findings.

An increment with a failing gate remains in progress.

## Ordered increments

### UI-00: Baseline and isolation

- Isolated feature worktree and branch.
- Resource coverage and validation matrices.
- Dedicated Kubernetes test namespace and repeatable seed data.
- Baseline test, lint, typecheck, and build results.

### UI-01: Editor safety foundation

- Controller operator/server merge for filesystem, etcd, and Kubernetes.
- Authenticated self-access endpoint, Center proxy integration, and action hook.
- ReferenceGrant v1beta1 Kubernetes writer projection.
- Lossless resource adapters and preservation-test helpers.
- Resource capability registry.
- Unified conditions and permission-aware action components.
- Shared reference editors and validation primitives.

### UI-02: Destructive-drift repairs

- EdgionConfigData, EdgionAcme, GatewayClass, EdgionTls.
- EdgionStreamPlugins and HTTPRoute.
- TCPRoute, TLSRoute, and UDPRoute multi-rule preservation.
- BackendTLSPolicy, Gateway multi-certificate editing, and
  EdgionGatewayConfig trusted IP groups.

### UI-03: EdgionBackendTrafficPolicy

- Complete CRUD, form/YAML, status, authorization, Service linkage, topology,
  dashboard, and conflict diagnostics.

### UI-04: Route completion

- HTTPRoute and GRPCRoute filters, timeouts, retries, session persistence,
  delegation, backend filters, and cross-namespace references.
- Stream Route rules, annotations, plugins, and validation.

### UI-05: Gateway completion

- Gateway addresses, allowedRoutes, namespace selectors, listener kinds,
  certificates, frontend validation, and global TLS.
- GatewayClass parameters.
- EdgionGatewayConfig server, timeout, load-balancing, DNS, outbound TLS,
  security, global-plugin, ReferenceGrant, and LinkSys fields.

### UI-06: Plugin and ConfigData completion

- Current HTTP plugin catalog and variant editors.
- Both StreamPlugin stages and all current variants.
- All EdgionConfigData variants and consumer references.

### UI-07: Dependency and LinkSys completion

- All LinkSys variants and current advanced settings.
- Service and EndpointSlice templates and validation.
- Complete BackendTLSPolicy UX.
- Restricted ConfigMap and Secret dependency workflows.
- Restricted dependency safety exception: only metadata `list-keys`, create,
  and full replacement are exposed; value view, delete, and batch delete remain
  hidden and are locked by catalog/page tests.

### UI-08: Observability and navigation

- Conditions across all resources.
- Cross-resource links and full topology.
- Dashboard counts, conflicts, unresolved references, and ACME expiry repair.
- Multi-Controller state comparison and consistent bulk operations.
- Frontend and Edgion skill updates.

### UI-09: Runtime acceptance

- Executable Playwright dependency/config, authentication projects, generators,
  action inventory, API/Kubernetes oracles, artifacts, and cleanup ledger.
- Standalone Center with two Controllers and seeded resources.
- Kubernetes Center image deployed in OrbStack with two Controllers.
- Full page and button browser suite in both modes.

### UI-10: Final audit

- Full repository checks and E2E evidence.
- Independent correctness, compatibility, and security review.
- Final branch diff audit, retained environment, access instructions, and merge
  handoff.
