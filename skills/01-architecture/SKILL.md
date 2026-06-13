---
name: center-architecture
description: Entry point for EdgionCenter internals. Routes to the 06-center docs and maps src/ modules to their documentation.
---

# 01 Architecture

EdgionCenter internals. The federation hub design is documented under
[`06-center/`](06-center/SKILL.md); this page additionally maps each `src/` module to its
doc (or marks it as not yet documented).

## File list

| Topic | Entry |
|-------|-------|
| Center overview, fed-sync, aggregator, watch cache, persistence | [06-center/SKILL.md](06-center/SKILL.md) |

## src module → doc map

| `src/` module | Responsibility | Doc |
|---------------|----------------|-----|
| `fed_sync/` | Federation gRPC server, Controller registry, bidirectional stream | [06-center/01-fed-sync-server.md](06-center/01-fed-sync-server.md) |
| `aggregator/` | `ResourceAggregator` — merges Controller-published metadata | [06-center/02-aggregator-and-watch-cache.md](06-center/02-aggregator-and-watch-cache.md) |
| `watch_cache/` | `CenterWatchCache`, registry, reverse-Watch data flow | [06-center/02-aggregator-and-watch-cache.md](06-center/02-aggregator-and-watch-cache.md) |
| `metadata_store/` | `CenterMetaDataStore` | [06-center/02-aggregator-and-watch-cache.md](06-center/02-aggregator-and-watch-cache.md) |
| `db/` | `CenterDb`, SQLite `controllers` table | [06-center/03-persistence.md](06-center/03-persistence.md) |
| `api/` | Admin API handlers (consistency, region-route, IP-restriction, web) | TODO: not yet documented — see `src/api/` |
| `proxy/` | Controller proxy | TODO: not yet documented — see `src/proxy/` |
| `commander/` | Command dispatch to Controllers | TODO: not yet documented — see `src/commander/` |
| `cli/` | Binary startup / argument parsing | TODO: not yet documented — see `src/cli/` |
| `config/` | Config schema & defaults (ports, sync intervals) | [../02-features/SKILL.md](../02-features/SKILL.md) |
| `common/` | Shared infra: auth, conf_sync, fed_sync proto, observe, startup | TODO: not yet documented — see `src/common/` |

## See also

- [02-features/SKILL.md](../02-features/SKILL.md) — config schema and Admin API endpoints
- [04-review/SKILL.md](../04-review/SKILL.md) — review findings touching these modules
