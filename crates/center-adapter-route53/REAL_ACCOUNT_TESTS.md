# Route 53 real-account integration test

`tests/real_account.rs` exercises the production AWS SDK client and the complete Route 53 DNS
adapter against a pre-provisioned, disposable **public** hosted zone. It creates one uniquely named
TXT RRset, waits for and reads the exact result, replaces it using the observed revision, waits for
and reads the exact replacement, and then performs a deterministic stale-writer race check. The
race writer first replaces the provider RRset through the raw SDK seam. A stale exact
`DELETE`+`CREATE` batch must then be rejected as `InvalidChangeBatch`/`Conflict`, with the race
writer's marker unchanged. Cleanup deletes only the currently observed owned marker.

The test never creates or deletes a hosted zone, health check, alias, or routing-policy record. A
normal run makes at most five mutation submissions: adapter create, adapter exact replace, raw race
replace, one expected-to-be-rejected stale exact batch, and cleanup delete. At most four batches can
be accepted. The disposable hosted zone itself remains subject to normal AWS hosted-zone charges.

## Safety contract

The test is compiled but ignored by default. Even when an operator explicitly runs ignored tests,
it returns before loading AWS configuration unless `EDGION_TEST_ROUTE53=1` is present. When enabled,
all of these values are required and validated before the AWS SDK or credential chain is loaded:

- `EDGION_TEST_ROUTE53_CONFIRM=DELETE_ONLY_EDGION_TEST_RECORDS`
- `EDGION_TEST_ROUTE53_EXPECTED_ACCOUNT_ID`: the exact 12-digit AWS account ID returned by STS
- `EDGION_TEST_ROUTE53_ZONE_ID`: the dedicated public hosted-zone ID
- `EDGION_TEST_ROUTE53_ZONE_APEX`: the exact DNS apex for that ID

Use a dedicated empty test zone. The credentials need only `sts:GetCallerIdentity`, Route 53 hosted
zone/record/change reads, and `route53:ChangeResourceRecordSets` on that zone. The deterministic
race check uses no additional permissions. Do not grant hosted-zone creation or deletion to the test
identity.

Example invocation:

```bash
EDGION_TEST_ROUTE53=1 \
EDGION_TEST_ROUTE53_CONFIRM=DELETE_ONLY_EDGION_TEST_RECORDS \
EDGION_TEST_ROUTE53_EXPECTED_ACCOUNT_ID=123456789012 \
EDGION_TEST_ROUTE53_ZONE_ID=Z0123456789EXAMPLE \
EDGION_TEST_ROUTE53_ZONE_APEX=route53-test.example.com \
cargo test -p edgion-center-adapter-route53 --test real_account -- --ignored --nocapture
```

The owner label contains `edgion-it`, the process ID, and the current timestamp. Owner and TXT-marker
lengths are checked before mutation, then the owner and all safe test markers are printed before the
first write. Cleanup runs after ordinary scenario errors and recognizes the create, replacement,
race-writer, and stale-writer markers from this invocation. After an uncertain create it uses the
full change-poll window and requires consecutive absence observations before concluding there is
nothing to delete. It refuses deletion for any other content. Process termination cannot run
in-process cleanup; use the printed owner and marker to inspect and remove only that RRset. Never
delete the hosted zone as test cleanup.
