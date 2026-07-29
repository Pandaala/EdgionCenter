---
name: center-proxy-permission-method-split-only
description: Use when reviewing Center proxy authorization findings of the form "proxy:read / proxy:write is too coarse — it should be scoped per Controller, per resource kind, or per path".
---

# Center Proxy Permission: Method Split Only (new_work-12)

**Status / verdict**: fixed for the method split (new_work-12, 2026-07-29); per-Controller
and per-kind scoping is a **deliberately rejected** design, not an open gap.

## What was fixed

`route_permission` (`crates/center-app/src/common/authz/catalog.rs`) used to return a single
method-blind `proxy:access` key for `/api/v1/proxy/`, while every other route family already
branched on method. Because `SqlAuthorizer::authorize`
(`crates/center-adapter-sql/src/lib.rs`) is a flat permission-key set-membership test that
ignores `request_path` and `request_verb`, that one key meant read **and** write on every
Controller — a standalone deployment could not express a read-only proxy grant, which
Kubernetes mode could already express through `nonResourceURLs` on the SubjectAccessReview.

The proxy arm now returns `PROXY_READ` for `GET`/`HEAD` and `PROXY_WRITE` otherwise.
`proxy:access` no longer exists.

## What was rejected, and why

Do **not** re-propose a Center-side per-Controller or per-resource-kind proxy permission
taxonomy. That was CCI-07's original proposal and the write-model convergence design
dropped it after verifying the machinery already exists in two layers that answer different
questions:

- **Controller → Center is a capability grant.** The Controller's `authz_classifier.rs`
  maps method + path to `(Verb, kind)` across every route shape and default-denies anything
  unmatched; `center.rbac` evaluates it per verb per kind, defaulting to writes on
  `EdgionConfigData` only and denying `Secret` entirely. `/api/v1/access` reports the
  effective policy, so the dashboard follows the operator's grant with no Center-side kind
  table.
- **Center → its users is a distribution decision.** Kubernetes mode already expresses
  per-verb and per-kind grants natively. Adding Center permission keys for kinds would layer
  a second, coarser model on top of a working one.

The method split is the exception because it costs one catalog key and no new model — it
lets the *existing* flat-key model say what Kubernetes mode already says.

## Source paths

- `crates/center-app/src/common/authz/catalog.rs` — `PROXY_READ` / `PROXY_WRITE`,
  `all_keys()`, `catalog_groups()`, the `route_permission` proxy arm, and the
  `read_vs_write_keys` assertions
- `crates/center-adapter-sql/src/lib.rs` — `SqlAuthorizer::authorize` (key-only by design)
- `crates/center-adapter-kubernetes/src/sar.rs` — the path/verb SubjectAccessReview and the
  `/permissions/{key}` discovery probe
- `web/src/components/resource/PermissionAwareButton.tsx` — `requiredProxyPermission`
- `skills/02-features/access-control.md` — the operator-facing catalog

## Re-report guard

- "Proxy authorization ignores the Controller id / resource kind" → reject; see above.
- "`SqlAuthorizer` ignores `request_path` / `request_verb`" → accurate but **not** a defect.
  That authorizer's contract is key matching; expressiveness is added by splitting catalog
  keys, not by teaching it to parse paths.
- "The `operation` branch of `PermissionAwareButton` requires `proxy:write` even for
  read-ish operations" → intentional. An operation carries no verb, no call site passes one
  today, and fail-closed is the default posture.

## Migration note

`proxy:access` is the first key ever removed from the catalog, which exposed a latent
contradiction: the roles page **deliberately re-submits held keys the catalog does not
manage** (`RoleManagementPage.handleSave`, for version skew), while `validate_keys`
(`crates/center-app/src/api/roles.rs`) rejects any unknown key with 400. A role left holding
a stale `proxy:access` would therefore not merely lose access — it would become
**uneditable through the dashboard**, since every subsequent permission save fails.

Standalone deployments are covered by migration
`crates/center-adapter-sql/src/migrations/{sqlite,mysql}/0010_proxy_permission_split.sql`,
which rewrites each stored `proxy:access` row into `proxy:read` + `proxy:write`, preserving
the prior grant exactly. Kubernetes deployments have no such automation: their RBAC
`nonResourceURLs` must be re-granted with both new permission-discovery URLs, or the
dashboard loses the "enter Controller" button.

The underlying UI/backend contradiction is untouched and will bite again the next time a key
is removed. Deciding whether `validate_keys` should tolerate already-stored keys is a
separate call — do not fold it into a key rename.
