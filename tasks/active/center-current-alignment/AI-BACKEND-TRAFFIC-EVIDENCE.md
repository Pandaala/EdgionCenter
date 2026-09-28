# AI backend routing and policy traffic evidence

Date: 2026-09-28. Center source: `b2848c2` (implementation unchanged from
`ba012f3`). No production code change was needed for these scenarios.

## Composition and boundary

The isolated native harness `/tmp/ws5-ai-runtime.cjs` starts Center, one
filesystem Controller, Gateway, Vite and two local HTTP provider fixtures. It
uses current native binaries, real Center-to-Controller mTLS federation and SQL
RBAC/password login. Its private configuration derives from the preceding full
browser run; generated credentials and certificates remain outside Git.

Center Admin/federation use `17601/17651`, Controller Admin/config sync use
`17701/17751`, Gateway Admin uses `17801`, Vite uses `17573`, and Gateway traffic
uses `18600`. The two provider fixtures listen on `18601/18602`. Gateway config
sync uses the existing isolated plaintext test profile, and fixture provider
traffic explicitly enables insecure HTTP and loopback egress. This is not an AI
provider TLS test.

The initial filesystem fixtures install GatewayClass/Gateway/config, one
EdgionBackend, a credential Secret, one AiProxy plugin and an HTTPRoute. The
route references the typed EdgionBackend without a Service port; the plugin uses
Weighted selection. All subsequent resource mutations pass through Center's
federation proxy. Endpoint and concurrency edits use the actual dashboard forms;
Secret replacement and policy creation/deletion use the authenticated Center API.
The harness never calls a managed Kubernetes API.

## Assertions

1. An OpenAI-compatible request traverses Gateway and reaches provider A with
   the configured endpoint prefix and `/v1/chat/completions` path. The model
   alias becomes the configured canonical model. The provider receives the
   resolved fixture credential, not the distinct caller Authorization value.
2. Editing the backend endpoint in the Center form moves subsequent successful
   requests to provider B without restarting any service.
3. Replacing the Secret through Center changes the credential observed by B.
   Results retain only `first`/`rotated` classifications, not credential values.
4. Creating an EdgionBackendTrafficPolicy targeting the AI backend produces
   `Accepted=True` in Controller's processed view and a Gateway policy entry.
   With `maxParallelRequests: 1`, one request is held at the provider while a
   second receives an error without reaching it. Releasing the first completes
   successfully.
5. Editing that policy in the Center form to `maxParallelRequests: 2` is read
   back from the Controller and Gateway. Two requests then reach the provider
   simultaneously and both complete successfully after release.
6. Deleting the policy through Center removes its Gateway entry and permits
   two simultaneous successful provider requests.

Publication alone is not the traffic oracle. The bounded concurrency probe
waits for provider arrival or a terminal response, releases every held request
in `finally`, and retries until the observed capacity matches the expected
generation. It does not use a fixed delay to declare policy convergence.

## Results and retained attempts

- Complete bounded run: `/tmp/ws5-center-ai-runtime-1790578054721/result.json`;
  **12 checkpoints passed**, 13 provider requests. Log:
  `/tmp/ws5-ai-runtime-bounded-first-pass.log`.
- Stability repeat: `/tmp/ws5-center-ai-runtime-1790578104021/result.json`;
  **12 checkpoints passed**, 15 provider requests, log `/tmp/ws5-ai-runtime.log`.
  Both runs exited zero and their owned Admin/Vite/traffic listeners were absent
  after cleanup. Request counts include bounded convergence probes.
- The earlier API-policy run also passed, but did not edit capacity through the
  browser. Its log is `/tmp/ws5-ai-runtime-policy-api.log`.
- Two harness failures remain explicit: the first read status from the raw
  storage endpoint instead of the processed view; another treated config-list
  publication as sufficient readiness and left a held request to time out.
  Logs: `/tmp/ws5-ai-runtime-storage-oracle.log` and
  `/tmp/ws5-ai-runtime-publication-race.log`. Neither is a passing gate or proof
  of a production defect. The final bounded probe supersedes their oracles.

The runner stops only its owned processes and retains private fixtures,
screenshots and results. This establishes selected AI routing, credential hot
reload, typed policy attachment and concurrency enforcement through Center.
It is not a claim of all provider protocols, TLS combinations, retry/outlier
mechanisms or external provider conformance.
