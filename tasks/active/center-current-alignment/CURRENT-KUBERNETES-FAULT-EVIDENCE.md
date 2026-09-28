# Current Kubernetes deployment and owner failure evidence

Date: 2026-09-28. Test namespace: `eruie2e-c0c61d3a-system` on OrbStack.
This is the existing task-owned isolated deployment, not a managed cluster.

## Current image and connectivity

The repository build script completed successfully:

```sh
cicd/build-image.sh --mode kubernetes \
  -t edgion-center-kubernetes:ws5-alignment-current-36d771d --load
```

Build source is `36d771d`; subsequent Center commits before this test only added
evidence documents. Production implementation is unchanged from the current
848-test frontend and 174-pass standalone browser regression. The embedded
dashboard and release Kubernetes binary were rebuilt. No image or Git push ran.

Image ID: `sha256:a1447812eeacfdf2d117def92932d428643dc0383a6039e02c40a1425398f4d3`.
Build log: `/tmp/ws5-center-current-kube-image.log`. Both Center replicas were
rolled to this image, replacing the older v5 deployment.

The native Controller was restarted from the current workspace binary once
before testing, then kept running throughout both fault attempts. Its dedicated
configuration now points to `https://127.0.0.1:31650`, the federation NodePort of
the task-owned `alignment-current-federation` Service. The server certificate
covers that IP. This removes the old dependency on a single-Pod port-forward;
Controller reconnection uses Kubernetes Service routing with no manual endpoint
change during failure. The same test Service exposes the OAuth sidecar on local
port `31529` for authenticated API probes.

## Current-image checks

- Three browser checks pass: real OIDC authentication, authenticated identity,
  and Kubernetes capabilities hiding SQL administration/rejecting direct access.
  Log: `/tmp/ws5-center-kube-current-20260928/repo-browser.log`.
- Ten owner-failure checkpoints pass in `crash-result.json` in that directory.
  Before dispatch, the test verifies the exact owner Pod UID and current image.
- The non-owner initially rejects proxy access. A temporary, user-specific SAR
  grant scoped to `e2e-a/auth-a` enables proxy operations for the test. Controller
  RBAC is unchanged: its default policy still governs the tunneled request.
- Non-owner and Service reads return the same actual Controller ConfigData.
- Force-deleting the exact owner Pod with zero grace interrupts that owner.
  Retained Kubernetes events record container stopping; the namespace and the
  other replica remain intact.
- The final run moves ownership from `...-9mdsf` to the already-running
  `...-m5r2m`, with epoch **9 to 10**, a new non-empty fencing token and the
  survivor's explicit holder identity. The settled lease was observed after
  **16.008 seconds**; the first successful Service read followed at **16.017
  seconds**. These are observed fixture timings, not an availability SLA.
- A post-failure CAS write is read back from the actual Controller with its new
  resourceVersion. Reusing the old version returns **409**; Secret listing still
  returns **403**. The test restores its original ConfigData value.
- The temporary ClusterRole and binding are deleted, and proxy access again
  returns **403**. The task does not leave the viewer with elevated access.

After the final test, the Deployment returned to two ready current-image replicas.
Their runtime image IDs match the built digest. Pod-specific browser/API forwards
were refreshed, and both replicas independently confirm controller reads are
allowed while proxy access and `proxy:access` permission are absent. Evidence:
`pods-final.json`, `recovery-rollout-final.log` and `permissions-restored.json`.
The current environment and Service are retained for subsequent verification.

## Oracle and scope

The first attempt also recovered reads/writes, but its lease predicate accepted
an intermediate epoch increment while `holderIdentity` was absent. Its artifacts
are retained under `initial-oracle/`. The final repeat requires an explicit
surviving Pod holder, changed token and increased epoch together. It is the
authoritative takeover run; the earlier weak predicate is not reused as proof.

All artifacts and scripts are under
`/tmp/ws5-center-kube-current-20260928`, including before/after deployment and
Lease documents, exact Pod identity, observations and Kubernetes events.
Credentials and configuration backups remain private and outside Git.

This establishes automatic recovery after forced owner-Pod removal on the
current image. It does not simulate node power loss or prove ambiguous
post-dispatch response loss/non-replay; that separate fault remains in the
completion audit. No production code change was needed for this scenario.
