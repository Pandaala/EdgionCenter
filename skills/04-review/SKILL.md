---
name: center-review
description: EdgionCenter review findings, one file per finding, grouped by topic.
---

# 04 Review

Recorded review findings for EdgionCenter. One file per finding; grep/ls to discover.

## Topic groups

| Group | Findings |
|-------|----------|
| `architecture/` | [center-proxy-permission-method-split-only.md](architecture/center-proxy-permission-method-split-only.md), [center-writes-use-unfenced-proxy-forward.md](architecture/center-writes-use-unfenced-proxy-forward.md), [dashboard-write-path-no-preflight-access-fetch.md](architecture/dashboard-write-path-no-preflight-access-fetch.md), [register-validation-registry-caps.md](architecture/register-validation-registry-caps.md), [reload-outcome-not-just-dispatch.md](architecture/reload-outcome-not-just-dispatch.md) |
| `cpu-memory/cases/` | [admin-api-controller-summaries-multi-call.md](cpu-memory/cases/admin-api-controller-summaries-multi-call.md), [centerdb-single-mutex-connection-not-blocking.md](cpu-memory/cases/centerdb-single-mutex-connection-not-blocking.md), [offline-controller-data-retention-business-requirement.md](cpu-memory/cases/offline-controller-data-retention-business-requirement.md), [watch-cache-registry-get-or-create-slow-path.md](cpu-memory/cases/watch-cache-registry-get-or-create-slow-path.md) |
| `h2-grpc/` | [fed-sync-keepalive.md](h2-grpc/fed-sync-keepalive.md), [heartbeat-timeout-pong-tracking.md](h2-grpc/heartbeat-timeout-pong-tracking.md) |
| `observability/` | [watch-kind-label-comes-from-watch-state.md](observability/watch-kind-label-comes-from-watch-state.md) |

## See also

- [01-architecture/SKILL.md](../01-architecture/SKILL.md) — the modules these findings touch
- Edgion review conventions: https://github.com/Pandaala/Edgion/blob/main/skills/04-review/SKILL.md
