# Deployed Kubernetes replica forwarding evidence

Checkpoint: 2026-09-28. Uses the isolated runtime described in
[KUBERNETES-AUTH-EVIDENCE.md](KUBERNETES-AUTH-EVIDENCE.md), with a native current
Controller connected to exactly one deployed Center Pod over federation mTLS.
All evidence is local; no shared Gateway CRD or Edgion source was changed.

## Initial v4 deployment results

Controller `e2e-a/auth-a` uses a dedicated file-system directory and loopback
Admin/conf_sync ports. Its certificate is bound to that exact Controller SPIFFE
identity. The default Controller federation policy is unchanged. Temporary
Center-side SAR grants allow the fixture viewer to use only this Controller's
proxy URL; the grant does not add Controller wildcard RBAC.

Five scenarios passed before migration and again after migration:

1. Owner and non-owner proxy reads return the same Controller object.
2. A non-owner CAS update changes the resource read directly from the Controller.
3. A stale version with a different desired value returns 409.
4. Secret reads and Service writes remain forbidden by the Controller.
5. Remote-forward metrics confirm the internal hop; after migration, the former
   owner's successful remote-forward counter increased by exactly five requests.

The initial fixture omitted `resourceVersion`, so its first purported stale-CAS
assertion did not send a valid precondition. That failed harness result is
retained. Subsequent proofs assert a nonempty version before dispatching CAS.
It was not a lost-header defect.

Moving the Controller's federation connection to the other Pod changed the
Lease holder/Pod UID and advanced its epoch from 1 to 2. Direct internal gRPC
checks used the generated protocol and the dedicated internal CA, with server
name verification enabled. Current fences succeed; stale tokens, wrong holders
and hop count 2 return `FailedPrecondition`. After migration, the new owner also
rejects the old token.

## Confirmed lifecycle defect and repair

The old owner's retained offline session still considered its ownership valid
after normal session cancellation released the Lease. An internal request with
that old, previously valid fence reached the dispatch helper and returned
`Unavailable` (14), even though it could not be sent to the Controller. The
caller therefore treated a safe pre-dispatch stale route as an uncertain result
and could not perform its bounded owner re-resolution. No stale request executed.

`maintain_ownership` now invalidates the shared ownership flag before releasing
the Lease on normal cancellation too. The existing blocked-release test now
asserts this ordering: it failed before the fix and passes after it. This keeps
the existing rule that failures after possible dispatch are never replayed.

Runtime unit tests passed: 153, including the retry/no-replay tests. The backend
matrix passed 873 workspace tests and 244 app tests without default features,
formatting, Clippy, dependency isolation and manifest rendering. Its final exit
was 1 at the existing `fix-issue-workflow-generic.zh.md` English-only violation.
The subsequent no-legacy guard and a separate workspace/all-targets `cargo check`
passed. Frontend unit checks were not repeated for this backend-only behavior
change; deployed browser cases were rerun as described below.

## Final v5 deployment verification

Both ready replicas run `edgion-center-kubernetes:alignment-forward-20260928-v5`.
The local inspected image ID is
`sha256:6973d4055938ed7ba11b74cacf92d386fdb8087c19a3dfa8c67a523f66ba3a57`.
All 22 proxy/fencing checks passed before and after connection migration. The
Lease epoch advanced from 3 to 4. The previous owner's formerly valid fence now
returns `FailedPrecondition` (9), confirming the lifecycle fix on the deployed
binary. The new owner accepts its current fence and rejects the previous token.

The migration observation initially expired at 60 seconds while the Controller
was in its existing 60-second reconnect backoff. Subsequent inspection confirmed
the new holder/epoch; tests continued without restarting the process or repeating
the migration. This timeout was not a failed deployment or ownership takeover.

The temporary proxy ClusterRole and binding were deleted. Both replicas then
returned 403 for the viewer's proxy request, retained 200 for Controller listing,
and omitted `proxy:access` from identity discovery. The three repository OIDC,
identity and Kubernetes capability browser cases passed again (3.1 seconds).
The root agent guide was also corrected to remove its obsolete Command-channel
claim; the federation protobuf remains unchanged.

## Artifacts and scope

Private root: `/tmp/ws5-center-kube-forward-20260928`.

- `proxy-result.json`, `proxy-migrated-result.json`: passing v4 proxy proofs.
- `metrics-*.txt`: per-Pod remote forwarding counters.
- `lease-before.json`, `lease-pre-migration.json`, `lease-after-migration.json`:
  actual holder, token and epoch observations.
- `fencing-result.json`, `fencing-migrated-result.json`: valid/stale/hop checks.
- `fencing-revoked-result.json`: the old-owner 14-versus-9 failure before repair.
- `old-owner-v4.log`, `new-owner-v4.log`: deployed server observations.
- `/tmp/ws5-center-release-fence-red.log`: failing regression assertion.
- `/tmp/ws5-center-release-fence-runtime.log`: all 153 runtime tests passed.
- `/tmp/ws5-center-release-fence-matrix.log`: current backend matrix output.
- `/tmp/ws5-center-forward-image-v5.log`: successful image build output.
- `v5/*-result.json`: final passing proxy/fencing evidence, including revocation.
- `v5/permissions-restored.json`: revoked temporary grants on both replicas.
- `v5/repo-browser.log`, `v5/repo-browser/`: three passing browser cases.
- `v5/lease-*.json`, `v5/metrics-*.txt`, `v5/rollout.log`: deployed observations.
- `v5/topology.json`: current owned port-forward PIDs, Pod names and mappings.
  Revalidate before reuse. Controller session 7083 remains on loopback Admin
  15911/conf_sync 50963, with its federation connection forwarded on 50962.
  Browser 14181 and per-Pod 14281/14282 still run through OAuth sidecars.
- `controller-binary.sha256`: exact native Controller artifact identity.

Credentials, client keys, browser cookies and rendered Secrets remain private
local artifacts. No credentials or runtime manifests were committed.

This proves explicit connection migration, not a Pod crash takeover or an
ambiguous transport fault injected after mutation execution. Those scenarios
retain narrower adapter/runtime evidence until exercised independently.
