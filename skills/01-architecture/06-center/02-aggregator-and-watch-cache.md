# Aggregation and reverse watches

The shared in-memory read path is implemented in `crates/center-runtime/src/`:

- `aggregator.rs` tracks per-cluster Controller summaries and online state.
- `watch_cache/` manages per-Controller typed reverse-watch caches.
- `metadata_store.rs` builds the global view used by Admin API handlers.
- `poll.rs` schedules list/watch refreshes.

Center sends watch/list requests over each Controller's existing federation stream; the
Controller remains the data source. Responses are associated with the live session and
merged into the global read model. A Controller server-ID change causes the cache to
re-establish watches rather than trusting state tied to the prior process.

These structures are intentionally platform-neutral and in-memory. Durable controller
identity and platform authorization live behind core ports. Kubernetes CRD status is a
projection for operators and global reads, not an ownership oracle; command/proxy routing
uses Lease fencing. Standalone persists the directory to SQL but still uses the shared live
registry and caches for active streams.
