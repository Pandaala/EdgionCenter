# Completion audit checkpoint

Date: 2026-09-28. Source checkpoint: `7433d37` plus documentation in this commit.
Verdict: **not yet complete**. This reconciles current evidence; it does not
replace the user's full backend/dashboard, resource-by-resource/menu-by-menu scope.

## Requirements and evidence

| Requirement | Current evidence | Assessment |
| --- | --- | --- |
| Use current Edgion, without reconstructing its history | Current ResourceKind and CRD/handler reads; per-fix source citations | Satisfied for work performed |
| Cover every resource type | Fresh exact set comparison: 22 Edgion kinds = 22 Center catalog kinds; all 20 first-class resource paths occur in the Controller menu; Secret/ConfigMap share restricted dependencies | Catalog complete; nested operator/runtime ownership audit still open |
| Check every menu | Current Center menu has 17 leaves; controller catalog paths have no omissions; native 173-pass baseline plus focused follow-ups and separate OIDC/Kubernetes evidence | Current menu coverage indexed; runtime limits below remain explicit |
| Update Center backend | Federation ownership revocation, global write semantics, auth/cloud/error handling repairs; fresh 876 workspace + 247 no-default-feature app tests | Implemented paths verified; deployed Kubernetes image predates later backend fixes |
| Update Center frontend | Fresh 835 tests in 117 files; recent Gateway/BTP fixes have focused browser/component evidence | Current unit/component regression passes; last complete native run predates newest fixes |
| Exercise local runtime | Isolated native Center + two mTLS Controllers; API readback, real Gateway traffic, and separate OrbStack deployments | Established for recorded scenarios; no blanket conformance claim |
| Commit Center only, do not push | Center task commits; Edgion ACME repair remains local/uncommitted; unrelated work preserved | Scope retained |

Source comparison artifact: `/tmp/ws5-center-closure-source-audit.json`.
Current resource/menu details: CURRENT-COVERAGE.md. The opening table in README.md
is the original historical checkpoint, not an up-to-date pending-work list.

## Fresh gates

- Frontend: 835 passed / 117 files, 78.09 seconds.
  `/tmp/ws5-center-closure-frontend-full.log`.
- Backend matrix: formatting, Clippy, 876 workspace tests, 247 no-default-feature
  app tests, binary dependency purity, and offline kustomize rendering pass.
  `/tmp/ws5-center-closure-backend-matrix.log`.
- Matrix exit is **1**, at the existing English-only violation in the unrelated
  root `fix-issue-workflow-generic.zh.md`. It was preserved. This is not a green
  whole-matrix claim. The later no-legacy guard and diff whitespace check pass
  when run separately.
- EDGION_SKIP_WEB=1 avoids reinstalling the shared frontend dependencies while
  retained Vite environments may use them; frontend tests ran independently.
- External MySQL, real Kubernetes and federation opt-ins were not enabled for
  this matrix. Earlier real evidence retains its own version/runtime boundaries.
- Strict E2E inventory and E2E typecheck are recorded in
  `/tmp/ws5-center-closure-inventory.log` and `-e2e-types.log`.

## Remaining work, not superseded historical TODOs

1. Finish the nested operator/runtime-field review across the current resource
   implementations. The recent Gateway private projection finding proves why
   catalog parity and generic CRUD alone cannot close this requirement. Inspect
   ownership in ingestion before modifying exclusion paths; schemars(skip) alone
   does not determine ownership. Preserve supported operator configuration.
   [FIELD-OWNERSHIP-AUDIT.md](FIELD-OWNERSHIP-AUDIT.md) now records the checked
   EdgionTls, BackendTLSPolicy, EdgionBackendTrafficPolicy, Gateway, GatewayClass
   and EdgionGatewayConfig mutation boundaries.
2. Close selected cross-resource runtime gaps retained in the existing scope:
   frontend TLS policy transitions, AI backend/provider routing and policy
   attachment, and resilience behaviors beyond the five proven probe types.
   Existing configuration persistence must not be called traffic proof. Global outbound
   client identity resolution is repaired in the local, uncommitted Controller
   worktree; Center rejects unsupported global hostname/SAN constraints. See
   [GLOBAL-OUTBOUND-TLS-EVIDENCE.md](GLOBAL-OUTBOUND-TLS-EVIDENCE.md). Native
   inherited mTLS and Secret rotation now pass, but ReferenceGrant revocation
   reproduces stale Webhook client reuse after forced TLS rebuild failure. Resolve
   that invalidation boundary and rerun revocation/recovery/deletion before closure.
3. Reconcile Center menu async/fault coverage with current backend source. Real
   owner migration is proven; Pod crash takeover and ambiguous post-dispatch
   faults have narrower adapter/runtime evidence. New cloud error states have
   component/HTTP adapter evidence rather than native provider mutations.
4. After resulting fixes, rerun affected checks and the complete current native
   browser regression. The Kubernetes v5 deployment and old full browser runs
   cannot acquire newer source coverage by documentation changes.

Optional external-account/MySQL tests and a broken isolated kind node are
validation limits, not reasons to stop work that can proceed in native runtimes.
Do not restart any retained service based only on old process notes.

## Previously open items now established

Separate evidence establishes real Dex/OAuth login/logout, Kubernetes SAR and
owner/non-owner forwarding, all five active probe protocols (including mTLS
failure/recovery), actual RegionRoute dual-Gateway failover/source synchronization,
ReferenceGrant traffic transitions, and ACME issuance/renewal/certificate hot-load
with the local Controller repair. These must not remain generic pending items
in the current ledger, but none proves every feature of its wider category.
