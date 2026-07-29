---
name: center-writes-use-unfenced-proxy-forward
description: Use when reviewing `center-runtime/src/proxy.rs` or the `EdgionConfigData` CAS write path and a finding claims Center writes must pin the request to the ownership epoch the caller observed — i.e. proposes reintroducing `ControllerHttpClient`, `forward_expected_route`, or `forward_expected_session`; explains why those owner/session-fenced dispatch paths were deleted rather than re-sourced.
---

# Center Writes Dispatch Through Unfenced `ProxyForwarder::forward` — BY DESIGN

## Conclusion

Every Center-initiated write against a Controller resource goes through
`ProxyForwarder::forward` (`crates/center-runtime/src/proxy.rs`), which resolves the owning
replica at dispatch time. It deliberately does **not** pin the request to an ownership epoch
supplied by the caller.

Fix suggestions of the form "a CAS write must be dispatched against exactly the owner route
the caller observed, so the outcome can be attributed to a known revision", or "reintroduce
the owner-fenced / session-fenced dispatch path for the write model", are **not accepted**
on their own. The premise assigns the write's correctness to the transport layer, where it
does not live.

`ControllerHttpClient` (`poll.rs`), its `ProxyForwarder` impl, `forward_expected_route`, and
`forward_expected_session` existed until 2026-07-28 and were deleted after their last callers
went with the effective-state poller and the durable desired-state sync.

## Core Rationale

**1. CAS, not epoch pinning, is what makes the write attributable.**

The authoritative write model
(`docs/superpowers/specs/2026-07-26-center-controller-write-model-convergence-design.md` §3)
states that all writes go through the generic proxy CRUD carrying a CAS precondition:
`If-Match` on `metadata.resourceVersion`, taken from the local watch cache. A write that
races anything else fails the precondition and returns 409; it can never silently overwrite.
Which Center replica happened to carry the request to the Controller does not change that —
the Controller applies the precondition regardless of the hop. Owner fencing would protect
a property the CAS precondition already owns.

**2. The "never replay an ambiguous transport failure" invariant does not live in the fenced
path.**

`forward`'s bounded retry (`proxy.rs`, the `attempt` loop) fires on exactly one error kind,
`ForwardErrorKind::StaleOwnership`. On the receiving side, `forward_fenced_local` returns
`FencedProxyError::StaleOwnership` only *before* the request reaches the Controller session
(no session, or an ownership mismatch), and anything after dispatch becomes
`FencedProxyError::Dispatch`, which `proxy_dispatch_status`
(`crates/center-runtime/src/internal_forwarding/mod.rs`) maps to `Unavailable`
("controller dispatch result is uncertain") — a kind `forward` never retries. The retry is
therefore confined to requests that were never executed, and that is now pinned by
`uncertain_dispatch_failures_are_never_replayed` and
`stale_ownership_is_retried_exactly_once_against_a_newer_owner` in `proxy.rs`. Deleting the
fenced client paths does not weaken the invariant, because the invariant was never enforced
there.

**3. Fence enforcement itself was kept.**

Only the *client-side* fenced entry points were removed. `ProxyForwarder::forward_fenced_local`
— the replica-hop server side that actually validates holder and fencing token before
dispatching — remains, with its production caller in
`InternalForwardingService::forward_http`. Replica forwarding is still fenced end to end.

**4. Convergence observation is a cache property, not a dispatch property.**

`center-app/src/api/config_data_ops.rs` observes convergence by polling the *local* watch
cache, and only a replica that owns the session can observe it at all
(`local_session_is_dispatchable`). A non-owning replica's write terminates in `Accepted` by
construction. Pinning the dispatch to an epoch would not give a non-owning replica anything
to observe.

## Consequences Accepted

- A write may be carried by a different owning replica than the one the caller last observed.
  That is correct: the CAS precondition, not the route, decides whether it lands.
- `forward_local` no longer takes an `expected: Option<(&str, &OwnershipFence)>` argument; its
  only non-`None` caller was `forward_expected_route`. Ownership-checked local dispatch is
  reached through `forward_fenced_local`.
- The `operation="proxy_fenced"` label on `edgion_center_internal_forward_total` is gone. It
  was never emitted in production — its only emission points sat on the unreachable path.

## Re-evaluation Triggers

Re-open this decision only if:

- The write model stops carrying a CAS precondition on every mutation, so the transport is
  once again the only thing that can bound a write. §3 of the spec calls a write without a
  version precondition a bug, so this would be a deliberate design reversal.
- A Center-initiated operation appears that is genuinely non-idempotent and cannot be
  expressed as a CAS write. That operation needs a fenced dispatch path of its own,
  designed with it — not this one restored speculatively.

## Reference Cases

- `20-retire-or-reuse-fenced-forwarding` (Size S/M), closed 2026-07-28 by deleting `poll.rs`,
  the `ControllerHttpClient` impl, both `forward_expected_*` methods, the now-unused
  `expected` parameter of `forward_local`, and the orphaned `internal_forwarding::expected_fence`
  helper that produced that argument.
- Source: `crates/center-runtime/src/proxy.rs` (`forward`, `forward_fenced_local`),
  `crates/center-runtime/src/internal_forwarding/mod.rs` (`proxy_dispatch_status`),
  `crates/center-app/src/api/config_data_ops.rs` (the CAS write core).
