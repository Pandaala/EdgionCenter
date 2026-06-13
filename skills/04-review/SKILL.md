---
name: center-review
description: EdgionCenter review findings, one file per finding, grouped by topic.
---

# 04 Review

Recorded review findings for EdgionCenter. One file per finding; grep/ls to discover.

## Topic groups

| Group | Findings |
|-------|----------|
| `architecture/` | [fed-proxy-header-forwarding.md](architecture/fed-proxy-header-forwarding.md), [register-validation-registry-caps.md](architecture/register-validation-registry-caps.md) |
| `cpu-memory/cases/` | [admin-api-controller-summaries-multi-call.md](cpu-memory/cases/admin-api-controller-summaries-multi-call.md), [centerdb-single-mutex-connection-not-blocking.md](cpu-memory/cases/centerdb-single-mutex-connection-not-blocking.md), [offline-controller-data-retention-business-requirement.md](cpu-memory/cases/offline-controller-data-retention-business-requirement.md), [watch-cache-registry-get-or-create-slow-path.md](cpu-memory/cases/watch-cache-registry-get-or-create-slow-path.md) |
| `h2-grpc/` | [fed-sync-keepalive.md](h2-grpc/fed-sync-keepalive.md), [heartbeat-timeout-pong-tracking.md](h2-grpc/heartbeat-timeout-pong-tracking.md) |

## See also

- [01-architecture/SKILL.md](../01-architecture/SKILL.md) — the modules these findings touch
- Edgion review conventions: https://github.com/Pandaala/Edgion/blob/main/skills/04-review/SKILL.md
