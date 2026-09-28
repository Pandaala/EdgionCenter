# EndpointSlice source readiness and Gateway traffic

## Current authority and repair

The current sibling Gateway's `backends/discovery/coordinator.rs` builds
EndpointOrigin.ready from conditions.ready with unwrap_or(false), then includes
only ready slots. The Center list already counts explicitly ready endpoints,
but topology marked only explicit false as not ready. Missing/null ready was
therefore not marked even though the Gateway excluded those endpoints.

Topology now uses the same explicit-true rule. Serving alone is insufficient;
terminating does not override ready=true in the current implementation. This
shows source readiness, not active health-check state or complete route eligibility.
The node DOM exposes its full graph identity for unambiguous inspection where
several EndpointSlices contain the same address.

## Runtime evidence

Artifacts: `/tmp/ws5-center-endpoint-ready-20260928/` (`proof.cjs`, `result.json`,
state screenshots and focused node screenshots). Seven browser/traffic checks pass:

| Endpoint conditions | Gateway status | Center topology |
| --- | --- | --- |
| ready=true | 200, region-east | No not-ready badge |
| ready=false, serving=true | 503 | Not ready |
| Conditions absent | 503 | Not ready |
| ready=null | 503 | Not ready |
| serving=true only | 503 | Not ready |
| ready=true, terminating=true | 200, region-east | No not-ready badge |
| ready=true restored | 200, region-east | No not-ready badge |

The missing/restored node screenshots were visually inspected. These are actual
Controller source reads through Center and actual Gateway requests, not injected
browser responses. Fixtures are the three resources named `center-endpoint-ready`
(Service, EndpointSlice, HTTPRoute), namespace `center-region-flow`, labeled
`alignment-run: endpoint-ready-20260928`. The new host is
`endpoint-ready-alignment.example.com`; it uses the existing owned Gateway on
18200 and backend on 18201. Existing resources were not modified.

The external harness uses direct Controller admin writes to prepare and transition
its owned fixtures. Center reads stay within default permissions; this does not
claim Center write permission for these kinds. Every update uses resourceVersion
CAS. The final owned EndpointSlice is ready=true and the route returns 200;
fixtures are retained. No policy, Secret or CRD changed.

## Validation and audit scope

29 tests pass across topology construction, rendering and core-resource adapters,
including seven readiness cases. Build and lint pass, with the existing bundle
warning. Logs: `/tmp/ws5-center-endpoint-ready-tests.log`,
`/tmp/ws5-center-endpoint-ready-build.log`, `/tmp/ws5-center-endpoint-ready-lint.log`.
The previous full frontend checkpoint remains 716 tests; this is additional
focused coverage, not a new full-suite count. No backend code changed.

Service and EndpointSlice menus, editor adapters and current source layouts were
reviewed. Their guide now describes existing permission-gated CRUD, Form/YAML
support, retained operator fields and top-level EndpointSlice fields. This pass
does not prove ExternalName DNS, every Service networking option or active probe
behavior. Those are separate runtime concerns; the overall alignment stays active.
