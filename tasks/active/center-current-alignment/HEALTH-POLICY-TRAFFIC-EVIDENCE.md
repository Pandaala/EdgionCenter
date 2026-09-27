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


## HTTPS follow-up: verified identity, failure and recovery

The next run uses the same Center form, Controller and Gateway, with a separate
CA and HTTPS health server on port 18092. The application still serves plaintext
HTTP on 18091. Only the run-owned EndpointSlice moved from loopback to the host's
private interface; no BackendTLSPolicy was installed. The CA reference is the
run-owned Secret `center-health-flow/center-health-probe-ca`, seeded directly
with the private Controller admin credential. Center never received Secret
read access.

The structured form changes the active type to `https`, sets port 18092 and
writes the Probe TLS JSON with its CA reference and validation hostname:

1. `probe.health.example.com`: actual HTTPS probe requests advance the server
   counter and application traffic returns 200.
2. `wrong.health.example.com`: the policy PUT succeeds, but verified probes fail
   and Gateway traffic converges to 503.
3. Restoring `probe.health.example.com` resumes actual HTTPS probes and traffic
   returns 200 with `health-flow-backend`.

All three edits advanced Controller resourceVersion and passed exact readback
assertions. Healthy steps wait for more than two additional HTTPS requests
before checking application traffic. At the final checkpoint the probe server
recorded eight requests, `/health`, and the expected TLS host authority.
The inspected screenshot displays the HTTPS health badge in the policy menu.

The first attempt used loopback and failed recovery. Current encrypted probes
explicitly call endpoint validation with loopback disabled, independently of
GatewayConfig's business-upstream setting. Its initial transient 200 was stale
health state, not proof of a successful TLS exchange; that attempt is excluded.
The passing run uses a private interface and a stronger request-counter oracle.
No product source change was required.

Artifacts: `/tmp/ws5-center-encrypted-health-20260928/`, including
`browser-proof.cjs`, passing `browser-result.json`, `probe-stats.json`, inspected
`recovered.png`, and passing `restored.json`. Private keys/configuration stay
outside Git. The retained LAN backend process is session 50259; revalidate it
and the host address before reuse. The original loopback backend is retained.
The active policy now uses HTTPS with the correct hostname.

The exact Controller configuration was restored and reloaded; remote policy
update permission is absent, Gateway traffic remains 200, and removal of the
Center grant makes proxy read return 403. All three restoration checks passed.
This checkpoint extends the HTTP proof to HTTPS identity validation. The next
section records the subsequent mTLS and GRPC/GRPCS proof. The frontend unit/build/lint baseline
was not rerun for this runtime-and-documentation-only follow-up.


## mTLS and gRPC follow-up

The same current Center form and retained data plane passed 14 further edits.
The HTTP application remains on 18091. Separate private-interface probe servers
require HTTPS mTLS on 18093, plaintext gRPC on 18094, GRPCS on 18095, and GRPCS
mTLS on 18096. The gRPC fixture implements the standard unary
`/grpc.health.v1.Health/Check` exchange over HTTP/2, with protobuf SERVING or
NOT_SERVING responses and gRPC status trailers. It is a synthetic protocol
fixture, not a production application server.

| Probe | Form changes | Actual Gateway traffic |
|---|---|---|
| HTTPS mTLS | No client reference; trusted client; untrusted client; trusted client | 503, 200, 503, 200 |
| gRPC | `alignment` service; `not-serving`; `alignment` | 200, 503, 200 |
| GRPCS | Correct hostname; incorrect hostname; correct hostname | 200, 503, 200 |
| GRPCS mTLS | No client reference; trusted client; untrusted client; trusted client | 503, 200, 503, 200 |

The trusted client is signed by the probe server's trusted CA; the untrusted
client is a valid identity signed by a separate CA. Both live in run-owned TLS
Secrets, seeded directly through the private Controller admin API. Center's
permission remains only the concrete policy update and necessary read paths;
no Secret read permission was added.

Every browser edit received a successful proxied PUT and advanced Controller
resourceVersion. Readback verifies protocol, port and service name; the HTTPS
mTLS run additionally checks the exact client reference. Healthy checks require
more than two additional successful requests at the selected probe server, so
old healthy state alone cannot pass. Both mTLS servers recorded an authorized
peer with CN `center-health-trusted`. Final counters were seven HTTPS mTLS
requests, nine plaintext gRPC requests, eight GRPCS requests and seven GRPCS
mTLS requests. The inspected final screenshot shows the GRPCS menu badge.

Artifacts are `/tmp/ws5-center-mtls-health-20260928/`: `browser-proof.cjs`,
`browser-result.json` (four checks), `grpc-proof.cjs`, `grpc-result.json`
(ten checks), probe counters, `grpc-recovered.png`, and `restored.json`.
Private certificate keys and Controller configuration are not committed.
Retained probe processes are sessions 69800 (HTTPS mTLS) and 23035 (gRPC
variants); revalidate them before reuse.

Afterward, the policy's entire active-health configuration was restored to its
pre-run HTTPS configuration on 18092. Exact Controller configuration bytes were
restored and reloaded. Remote update permission disappeared, Gateway traffic
remained 200, and removing the temporary Center grant returned proxy read to
403. All three restoration assertions passed.

No production source changes were needed. These runs establish the Center form
integration with HTTP, HTTPS, gRPC and GRPCS probes, including basic identity
failure and recovery. They do not establish TCP-only probes, every TLS option,
certificate rotation, outlier ejection, retry budgets, circuit-breaking or
load-balancer algorithms. The overall alignment goal remains active.
