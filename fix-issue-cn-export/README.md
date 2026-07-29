# `/fix-issue-cn` export

Exported on 2026-07-29 for inspection. Nothing here is loaded by the tooling — this
directory is a read-only copy.

## What `/fix-issue-cn` actually is

It is a two-part construct:

1. **The slash command** — `~/.claude/commands/fix-issue-cn.md` (user-global, applies to
   every project). It contains only a pointer, a Simplified-Chinese language override for
   user-facing narrative, and the argument-parsing rules. It carries no workflow logic.
2. **The skill** — the command tells the agent to read
   `skills/04-review/fix-review-issue.skill` *in the current project*. All of the workflow
   lives there. `/fix-issue` and `/fix-issue-cn` point at the same skill file; the only
   difference is the language override.

Because the skill path is resolved per project, `/fix-issue-cn` behaves differently in each
repository — and does not work at all in a repository that has no such file.

## Completeness of this export

The export is complete at the **workflow-logic** level: tier A + tier B below let you read
the whole procedure end to end without opening another file. It is deliberately *not*
complete at the **knowledge-corpus** level (tier C).

### Tier A — the feature itself

| File | Origin |
|---|---|
| `01-command-fix-issue-cn.md` | `~/.claude/commands/fix-issue-cn.md` |
| `02-skill-fix-review-issue--from-Edgion.skill` | `../Edgion/skills/04-review/fix-review-issue.skill` (453 lines) |
| `03-skill-fix-review-issue--from-Edgion-resource-ui.skill` | `../Edgion-resource-ui/skills/04-review/fix-review-issue.skill` (440 lines) |

The two skill copies have diverged: different module taxonomy, different Step 6 verification
command set, different Step 4b knowledge-base closing convention.

### Tier B — files the skill instructs the agent to read or execute

Copied under `deps-Edgion/` and `deps-Edgion-resource-ui/`, preserving repo-relative paths.

| Path | Where the skill uses it |
|---|---|
| `skills/04-review/SKILL.md` | P3 — always loaded; also Step 1 false-positive check |
| `skills/04-review/_template.md` | Step 4b — shape of a new finding file |
| `skills/04-review/CLOSED-FINDINGS.md` | Step 4b — central index (Edgion convention only) |
| `skills/SKILL.md` | Step 4b — "Key constraints" table check |
| `skills/11-doc/SKILL.md` | Step 4a — paired `docs/en/` + `docs/zh-CN/` rules |
| `skills/05-testing/01-integration-testing.md` | Step 4c — how to add an integration scenario |
| `cicd/checks/validate_agent_docs.py` | Step 6 — executed, must pass |
| `cicd/checks/check_ssa_force.py` | Step 6 — executed, must pass |

Note: `Edgion-resource-ui` also ships `CLOSED-FINDINGS.md`, `11-doc/SKILL.md`, and both
check scripts, even though its own skill text claims "there is no central index" and omits
the python checks from Step 6. Its skill file is stale relative to its own repository.

### Tier C — NOT exported: the accumulated finding corpus

`skills/04-review/SKILL.md` is an index into a per-repository corpus of closed findings:

| Repository | Files under `skills/04-review/` | Topic dirs | Size |
|---|---|---|---|
| `Edgion` | 401 | 46 | 2.8 MB |
| `Edgion-resource-ui` | 340 | 46 | 2.4 MB |

This is accumulated data, not workflow logic. P3 loads only the sub-file matching the
finding at hand, so no single run reads more than a few of these.

### Remaining dangling references, and why they stop here

Tier B files are routers. Following their references to closure pulls in essentially the
whole repository — `skills/SKILL.md` routes into all 583 files of `Edgion/skills/`,
`05-testing/01-integration-testing.md` describes the entire `edgion-tests/` tree,
`11-doc/SKILL.md` describes all of `docs/`. Those are the *subject matter* the workflow
operates on, not part of the workflow definition.

The other unresolved paths are runtime targets, not content to read:
`tasks/review-*/` (input findings), `tasks/todo/` (Step 1 option-A output),
`docs/en/` + `docs/zh-CN/` (Step 4a output), `edgion-tests/` (Step 4c output).

## Note on this repository

`EdgionCenter` has **no** `skills/04-review/fix-review-issue.skill`. Running
`/fix-issue-cn` from this repository therefore has no workflow to follow.
