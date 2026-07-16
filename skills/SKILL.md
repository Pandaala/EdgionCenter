---
name: edgion-center-skills
description: Root router for the two-mode EdgionCenter workspace.
---

# EdgionCenter skills

EdgionCenter has a shared hexagonal core and two independent deployment compositions:
SQL-backed standalone and database-free Kubernetes-native. Start here, then load only the
smallest relevant subtree.

| Need | Entry |
|---|---|
| Crate boundaries, federation, ownership, persistence | [01-architecture/SKILL.md](01-architecture/SKILL.md) |
| Binaries, config, ports, auth, deployment | [02-features/SKILL.md](02-features/SKILL.md) |
| Validation matrices | [05-testing/SKILL.md](05-testing/SKILL.md) |
| Dashboard | [web/skills/SKILL.md](../web/skills/SKILL.md) |
| Historical review findings | [04-review/SKILL.md](04-review/SKILL.md) |

Resource schemas and shared Edgion conventions are intentionally not copied here. Use
`../../Edgion/skills/SKILL.md` from this workspace.
