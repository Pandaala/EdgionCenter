# Subtasks

| ID | Status | Deliverable |
|---|---|---|
| UI-00 | done | Approved design, two-repository isolation, ledger, baseline, environment and E2E evidence plan |
| UI-01 | done | Operator projection, mutation envelope, capability registry, conditions, action gating and tests |
| UI-02 | done | All destructive schema-drift repairs and preservation fixtures |
| UI-03 | done | Complete EdgionBackendTrafficPolicy UX |
| UI-04 | done | Complete HTTP, gRPC, TCP, UDP and TLS route UX |
| UI-05 | done | Complete Gateway, GatewayClass and EdgionGatewayConfig UX |
| UI-06 | done | Complete HTTP/stream plugins and ConfigData UX |
| UI-07 | done | Complete LinkSys, backend dependency and restricted Secret/ConfigMap UX |
| UI-08 | done | Conditions rollout, topology, dashboard, multi-Controller comparison and skills |
| UI-09 | done | Standalone and Kubernetes two-Controller Playwright matrices |
| UI-10 | done | Full audit, reviews, retained environment and merge handoff |

## Ownership rules

- An implementation subagent receives one resource adapter/editor family or one
  server protocol boundary. It must not modify another assigned family.
- The main agent owns shared document semantics, cross-repository contract
  convergence, plan state, runtime environments, and final integration.
- Every UI increment includes its preservation fixtures and direct tests.
- Review agents are read-only unless a separate implementation task explicitly
  assigns fixes.

## UI-00 gate result

Closed after three independent reviews. The final review approved formal
implementation with no remaining Must Fix. Initial UI-01 files remain
uncommitted and are rechecked against the approved mutation/access contracts.

## UI-02 gate result

Closed after the integrated review's two findings were fixed: the common
mutation boundary now rejects missing/unsupported API versions while allowing
only the three documented compatibility versions, and an unattached HTTPRoute
is accepted. Secret and ReferenceGrant also use the common lossless mutation
boundary; redaction sentinels cannot be written back. Final follow-up review
approved the phase with no Must Fix.

## UI-03 gate result

Complete EBTP CRUD, list, authorization, conditions, and structured Form/YAML
editing landed for every current Rust field. Independent review found and
closed narrow-edit loss in `healthCheck`, the valid `port: 0` boundary, and a
status-code draft transition that could silently reuse an old value. Final
focused evidence is 4 files/14 tests, including real Form-create and YAML-update
request payload assertions.

## UI-04 gate result

All five Route families now have complete structured multi-rule editing and
protocol-aware validation. Independent review findings closed the runtime
annotation matrix, narrow filter switching, full ExternalAuth configuration,
retry/delegation constraints, and real Form/YAML submit coverage. The final
review follow-up approved the phase with no Must Fix. Full gate: 39 files/152
tests, typecheck, lint, build, and diff checks; final SAN/JSON-value corrections
also passed 3 files/9 focused tests.

## UI-05 gate result

Gateway v1.5, GatewayClass, and the current Rust EdgionGatewayConfig contract
have complete structured editing, shared Form/YAML validators, and real submit
tests. Review findings closed global TLS, custom protocol/address/selector
support, listener cross-field validation, and status features. The companion
Edgion CRD ReferenceGrant default was aligned across Kubernetes and standalone
and pinned by a Rust test. Final review approved with no Must Fix.

## UI-06 gate result

The plugin editors now match the current Rust contracts: all 39 HTTP plugin
variants, their four legal stages and cardinality rules, both stream stages,
and all five ConfigData variants have structured, lossless editing. Narrow type
switches preserve the resource envelope while mutation stripping removes only
runtime, resolved, status, and redacted fields. Independent review approved
the phase with no Must Fix. The gate passed 46 files/178 tests, typecheck,
lint, production build, and diff checks.

## UI-07 gate result

All six current LinkSys variants, BackendTLSPolicy, Service, EndpointSlice, and
restricted Secret/ConfigMap dependencies now have contract-complete structured
editing and lossless mutation boundaries. Runtime SecretSlot/TLS fields are
stripped per variant, LinkSys type/target switches preserve drafts, and real
Form/YAML request tests cover every family. Secret/ConfigMap deliberately expose
only metadata keys plus create/write-only replace; value read, delete, and batch
delete remain forbidden. Other first-class lists enforce fail-closed CRUD and
exact-selection batch delete. Two review rounds closed six initial and two
follow-up Must Fix groups; final review approved. Full gate: 60 files/232 tests,
typecheck, lint, production build, and diff checks.

## UI-08 gate result

Conditions are now projected consistently across all condition-bearing lists
and details. The topology resolves the complete current resource graph and
distinguishes missing, unavailable, unknown, rejected, conflict, and not-ready
states, including ReferenceGrant enabled/disabled/unknown semantics. The fleet
dashboard adds per-kind/cluster/Controller counts, health and ACME summaries,
standalone diagnostics conflicts, and operator-projection drift comparison.
Independent review closed condition truth, partial-availability, reference
parsing, EndpointSlice fingerprint, ReferenceGrant, and Wasm reference issues;
final review approved. Full gate: 60 files/234 tests plus typecheck, lint,
production build, and diff checks.

## UI-09 gate result

The executable inventory covers 21 resource kinds, six state families, and 213
implemented action selectors. The final standalone run passed all 104 cases
against exactly two connected Controllers. The final Kubernetes run passed 103
of 105 cases; the two SQL-admin-only audit/user/role cases were intentionally
skipped because Kubernetes mode uses OIDC and Kubernetes authorization. Both
runs had zero failures and exercised both resource sets, authorization states,
navigation, safe mutations, retry behavior, dashboard, and topology.

## UI-10 gate result

All static, Rust, integration, policy, diff, runtime, and browser gates passed.
Independent reviews approved the frontend/Center boundary, Kubernetes discovery,
storage/watch namespace isolation, and the final ACME/DNS namespace-scoped
background scans. The retained OrbStack run has exactly two Ready Controllers,
73 ledger-owned objects in four task namespaces, no cluster-wide Secret list
permission, and no Controller `Forbidden` errors. No commit or push was made.
