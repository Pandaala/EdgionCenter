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
