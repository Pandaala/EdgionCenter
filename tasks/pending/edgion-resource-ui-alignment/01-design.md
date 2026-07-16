# Solution Assessment and Design

## Solution assessment

### Resource document handling

| Dimension | Operator projection with internal-field denylist | CRD whitelist/generated forms | Keep current normalizers |
|---|---|---|---|
| Safety | High when projection tests cover internal fields | High for known structural fields, low for forward compatibility | Low; known destructive drift |
| Compatibility | Preserves unknown future operator fields | Drops fields until the frontend schema is regenerated | Drops fields outside each form model |
| Scope | Shared adapter plus per-resource internal paths | Schema loader/generator and form runtime | Small, but does not meet the objective |
| Test difficulty | Moderate, fixture-driven | High, especially CRD unions and preserve-unknown payloads | Low but insufficient |
| Rollback | Adapter can be adopted resource by resource | Large platform rollback | N/A |

Use the operator-projection design. CRD-only whitelisting was rejected because
several Edgion CRDs contain preserve-unknown payloads and the frontend must not
delete a newly added operator field before its own release catches up. Current
normalizers are rejected because they caused the existing data-loss defects.

### Controller mutation capability discovery

| Dimension | Authenticated self-access endpoint | Federation registration/report | Infer from defaults or wait for 403 |
|---|---|---|---|
| Hot reload | Read current policy on refresh | Requires an update message and revision state | Cannot detect explicit policies |
| Protocol impact | One bounded HTTP response | Protobuf change in both repositories | None |
| Selected Controller UX | Natural through the existing proxy | Center must expose a new projection API | Incorrect for non-default policy |
| Older Controller | Fail closed for mutation | Fail closed for mutation | Unsafe guessing |

Expose `GET /api/v1/access` as an authenticated self-introspection endpoint. It
is reached only after CLI-token or Center identity injection and does not require
an ordinary policy grant, because it reveals only the already-authenticated
caller's effective allow set. Missing identity returns 401. Center proxy mode
reads it through the selected Controller and direct mode reads the same endpoint.
Center/user authorization is evaluated separately and the server remains
authoritative. A federation protocol change is unnecessary because the selected-
Controller HTTP proxy already carries the effective identity and observes policy
hot reload immediately.

The route is classified as an inherent `AccessSelf` action. Authorization
middleware first requires a `Role`; it then permits only this action without
consulting configurable allow rules. `AccessSelf` is not accepted in RBAC config,
is not a wildcard-matchable operation, and never appears in the returned policy.
This preserves the 401 authentication boundary without requiring a bootstrap
grant or adding the route to the static public skip set. Tests cover no role,
CLI-token role, Center role, deny-all role, and a write-only explicit policy.

Concretely, both existing routers mount `/api/v1/access` inside their current
authentication onion. `authz_classifier` recognizes the route as `AccessSelf`;
`authz_layer` performs its existing missing-Role 401 check, then handles
`AccessSelf` before `Authorizer::authorize`. The local admin router therefore
still requires `require_token_configured_gate` and `cli_token_pre_authn`; the
federation router still requires `inject_center_identity`. The route is never in
`public_paths` and unknown routes continue to return 403.

## Contract channels

Rust serde, the hand-maintained structural CRD, and Controller handlers are
cross-checked; they are not a simple precedence list. Every field belongs to one
of these channels:

1. **Operator document**: operator-owned metadata and non-internal spec fields
   accepted by the Controller/CRD. These may be edited and written back.
2. **Server-owned view**: status plus metadata such as `uid`, `generation`,
   `managedFields`, and `creationTimestamp`. These may be displayed but never
   changed by a structured form.
3. **Runtime/internal**: fields marked `schemars(skip)`, parsed/compiled/resolved
   material, denial markers, and redacted values. These are neither editable nor
   written back.

Edgion skills define operator semantics. When a skill conflicts with the actual
operator channel, record the discrepancy and correct the skill in the isolated
Edgion branch.

## Mutation envelope

```text
viewDocument     = complete API response used for display
editableDocument = operator metadata + operator spec
mutationDocument = complete editableDocument sent as requested operator state
```

Create and update retain `apiVersion`, `kind`, operator labels/annotations,
namespace, name, and the complete operator spec. Both strip `status`, protected
metadata, and each adapter's internal paths. A redaction sentinel is never
serialized back; a sensitive field must be explicitly replaced or left untouched
through a dedicated server operation.

The Controller update boundary performs an operator/server merge before every
backend write. It loads the current object once, replaces only operator-owned
metadata and spec with the request, and preserves current `status` plus protected
metadata (`uid`, `generation`, `managedFields`, timestamps, `ownerReferences`,
`finalizers`, and backend concurrency tokens). Kubernetes replace uses the
current dynamic object's metadata and `resourceVersion`; filesystem and etcd
apply the same JSON merge before persistence. Runtime/internal spec fields are
not copied from the current object: they are recomputed by reconciliation. This
rule is tested with common fixtures and backend-specific filesystem, etcd, and
Kubernetes tests.

Preservation tests compare the operator projection before and after editing.
They do not require status or runtime fields to survive a mutation payload.
`P-UNKNOWN` means a Controller-accepted operator field that is not modeled by
the current TypeScript form. It proves preservation inside the frontend request;
it does not promise that an arbitrary future field rejected or pruned by the
current Controller or Kubernetes schema survives end to end.

## Lossless editor architecture

Structured controls apply narrow immutable patches to `editableDocument`. They
must not reconstruct `spec`, list entries, references, or plugin entries from a
subset. Each adapter provides:

- create template and accepted API versions;
- parse and display projection;
- operator projection and mutation-envelope builder;
- internal-path and sensitive-path rules;
- narrow typed accessors and patches;
- validation without destructive cleanup;
- preservation fixtures for current, accepted alternate, unknown, multi-rule,
  and multi-reference documents.

Empty arrays or objects are removed only when the authoritative contract says
absence and emptiness are equivalent. Generic `removeEmpty` is forbidden on an
existing operator document.

## Resource capability registry

One registry accounts for all 21 `ResourceKind` values and describes scope,
API versions, lifecycle, route, editor, status shape, references, Controller
operations, required Center permission, and mutation capability. Menu, routes,
actions, topology discovery, and resource labels consume this registry where it
removes duplication. Secret and ConfigMap remain explicit
`restrictedDependency` entries rather than hidden omissions.

## Forms and diagnostics

- Shared Gateway API structures use common reference, condition, timeout,
  retry, session-persistence, TLS, and backend editors.
- Tagged enums use discriminator-driven variant editors.
- The plugin registry accounts for all 39 current HTTP variants, Stream Stage 1
  variants `IpRestriction`, `GlobalConnectionIpRestriction`, and
  `ConnectionRateLimit`, and Stream Stage 2 variant `IpRestriction`.
- ConfigData accounts for `KeyList`, `IpList`, `Selector`,
  `RegionRouteOverride`, and `Misc`.
- LinkSys accounts for Redis, etcd, Elasticsearch, Webhook, Kafka, and HTTPDNS.
- YAML is a first-class editor, not an escape hatch for destructive forms.
- Conditions render type, status, reason, message, observed generation,
  transition time, and parent/ancestor/listener context. No page invents an
  `Active` status.

## Authorization data flow

```text
Controller effective request Role (one policy Arc snapshot)
  -> GET /api/v1/access {schemaVersion, revision, resources, operations}
  -> Center selected-Controller HTTP proxy
  -> useControllerResourceAccess(controllerId)

Center /auth/me permissions ------------------------------+
Controller resourceAccess --------------------------------+-> action state
server capability flags ----------------------------------+
```

The endpoint snapshots one `Role::Restricted(Arc<CenterPolicy>)`, normalizes it
against the 21 bounded `ResourceKind` values, seven resource verbs, and nine
synthetic operations, sorts the result, and hashes the canonical bytes as
`revision`. No independent counter can race the policy snapshot. Response shape:

```json
{
  "success": true,
  "data": {
    "schemaVersion": 1,
    "revision": "sha256:<hex>",
    "resources": [
      {"kind": "HTTPRoute", "scope": "namespaced", "verbs": ["get", "list", "list-keys", "watch", "create", "update", "delete"]}
    ],
    "operations": [
      {"name": "regionRoute.list", "allowed": true},
      {"name": "regionRoute.failover", "allowed": false}
    ]
  }
}
```

The complete operation names are `regionRoute.list`, `regionRoute.failover`,
`acme.trigger`, `confSync.rotate`, `reload`, `serverInfo`, `diagnostics`,
`wipeAll`, and `debug`. Cardinality is capped at 21 resource rows, seven verbs
per row, and nine operations. Unsupported schema, 404, timeout, and malformed
responses retain readable pages but fail closed for mutations and synthetic
operations with a visible explanation. The query key is
`['controller-access', controllerId | 'direct']`, stale time is 10 seconds, and
it refetches on window focus, Controller switch, explicit refresh, and successful
reload. An action is enabled only when Center grants `proxy:access`, effective
Controller access grants it, and the resource catalog supports it.

Canonicalization always emits all 21 resource rows; an empty `verbs` array means
no access, so a missing row is a schema error. Kinds use `ResourceKind::as_str()`
order and verbs use fixed order `get`, `list`, `list-keys`, `watch`, `create`,
`update`, `delete`; operations use the order listed above. Wildcards are expanded
against only these bounded sets. Revision input is canonical compact JSON of
`schemaVersion`, all resource rows, and all operation rows, excluding the
`revision` field itself; its SHA-256 hex is prefixed with `sha256:`.

Endpoint success uses the normal API envelope. No Role returns HTTP 401 with the
existing Bearer header; snapshot/serialization failure returns HTTP 500 with
`{"success":false,"error":{"code":"ACCESS_SNAPSHOT_FAILED","message":"..."}}`.
Center proxy preserves Controller status/body; connection failure returns its
existing 502 envelope. The web distinguishes 401 (session/token invalid), 403
(proxy permission mismatch), 404 (old Controller), 5xx/network (temporarily
unavailable), and schema-invalid (incompatible), failing mutations closed in all
but the successful v1 case.

## Sensitive dependencies

Secret and ConfigMap are not unrestricted generic resource pages. ConfigMap
support creates controlled CA/key-value objects and references. Secret support
is write-only/redacted and typed for certificates, credentials, and plugin
dependencies. The UI never assumes Secret list/get returns plaintext.

## Cross-resource model

```text
GatewayClass -> Gateway -> Route -> Service
                         Route -> EdgionPlugins -> EdgionConfigData/LinkSys/Secret
                         Gateway -> EdgionTls/EdgionAcme -> Secret
                         Service -> BackendTLSPolicy -> Secret/ConfigMap
                         Service -> EdgionBackendTrafficPolicy
                         StreamRoute -> EdgionStreamPlugins -> EdgionConfigData/LinkSys
```

## Code change plan

### Edgion

- `edgion-controller/src/api`: add authenticated self-access introspection;
  never expose policy source, token identity, unmatched rules, or credentials.
- `edgion-controller/src/authz`: produce a deterministic effective access
  snapshot and content-hash revision from one Role policy Arc.
- `edgion-controller/src/conf_mgr`: merge operator updates with protected
  server state consistently in filesystem, etcd, and Kubernetes storage.
- Kubernetes writer: project ReferenceGrant `v1` request objects to the detected
  `v1beta1` GVK exactly as the other accepted alternates are projected.
- Controller API tests: default, explicit, reload, Secret exclusion, and
  fail-closed serialization; storage tests cover status/finalizer preservation.
- `skills/02-features/03-resources` and architecture indexes: correct contract
  discrepancies found by the ledger.

### EdgionCenter server

- Preserve and forward the Controller `/api/v1/access` status and body using the
  selected Controller's effective federation identity.
- Add contract tests for selected-controller access behavior, proxy denial/error
  envelopes, and older Controllers returning 404.

### EdgionCenter web

- `src/utils/resource-document.ts`: channel projection and immutable patching.
- `src/config/resourceCatalog.ts`: complete resource and operation registry.
- `src/components/resource`: conditions and permission-aware controls.
- `src/types`, `src/utils`, and `src/components/ResourceEditor`: migrate each
  resource adapter and form incrementally.
- `src/pages`, `App.tsx`, and shell menu: resource pages, status, and actions.
- `src/pages/Topology` and Dashboard: relationships and aggregate diagnostics.
- `e2e`: Playwright fixtures, page/action inventory, Controller oracles, traces,
  and mode matrix.

## Compatibility and rollback

- Only API versions the current Controller explicitly accepts are tested as
  alternates. Removed pre-release aliases or fields are not reintroduced.
- Older Controllers without `/api/v1/access` remain readable but their mutation
  controls fail closed; YAML viewing still works.
- Adapter migration is per resource, allowing a faulty increment to be reverted
  without reverting the shared capability protocol.
- Runtime deployment uses task-prefixed resources and an isolated namespace;
  rollback deletes exact task-owned objects only.

## Design gates

- Center's selected-controller proxy must preserve the self-access response and
  attach the same effective Center role used for later mutations.
- Every path in `04a-field-boundaries.md` must have a stripping or preservation
  fixture before that adapter is migrated.
- ReferenceGrant v1beta1 create/get/update/delete must pass against discovery
  where only that served version exists before the alternate is marked supported.
