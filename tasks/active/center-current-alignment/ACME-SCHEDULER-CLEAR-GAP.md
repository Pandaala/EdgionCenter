# ACME scheduler clearing: locally repaired and runtime verified

## Reproduction and authority

The isolated Center alignment topology completed real Pebble HTTP-01 issuance,
published the TLS Secret, and served its certificate on Gateway HTTPS. The
Controller then logged `Certificate is published but the scheduler checkpoint could
not be cleared; retrying` every five seconds. Ready/expiry remained valid, but the
scheduler checkpoint was retained.

The original source and running binary agreed on the failure:

- `Edgion/edgion-controller/src/services/acme/service.rs:1712`,
  `patch_scheduler_status`, serializes `None` as `status.scheduler: null` in a
  forced SSA PATCH to `/status`, under manager `edgion-acme-scheduler`.
- `Edgion/config/crd/edgion-crd/edgion_acme_crd.yaml:194` declares scheduler as
  object, with required observedGeneration/attemptsUsed, without nullable.
- `Edgion/skills/04-review/acme/regression-guards/acme-scheduler-fencing-recovery.md`
  requires successful publication to clear the checkpoint. Retrying permanent
  validation failure is not successful clearing.

An exact PATCH against the run-owned resource with `dryRun=All`, its current
UID/resourceVersion, the same field manager and force=true returns HTTP 422:
`scheduler in body must be of type object: "null"`.
The installed schema and current repository schema both have this constraint;
this is not merely an older shared-cluster CRD. No CRD was altered to bypass it.

Evidence: `/tmp/ws5-center-acme-issuance-20260928/scheduler-dryrun.json` and
`check-scheduler.cjs`; repeated actual failures are in `controller/logs/controller.log`.
The dry-run request uses the isolated Controller ServiceAccount, not Center.

## Repair boundary and acceptance

Repair belongs to Edgion's status writer/schema contract; Center must not delete
scheduler state, misreport trigger queue admission as issuance, or bypass
Controller authorization to conceal this failure. The local repair uses existing
SSA field-manager removal semantics without broadening the resource schema. It
preserves fencing, UID/resourceVersion, other status writers and the independent
challenge/lifecycle fields.

A repair needs a real API-server check that reservation persists, successful
clear removes the checkpoint without changing unrelated status, and repeated
clear remains safe. Then rerun real issuance, renewal through Center and Gateway
certificate replacement. The verification below establishes recovery and renewal
with the locally repaired Controller.

## Local repair and verification

The Edgion working tree now omits the scheduler field when clearing, retaining
`status: {}`, the same forced SSA manager, and UID/resourceVersion preconditions.
The Ready-recovery test now requires an empty status object rather than using
JSON indexing plus is_null (which conflated missing and explicit-null values).
The owning regression guide documents structural-CRD deletion semantics.
These three Edgion files are deliberately uncommitted; unrelated work is preserved.

A real API-server dry run of the corrected body returns 200, removes scheduler,
and preserves every other existing status field. Artifact:
`/tmp/ws5-center-acme-issuance-20260928/scheduler-omit-dryrun.json`.
All 29 Controller ACME tests pass (`/tmp/ws5-acme-scheduler-tests.log`). Formatting,
SSA-force and unit-test-layout guards pass. This is targeted verification, not a
fresh full Edgion workspace matrix.

The Linux release build completed successfully in 12m55s. Only the owned
Controller container was stopped for binary replacement and restarted. Its new
SHA-256 is `58dc6445b428044bb81b003ebc1fa8b06f80f7880f2ed37b8197b828f261d803`;
`fixed-binary.json` records both hashes. Gateway PID 57529 remained running.
The first recovery check observed Ready, no scheduler, and the original serial
`ce4a33ca15ac8c` unchanged: clearing the old checkpoint did not reissue it.

The final `renewal-proof.cjs` run passes all five checks in `renewal-result.json`:

1. The repaired Controller starts the scenario Ready with no scheduler checkpoint.
2. A CAS edit through the actual Center form makes renewal due; the real CA issues
   a replacement certificate, and successful publication clears the scheduler.
3. Gateway HTTPS hot-loads the certificate published in the Secret without restart.
4. Center displays the actual renewed expiry, and the original renewal window is restored.
5. A finally check independently confirms restoration of the original policy.

The final run changed serial `612747be7fdd792` to `5e7f3e2dd9b1c54a`.
`renewed.png` was visually inspected. A subsequent API-server read confirms Ready,
no scheduler, and renewBefore/checkInterval/failBackoff restored to 1h/1h/10s.
A repeated corrected dry run still returns 200 and preserves unrelated status.
Only the public certificate was read by the external harness; the TLS fingerprint
probe does not assert public-CA trust. Center did not gain Secret permissions.

Two earlier renewal attempts are retained as failed test runs, not counted as
passing evidence. The first waited for an earlier expiry while the deliberately
oversized renewal window caused another issuance; the harness now restores the
window immediately and waits for settled Ready before comparing certificates.
The second compared hexadecimal serial strings with inconsistent leading zeros;
the harness now compares their numeric values. Both restored the renewal policy.
The complete third scenario passed after those oracle corrections.

No CRD changed. The three Edgion repair files remain uncommitted; this Center
commit records verification only. Broader Edgion and Center matrices were not
rerun for this documentation checkpoint. The overall alignment audit remains active.
