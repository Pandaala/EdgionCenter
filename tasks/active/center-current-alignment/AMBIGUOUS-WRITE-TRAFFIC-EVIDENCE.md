# Dispatched write with lost response: native non-replay evidence

Date: 2026-09-28. The current OrbStack Center deployment and image from
[CURRENT-KUBERNETES-FAULT-EVIDENCE.md](CURRENT-KUBERNETES-FAULT-EVIDENCE.md)
are unchanged. No production code change was required.

## Fault and oracle

The task Controller temporarily connects to its existing Kubernetes Service
through a loopback TCP relay on `50974`. The relay does not terminate, decrypt or
modify TLS. Once registration and non-owner proxy reads succeed, it withholds
Controller-to-Center encrypted bytes while allowing Center-to-Controller bytes.

One CAS PUT is sent through the non-owning Center replica. A separate direct
Controller read proves that the requested marker and a new resourceVersion are
already persisted while the caller's response remains pending and encrypted
return bytes are withheld. Only then does the harness destroy the relay's
connections, dropping the response. This is a post-dispatch fault, not a
pre-dispatch denial or stale-owner retry.

Existing Controller debug events under the narrowly enabled
`edgion_controller::fed_sync::fed_client` target record request ID, method and
path. The harness counts matching PUT dispatches after its baseline log offset;
it does not infer execution count from the resourceVersion increment. No body,
credential or decrypted TLS content is captured by the relay.

## Result

Eight checkpoints pass in `/tmp/ws5-center-ambiguous-20260928/result.json`:

- The actual write changes resourceVersion `259` to `273` before transport loss.
- The non-owner returns **503** with error
  `controller dispatch result is uncertain`.
- After automatic federation reconnection, a fresh proxied read returns the
  already-applied value with that same resourceVersion.
- Controller observes **exactly one** matching PUT request ID:
  `6a2e3525-5696-47cd-a376-10eca8b4a90b`. The request is not automatically replayed.
- The successful run restores its starting fixture value after verification.

The 503 contract follows `proxy_dispatch_status` in
`crates/center-runtime/src/internal_forwarding/mod.rs`: a dispatched error becomes
gRPC Unavailable with an explicit uncertainty message, and `proxy_error` maps
that to HTTP 503. Only pre-dispatch StaleOwnership is eligible for the bounded
retry in `crates/center-runtime/src/proxy.rs`.

## Retained attempts and restoration

The first start refused an occupied port before changing the Controller. Another
attempt proved the write landed but incorrectly expected only HTTP 502/504;
its 503 was the current cross-replica contract. These are retained in
`occupied-port.log`, `status-oracle.log` and `status-oracle-result.json`; they
are not successful full runs. The latter left its marker in the isolated task
fixture, which became the final run's starting value. The final harness also
restores its fixture in failure cleanup when the current value is still owned
by that run.

The final harness exits zero. The relay listener is gone; Controller is restored
to its original configuration and direct Service endpoint
`https://127.0.0.1:31650`, with normal info logging. Its retained process is
recorded in `restored-controller.json`. Both Center replicas observe it online
and reject the viewer's proxy access after temporary SAR grants are removed;
see `restored.json`. Existing Controller RBAC was not widened.

Artifacts and private scripts are under `/tmp/ws5-center-ambiguous-20260928`.
This closes the selected native ambiguous-dispatch/non-replay gap for a real
ConfigData CAS write through the non-owner path. It does not claim every possible
transport failure timing or external cloud-provider mutation is covered.
