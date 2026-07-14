---
name: center
description: Federation, ownership, aggregation, and persistence architecture for both Center modes.
---

# Center architecture

Controllers establish bidirectional, mTLS-authenticated streams on `:12251`. Center
validates registration, maintains live sessions, aggregates metadata, initiates reverse
watches, and routes commands or HTTP proxy calls back through the owning stream.

Standalone uses one process plus SQLite/MySQL. Kubernetes mode is active-active: Controller
CRDs are the durable directory, a Lease per Controller elects and fences the replica that
owns its stream, and non-owner replicas forward operations over dedicated mTLS on `:12252`.
Lease holder plus Pod UID and fencing epoch are authoritative; projected CRD status is not
used for routing correctness.

| Topic | Detail |
|---|---|
| Runtime lifecycle | [00-overview.md](00-overview.md) |
| Federation protocol and security | [01-fed-sync-server.md](01-fed-sync-server.md) |
| Aggregation and reverse watches | [02-aggregator-and-watch-cache.md](02-aggregator-and-watch-cache.md) |
| SQL and Kubernetes persistence | [03-persistence.md](03-persistence.md) |

Authentication and authorization differ by composition. Standalone supports OIDC/password
providers and optional database RBAC. Kubernetes requires OIDC and delegates every protected
action to Kubernetes SubjectAccessReview. Both fail closed.
