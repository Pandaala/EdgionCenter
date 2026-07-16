# EdgionCenter — AI Agent Project Guide

## Project overview

EdgionCenter is the multi-cluster federation management center for Edgion. Controllers
dial its mTLS-only `FederationSync` gRPC service, publish cluster metadata, and answer
reverse watch, command, and proxy requests. The workspace intentionally provides two
deployable compositions:

| Binary | State and platform integration |
|---|---|
| `edgion-center-standalone` | SQLite or MySQL persistence, password/OIDC authentication, database RBAC, queryable SQL audit log |
| `edgion-center-kubernetes` | Database-free CRDs and Leases, OIDC, SubjectAccessReview authorization, structured stdout audit, replica forwarding |

Shared policy and interfaces live in `crates/center-core`; federation and data-plane
runtime code lives in `crates/center-runtime`; HTTP/auth composition lives in
`crates/center-app`. Platform code belongs only in the matching adapter and binary.
Do not add SQL dependencies to the Kubernetes graph or Kubernetes dependencies to the
standalone graph.

- Federation gRPC: `:12251` (strict mTLS and Controller SPIFFE identity)
- Admin API: `:12201`
- Probe: `:12200`
- Metrics: `:12290`
- Kubernetes replica forwarding: `:12252` (dedicated mTLS identity, never public)
- Dashboard: `web/`; `embed-dashboard` embeds `web/dist` in either binary
- Container builds are self-contained and require only this repository as source context

## Knowledge system

Read `skills/SKILL.md`, then load only the relevant subtree. For dashboard work also read
`web/skills/SKILL.md`. Shared resource schemas, coding rules, and broader integration
guidance remain canonical in `../Edgion/AGENTS.md` and `../Edgion/skills/SKILL.md`.

## Development rules

- Preserve the two binary and adapter dependency boundaries.
- Federation wire-contract or shared-schema compatibility changes require validation in both repositories.
- Run `cicd/integration/run-matrix.sh` for the hermetic full matrix. Real Kubernetes and
  MySQL tests are explicit opt-ins documented in `cicd/integration/README.md`.
- Build images with `cicd/build-image.sh --mode standalone|kubernetes`; do not invoke the
  Dockerfile directly.
- Keep code, comments, logs, agent instructions, and skill docs in English.
- Do not commit or push unless the user explicitly requests it.
