# Center Skills System — Skeleton & Root Entry — Design

- Date: 2026-06-13
- Scope: **Skeleton + root entry** (not a full content rewrite). Establish a coherent,
  navigable skills knowledge base for EdgionCenter so future center development has a
  solid foundation. Deep per-feature content is filled in later, during development.
- Repo: `github.com/Pandaala/EdgionCenter` (default branch `main`).
- Shared upstream: `github.com/Pandaala/Edgion` (default branch `main`).

## Background

EdgionCenter was extracted from the Edgion monorepo (Cargo.toml still uses a `path`
dependency on `../Edgion/edgion-resources`). The skills knowledge base was carried over
piecemeal during extraction and is currently incoherent:

1. **Backend `skills/` — fragmentary, no entry point.** Only three fragments exist:
   `01-architecture/06-center/`, `04-review/{architecture,cpu-memory,h2-grpc}/`, and
   `09-misc/center-db.md`. There is **no root `skills/SKILL.md`** (Edgion has one) and
   **no category-level `SKILL.md`** for any directory except `01-architecture/06-center/`.
   The numbering mirrors Edgion (01/04/09) but the tree is mostly empty.

2. **Frontend `web/skills/` — inherited from the Edgion Controller dashboard, not cleaned.**
   The tree is relatively complete (root `SKILL.md` + `01-architecture` / `02-patterns` /
   `03-resources` / `04-testing` / `04-center`), but the content still describes the
   **Edgion Controller** frontend: port `12101`, "20 CRUD pages", and dead references to
   paths that do not exist in Center (`ws2/skills/`, `../../skills/02-dashboard/`).
   `web/CLAUDE.md` likewise still opens with "Edgion Controller … 端口 12101".
   `web/skills/PLAN.md` is a stale Controller frontend development plan.

3. **Repo root has no `AGENTS.md` / `CLAUDE.md`** (Edgion has a full one as the top entry).

Net: the frontend tree is usable but dirty, the backend tree lacks a skeleton and entry,
and the repo has no top-level command center. This is the foundation to fix before
starting center feature development.

## Decisions (settled during brainstorming)

| # | Decision | Choice |
|---|----------|--------|
| D1 | Ambition | **Skeleton + root entry.** Create navigation, category indexes, fix stale refs. Defer deep content. |
| D2 | Two trees (backend `skills/` + frontend `web/skills/`) | **Keep two trees + a single root pointer.** Mirror Edgion: root `AGENTS.md` + `skills/SKILL.md` is the backend/global entry and points to `web/skills/SKILL.md` as the frontend subtree. |
| D3 | Shared content with Edgion (resource Schema, coding rules, testing framework) | **Reference, do not copy.** Center skills only author Center-specific knowledge (federation / aggregator / center-db / multi-cluster UI). |
| D4 | Language | **All English** — including the frontend cleanup (translate, not just de-stale). Matches the global "source files in English" rule and Edgion's backend skills. |
| D5 | Cross-repo link form | **Remote GitHub URLs**, never local `../Edgion/...` paths. Directories → `https://github.com/Pandaala/Edgion/tree/main/skills/<path>`; files → `https://github.com/Pandaala/Edgion/blob/main/skills/<path>`. |
| D6 | `web/skills/PLAN.md` | **Delete** (stale Controller dev plan; recoverable via git). |

## Conventions (inherited from Edgion, restated for Center)

- **Progressive disclosure**: root `SKILL.md` → category `SKILL.md` → specific file. Load
  only the smallest subtree the task needs.
- **File-extension semantics**: `SKILL.md` = directory entry (routes, not invoked directly);
  `*.skill` = invokable step-numbered workflow (YAML frontmatter `name:`/`description:`);
  `*.md` = reference / decision-rule doc, loaded on demand by an upstream hint.
- **Standard category `SKILL.md` template**: YAML frontmatter (`name`, `description`) →
  one-paragraph scope → navigation/file table → `See also` / `External (Edgion)` section.
- **External references** use the remote GitHub URL form from D5.

## Design

### A. Root entry layer (new)

**`AGENTS.md`** (+ **`CLAUDE.md`** symlink → `AGENTS.md`), structured like Edgion's:
- Project overview: `edgion-center` single binary; `edgion-resources` dependency (note the
  current local `path` dep and the planned git-rev pin per Cargo.toml TODO); `:12251`
  Federation gRPC and `:12201` Admin HTTP; optional embeddable web dashboard
  (`embed-dashboard` feature).
- Knowledge-system navigation rules (progressive disclosure, three-layer lookup).
- Common workflows (entry points for the most frequent center dev tasks).
- Pointers to `skills/SKILL.md` (backend/global) and `web/skills/SKILL.md` (frontend).
- **"External dependency — Edgion skills"** section listing which knowledge lives upstream
  (resource Schema, coding rules, testing framework) with remote URLs.

**`skills/SKILL.md`** (new root index — currently missing entirely):
- Navigation rules + three-layer locator.
- Quick locator table + directory overview.
- **External dependency section** → `https://github.com/Pandaala/Edgion/tree/main/skills/...`
  for shared knowledge (resource Schema, `03-coding`, `05-testing`, `07-tasks`).
- **Frontend subtree pointer** → `web/skills/SKILL.md`.

### B. Backend `skills/` skeleton (add category indexes; do not copy Edgion)

Keep Edgion-aligned numbering. This round:

| Directory | Action | Content |
|-----------|--------|---------|
| `01-architecture/SKILL.md` | **new** | Route to existing `06-center/`; add an "implementation map" table keyed by `src/` modules (`aggregator`, `fed_sync`, `watch_cache`, `metadata_store`, `db`, `api`, `proxy`, `commander`, `cli`, `config`, `common`). Link modules that already have docs; leave stub placeholders (marked TODO) for those that do not. |
| `02-features/SKILL.md` | **new dir** | Skeleton for Center config Schema, binary/deploy, auth bootstrap, Admin API endpoints. Shared parts reference Edgion `02-features`. |
| `04-review/SKILL.md` | **new index** | Route to existing `architecture/`, `cpu-memory/`, `h2-grpc/` findings. |
| `05-testing/SKILL.md` | **new** | Center testing skeleton; shared parts reference Edgion `05-testing`. |
| `09-misc/SKILL.md` | **new index** | Route to `center-db.md`. |
| `03-coding`, `07-tasks` | **not authored in Center** | Root `SKILL.md` external-dependency section references the Edgion directories directly (remote URL). |

Stubs created this round contain: frontmatter, a one-line scope, the routing/file table,
and explicit `TODO:` markers for unwritten sections — so the skeleton is navigable and
honest about what is not yet documented.

### C. Frontend `web/skills/` cleanup (de-stale + translate; not a content rewrite)

- `web/skills/SKILL.md` + `web/CLAUDE.md`: retarget from **Controller · 12101** to
  **Center · 12201**; rewrite to English; remove dead references (`ws2/skills`,
  `../../skills/02-dashboard`, "20 CRUD pages", "completed features" claims that describe
  the Controller dashboard).
- Resource Schema quick-reference tables: mark **authoritative source = Edgion**
  (`https://github.com/Pandaala/Edgion/tree/main/skills/02-features/03-resources`), per D3
  (reference, not copy). Frontend keeps page-development notes only.
- Keep the generic frontend patterns (`02-patterns/`: list-page / editor-modal /
  types-and-utils / i18n) intact — translate any non-English prose to English, but do not
  restructure.
- **Delete `web/skills/PLAN.md`** (D6).

### D. Out of scope (explicitly deferred)

- Authoring deep per-module architecture docs for every `src/` module (only stubs now).
- Writing `*.skill` workflows for Center (none created this round).
- Migrating `edgion-resources` from path dep to git-rev pin (tracked separately in Cargo.toml).
- Translating already-correct frontend pattern prose beyond what cleanup touches.

## Deliverables checklist

- [ ] `AGENTS.md` + `CLAUDE.md` symlink at repo root
- [ ] `skills/SKILL.md` root index (external-deps + frontend pointer)
- [ ] `skills/01-architecture/SKILL.md` with src-module implementation map
- [ ] `skills/02-features/SKILL.md` (new dir, skeleton)
- [ ] `skills/04-review/SKILL.md` index
- [ ] `skills/05-testing/SKILL.md` skeleton
- [ ] `skills/09-misc/SKILL.md` index
- [ ] `web/skills/SKILL.md` + `web/CLAUDE.md` retargeted to Center, English
- [ ] `web/skills/` resource tables marked "source: Edgion" with remote URLs
- [ ] `web/skills/PLAN.md` deleted
- [ ] All cross-repo links use `github.com/Pandaala/Edgion` remote URLs (no `../Edgion/` paths)

## Success criteria

- From a cold start, an agent reads `AGENTS.md` → `skills/SKILL.md` and can reach any
  existing Center doc (backend or frontend) via the navigation, without guessing paths.
- No skill file references a local sibling path (`../Edgion/...`) or a non-existent path
  (`ws2/skills`, `02-dashboard`).
- No skill file claims Center is the "Controller" or uses port `12101`.
- Every category directory has a `SKILL.md` entry point.
- All skill content is in English.
