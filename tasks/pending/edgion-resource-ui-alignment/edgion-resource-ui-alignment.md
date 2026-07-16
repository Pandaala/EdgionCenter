# Edgion Resource UI Alignment

## Meta

| Key | Value |
|---|---|
| Created | 2026-07-15 |
| Status | complete |
| Type | feature / compatibility / frontend |
| Priority | P0 |
| Branch | `feature/edgion-resource-ui-alignment` |
| Companion branch | `feature/center-resource-ui-contract` in `../Edgion-resource-ui` |

## Objective

Align the EdgionCenter dashboard with the current Edgion resource contracts. The
result must preserve every supported wire field, expose the complete operator
workflow for first-class resources, and work against at least two Controllers in
both standalone and Kubernetes Center modes.

## Non-negotiable invariants

1. Editing one field must not remove or rewrite unrelated wire fields.
2. Rust resource structs and generated CRDs are the wire-contract authority.
3. Edgion skills define product semantics; a skill that conflicts with Rust must
   be corrected in the same delivery.
4. Every structured editor must have YAML-to-form-to-YAML preservation tests.
5. Mutation controls must reflect the selected Controller's capabilities and
   authorization before a request is sent.
6. Secret values are never exposed by default. Secret support is limited to
   safe creation, replacement, and reference workflows unless the server
   explicitly provides a redacted read contract.
7. Runtime verification uses at least two Controllers and includes accepted,
   rejected, unresolved-reference, and conflict states.

## Deliverables

- Lossless editor and resource-capability foundations.
- Repairs for every known destructive schema drift.
- Complete EdgionBackendTrafficPolicy management.
- Complete Route, Gateway, plugin, ConfigData, LinkSys, and backend-policy forms.
- Unified conditions, authorization, topology, dashboard, and cross-resource UX.
- Controlled ConfigMap and Secret dependency workflows.
- Updated frontend and Edgion skills.
- Automated checks plus standalone and Kubernetes browser E2E evidence.

## Document index

| Document | Purpose |
|---|---|
| `00-context.md` | Requirement, repository baselines, skills, and environment |
| `01-design.md` | Solution assessment and code-level design |
| `02-implementation.md` | Ordered implementation increments and results |
| `03-subtasks.md` | Trackable implementation units and ownership |
| `04-resource-ledger.md` | Resource-by-resource contract and coverage ledger |
| `04a-field-boundaries.md` | Exact editable, protected, internal, and sensitive JSON paths |
| `05-verification.md` | Automated and runtime evidence plan |
| `05a-playwright-plan.md` | Executable browser harness, action inventory, and mode matrix |
| `06-decisions.md` | Design and review decisions |
| `07-changelog.md` | Increment-by-increment delivery record |
| `08-skill-corrections.md` | Verified documentation drift and correction ledger |

## Completion rule

The task is complete only when every row in the resource matrix is implemented
or explicitly covered by the restricted-dependency design, all phase gates pass,
both runtime modes pass the two-Controller browser suite, and independent review
has no unresolved correctness or security findings.
