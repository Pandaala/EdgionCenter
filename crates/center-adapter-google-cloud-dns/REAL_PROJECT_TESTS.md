# Cloud DNS real-project integration test

`tests/real_project.rs` is an ignored, environment-gated test against a pre-provisioned disposable
authoritative managed zone. It never creates or deletes a zone. It creates one unique TXT RRset,
polls the Cloud DNS Change, reads the exact result, and deletes only that owned RRset.

Required safety variables:

```text
EDGION_TEST_GOOGLE_DNS=1
EDGION_TEST_GOOGLE_DNS_CONFIRM=DELETE_ONLY_EDGION_TEST_RECORDS
EDGION_TEST_GOOGLE_DNS_PROJECT_ID=your-project-id
EDGION_TEST_GOOGLE_DNS_ZONE_ID=numeric-managed-zone-id
EDGION_TEST_GOOGLE_DNS_ZONE_APEX=dns-test.example.com
```

ADC is loaded only after all safety values pass validation. Prefer an attached service account or
Workload Identity Federation. Run with:

```bash
cargo test -p edgion-center-adapter-google-cloud-dns --test real_project -- --ignored --nocapture
```

Use the permissions in `IAM.md`. Process termination cannot run in-process cleanup; the test prints
the unique owner before mutation so an operator can remove only that RRset if necessary.
