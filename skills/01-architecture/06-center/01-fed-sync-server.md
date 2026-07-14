# Federation server and ownership

The wire contract is generated from `crates/center-runtime/proto/fed_sync.proto` and served
by `crates/center-runtime/src/federation/server.rs`. The bidirectional stream begins with a
validated Controller registration. Center then accepts heartbeat, list/watch, command, and
proxy responses while sending the corresponding requests in the reverse direction.

Federation is mTLS-only. The server validates the client certificate chain and binds the
certificate's SPIFFE identity to the registered Controller under the configured trust
domain. Missing TLS, missing trust domain, malformed registration fields, identity mismatch,
or registry capacity exhaustion fail before the session becomes usable.

Standalone keeps one live-session owner in process. Kubernetes obtains a per-Controller
Lease through `KubernetesLeaseCoordinator` before registering the session. The Lease holder
contains replica Pod name and UID; its fencing token and monotonic epoch are attached to the
session. Lease loss marks ownership invalid before stream cancellation.

Commands and proxy calls first attempt a valid local owned session. Otherwise Kubernetes
resolves the authoritative Lease and Pod UID with `KubernetesControllerOwnerLocator`, then
uses `crates/center-runtime/src/internal_forwarding/` to call the owner on `12252`. The
internal service requires a dedicated Center-only mTLS CA and exact peer SPIFFE URI, verifies
the target holder and fence, limits message sizes, and never recursively forwards. A stale
pre-dispatch fence may trigger one fresh owner resolution; ambiguous transport failures are
not replayed because mutations may already have executed.
