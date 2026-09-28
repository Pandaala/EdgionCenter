# Completion audit checkpoint

Date: 2026-09-28. Source checkpoint: `ba012f3` (current full frontend and native browser regression).
Verdict: **not yet complete**. This reconciles current evidence; it does not
replace the user's full backend/dashboard, resource-by-resource/menu-by-menu scope.

## Requirements and evidence

| Requirement | Current evidence | Assessment |
| --- | --- | --- |
| Use current Edgion, without reconstructing its history | Current ResourceKind and CRD/handler reads; per-fix source citations | Satisfied for work performed |
| Cover every resource type | Fresh exact set comparison: 22 Edgion kinds = 22 Center catalog kinds; all 20 first-class resource paths occur in the Controller menu; Secret/ConfigMap share restricted dependencies | Catalog and nested ownership review complete; runtime requirements remain below |
| Check every menu | Current Center menu has 17 leaves; controller catalog paths have no omissions; current native 174-pass regression and separate OIDC/Kubernetes evidence | Current menu coverage indexed; runtime limits below remain explicit |
| Update Center backend | Federation ownership revocation, global write semantics, auth/cloud/error handling repairs; fresh 876 workspace + 247 no-default-feature app tests | Current Kubernetes image deployed; OIDC/capabilities and forced owner-Pod recovery verified |
| Update Center frontend | 848 tests in 117 files including the final Stream repair; Gateway/BTP fixes have focused browser/component evidence | Current unit/component and complete standalone browser regressions pass |
| Exercise local runtime | Isolated native Center + two mTLS Controllers; API readback, real Gateway traffic, and separate OrbStack deployments | Established for recorded scenarios; no blanket conformance claim |
| Commit Center only, do not push | Center task commits; Edgion ACME and outbound TLS repairs remain local/uncommitted; unrelated work preserved | Scope retained |

Source comparison revalidated at Center `6b97ed6` against current worktree files:
`/tmp/ws5-center-final-source-audit-20260928.json` records exact sets and SHA-256
hashes for all three source inputs. It confirms 22 kinds, 20 first-class routes,
the restricted-dependency menu and 17 Center leaves. No Edgion history was read.
Center implementation provenance is unchanged: `git diff --name-only ba012f3 HEAD`
contains only seven evidence documents; the backend-only diff from `7433d37` is
empty. The final frontend and native browser log summaries were re-read and
confirm 848 tests / 117 files and 174 passed / two skips, respectively.
Current resource/menu details: CURRENT-COVERAGE.md. The opening table in README.md
is the original historical checkpoint, not an up-to-date pending-work list.

## Fresh gates

- Current complete native browser regression: **174 passed, 2 Kubernetes-only
  skips**, 9.7 minutes. Both native programs rebuilt, SQL RBAC enabled, two real
  mTLS Controllers, all 70 retained fixture files verified. See
  [CURRENT-NATIVE-REGRESSION.md](CURRENT-NATIVE-REGRESSION.md) for provenance
  and the browser/API versus traffic evidence boundary.

- Latest full frontend: 848 passed / 117 files, 84.70 seconds;
  `/tmp/ws5-center-current-final-frontend.log`. This includes the final Stream repair.
  Its build/lint evidence is `/tmp/ws5-stream-scoped-fields-{build,lint}.log`.

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

## Closure evidence and final reconciliation

1. The dedicated nested operator/runtime ownership pass is now recorded for all
   catalog kinds in [FIELD-OWNERSHIP-AUDIT.md](FIELD-OWNERSHIP-AUDIT.md), including
   the route mirror/ExternalAuth repairs and HTTP/Stream opaque-data preservation.
   The current full frontend run passes 848 tests including the Stream repair,
   which also has focused build/lint evidence. This closes source
   ownership review, not the traffic/menu requirements below.
2. The selected frontend TLS transition gap is now verified by seven real
   handshake matrices: strict/fallback mode, clear-versus-delete port override,
   default inheritance and CA Secret rotation/restoration through Center. See
   [FRONTEND-TLS-TRAFFIC-EVIDENCE.md](FRONTEND-TLS-TRAFFIC-EVIDENCE.md).
   Selected AI backend/provider routing, typed policy attachment and
   resilience beyond active probes now have actual traffic evidence: endpoint
   edits, credential rotation and concurrency limit changes/deletion pass in two
   isolated runs with Center form/API mutations. See
   [AI-BACKEND-TRAFFIC-EVIDENCE.md](AI-BACKEND-TRAFFIC-EVIDENCE.md) for the exact
   OpenAI-compatible provider scope and the mechanisms not covered by that run.
   Existing configuration persistence must not be called traffic proof. Global outbound
   client identity resolution is repaired in the local, uncommitted Controller
   worktree; Center rejects unsupported global hostname/SAN constraints. See
   [GLOBAL-OUTBOUND-TLS-EVIDENCE.md](GLOBAL-OUTBOUND-TLS-EVIDENCE.md). Native
   inherited mTLS, Secret rotation, ReferenceGrant revocation/restoration and
   Secret deletion/recreation now pass after the local Gateway forced-rebuild
   invalidation repair. The persistent runner is also repaired: 80 runtime tests
   and a native Redis mTLS rotation/revocation/recovery scenario pass. The other
   persistent providers have no new provider-specific handshake proof from that run.
3. Ambiguous post-dispatch response loss now has native non-replay evidence:
   the Controller persists a CAS write, its encrypted response is dropped, the
   non-owner returns explicit uncertainty (503), recovery reads the same version,
   and Controller dispatch logs contain exactly one PUT. Eight checkpoints pass;
   see [AMBIGUOUS-WRITE-TRAFFIC-EVIDENCE.md](AMBIGUOUS-WRITE-TRAFFIC-EVIDENCE.md).
   Current-image owner-Pod failure recovery now passes ten native checkpoints,
   including explicit survivor ownership, increased fencing epoch, CAS write
   readback and restored authorization. Three current Kubernetes OIDC/capability
   browser checks also pass. See
   [CURRENT-KUBERNETES-FAULT-EVIDENCE.md](CURRENT-KUBERNETES-FAULT-EVIDENCE.md).
   New cloud error states retain component/HTTP adapter evidence rather than
   native external-provider mutations.
4. The complete current standalone browser regression now passes (174 tests,
   two Kubernetes-only skips). Rerun affected checks after subsequent fixes.
   The older Kubernetes v5 deployment has now been replaced by a current release
   image with the embedded dashboard; its fresh checks are recorded separately.

The selected runtime gaps listed at the earlier checkpoint now have concrete
evidence. Final requirement-by-requirement reconciliation against the full
resource/menu scope remains before marking the overall goal complete.

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
