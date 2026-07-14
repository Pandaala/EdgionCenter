# Runtime overview

Both binaries compose the same platform-neutral federation and Admin API layers. They bind
FederationSync on `12251`, Admin HTTP on `12201`, probes on `12200`, and metrics on `12290`.
Kubernetes additionally binds internal replica forwarding on `12252`.

```text
Controllers -> mTLS FederationSync -> center-runtime -> Admin API/dashboard
                                      |              |
                                      |              +-> command/proxy response paths
                                      +-> directory, ownership, audit adapters
```

Common startup rules are fail-closed: federation mTLS and peer trust identity are required;
protected Admin routes require a valid authentication provider and authorizer; required
platform persistence or Kubernetes API preflight failures prevent listeners from binding.

Standalone composition is in `bins/edgion-center-standalone/src/cli/mod.rs`. It opens the
configured SQLite/MySQL store before serving and installs SQL controller, user/RBAC, and
audit adapters. Kubernetes composition is in `bins/edgion-center-kubernetes/src/lib.rs`. It
validates OIDC and two distinct mTLS trust roots, performs Kubernetes directory/Lease/SAR
preflights, then starts federation, internal forwarding, Admin, probe, and metrics tasks.

`crates/center-runtime/src/federation/server.rs` owns stream registration and teardown.
`registry.rs` holds live sessions; `aggregator.rs`, `watch_cache/`, and `metadata_store.rs`
hold in-memory read models. A disconnect or heartbeat expiry invalidates the live session;
durable directory data remains available and online state is projected by the active mode.

Kubernetes shutdown sets platform readiness false, rejects new work, cancels ownership
tasks and sessions, and drains listeners. Ownership loss invalidates a session before any
additional local dispatch can be enqueued.
