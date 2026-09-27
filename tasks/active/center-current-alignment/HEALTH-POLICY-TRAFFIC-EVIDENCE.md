# Health policy form to live Gateway traffic

Checkpoint: 2026-09-28. The overall alignment goal remains active.

## Runtime and scope

Current Vite source on 15174 talks to deployed Kubernetes Center v5 through Dex
OIDC. Center forwards to the retained FS Controller `e2e-a/auth-a` (Admin 15911,
conf-sync 50963). A freshly built native Gateway listens on loopback 18090,
Admin 19101, probe 19100 and metrics 19190. The synthetic backend listens on
loopback 18091, with independent `/health`, `/status/503` and application paths.

This run deliberately adds a data plane to the retained Controller instead of
starting another Controller/Gateway integration topology. There is one Gateway
process and no Kubernetes CRD change. Controller and Gateway current builds
completed successfully; Gateway build log is
`/tmp/ws5-center-gateway-current-build.log`.

Seven run-owned resources form the chain: EdgionGatewayConfig, GatewayClass,
Gateway, Service, EndpointSlice, EdgionBackendTrafficPolicy and HTTPRoute.
Names are `center-health-flow`, using that namespace for namespaced objects.
Resources carry `alignment-run: health-flow-20260928`.

## Evidence

The browser edits the actual Backend Traffic Policy structured form through
Center's proxy, rather than sending a direct Controller policy update:

1. `/health` returns 200 to active probes; a request with Host
   `hc-alignment.example.com` reaches the backend and returns 200.
2. The form changes the probe path to `/status/503`. The proxied PUT returns 200,
   Controller readback contains the new path and a different resourceVersion,
   and repeated real Gateway requests converge to 503.
3. The form restores `/health`. A second successful PUT advances resourceVersion;
   Gateway requests recover to 200 with body `health-flow-backend`.

Access logs identify the same HTTPRoute throughout. During the unhealthy phase,
endpoint selection reports `BackendEndpointSliceNotFoundByRoundRobin`; after
recovery the same route succeeds. Backend counters independently record health
probes and application requests. The browser result records 340 probes and
seven application requests at its checkpoint.

Initial setup exposed the correct default loopback-upstream denial. Only the
run-owned GatewayConfig received `securityProtect.allowLoopbackUpstream: true`;
other Gateway configurations and the product default stayed unchanged. Center
already exposes this field and preserved it. Another initial probe used Node
fetch, which did not send the intended Host authority; it produced 404. The
final probe uses a separate unauthenticated Playwright HTTP context with the
explicit Host header. Neither setup failure is counted as passing evidence.

## Authorization restoration

For this isolated resource, the Controller default policy was reproduced with
explicit concrete kinds, adding only EdgionBackendTrafficPolicy `update`.
Center received a temporary exact-resource PUT grant plus required read paths.
No wildcard Controller grant or Secret read permission was introduced.

After the proof, the exact original Controller configuration bytes were restored
and reloaded. Remote access discovery confirms policy `update` is absent again.
Gateway traffic remains 200 after the reload. The temporary Center ClusterRole
and binding were removed; a subsequent proxy read returned 403. All three
restoration checks passed. The healthy policy and synthetic runtime remain for
review; tokens and private configurations remain outside Git.

## Artifacts and limits

`/tmp/ws5-center-health-flow-20260928/` contains `browser-proof.cjs`,
`browser-proof-v2.log`, passing `browser-result.json`, inspected
`policy-after-recovery.png`, `restored.json`, fixtures, access logs and private
runtime files. Gateway session 51350 and backend session 86493 were live at
this checkpoint; revalidate before reuse. The initial Gateway was stopped to
enable access logging before the final proof; no Gateway restart occurred
between the two form edits or during their traffic convergence.

This proves the HTTP active-health path from Center form through actual traffic,
including failure and recovery. It does not establish HTTPS/mTLS, GRPC/GRPCS,
outlier ejection, retry budgets, circuit-breaking or load-balancer algorithms.
No production source change was needed for this HTTP scenario. The embedded
Center v5 frontend was not rebuilt; the browser used current Vite source.
