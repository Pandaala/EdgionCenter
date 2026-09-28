# Open upstream gap: ACME scheduler clearing is rejected by the CRD

## Reproduction and authority

The isolated Center alignment topology completed real Pebble HTTP-01 issuance,
published the TLS Secret, and served its certificate on Gateway HTTPS. The
Controller then logs `Certificate is published but the scheduler checkpoint could
not be cleared; retrying` every five seconds. Ready/expiry remain valid, but the
scheduler checkpoint is retained.

Current sibling source agrees with the running behavior:

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
Controller authorization to conceal this failure. No Edgion edit was made in
this pass. Prefer checking the existing SSA field-manager removal semantics
before broadening the resource schema. Preserve fencing, UID/resourceVersion,
other status writers and the independent challenge/lifecycle fields.

A repair needs a real API-server check that reservation persists, successful
clear removes the checkpoint without changing unrelated status, and repeated
clear remains safe. Then rerun real issuance, renewal through Center and Gateway
certificate replacement. Current evidence establishes first issuance and initial
Secret hot loading; it does not establish renewal completion.

## Local repair checkpoint (runtime verification pending)

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

The Linux Controller build is still running at this checkpoint: exec session
89178, Docker container `ws5-acme-controller-build`, log
`/tmp/ws5-acme-scheduler-linux-build.log`. Re-poll that exact build before taking
any action; do not start another build merely because observation is delayed.
The running ACME topology still has the original Controller binary.

After a successful terminal build, stop only `ws5-center-acme-controller`, copy
the built Controller to `/tmp/ws5-center-acme-issuance-20260928/bin/edgion-controller`,
and start that same container. Keep Gateway session 63587 running. The prepared
`renewal-proof.cjs` first requires recovery to Ready with scheduler absent and the
original serial unchanged. It then changes renewBefore through the actual Center
form, verifies a new certificate and the Gateway fingerprint, and restores the
original renewal window in finally. Do not mark this gap resolved before those
runtime checks pass.
