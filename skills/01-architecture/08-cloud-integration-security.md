# Cloud integration security model

## Scope and deployment boundary

Cloud integration is a provider-specific DNS and WAF capability. It is not a shared cloud control
plane and has no Edgion Controller, Gateway, Region, federation, origin, certificate, cache,
load-balancing, webhook, or durable-operation coupling. Provider services are absent until their
individual composition switches are enabled; base standalone and Kubernetes deployments make no
cloud-provider request.

`ProviderAccount` stores a provider, fixed provider-native scope, and credential reference or
identity selector; it never stores resolved credential material. A caller cannot supply a provider
endpoint, account scope, external resource ID, credential reference, or arbitrary provider JSON in
place of a typed route-specific value. Every direct call reloads and validates exact account
authority; changed generation, provider, scope, credential binding, cursor, or target fails closed.

## Credential and identity threats

| Threat | Required control |
|---|---|
| Secret disclosure through persistence, API, logs, diagnostics, or audit | Resolve credentials only in provider adapters. Keep `CredentialRef`, opaque revision, provider, and scope separate from material; redact token headers, raw provider bodies, expressions, private keys, and fencing values. |
| Cross-account or confused-deputy use | Bind every request to one `ProviderAccount`, immutable provider-native scope, and expected credential purpose. Cloudflare rechecks its exact account on zone access. Provider authority never grants a human caller Center authorization. |
| Kubernetes Secret escalation | Prefer workload identity. The default-off mounted-file resolver receives one directory capability, accepts closed purpose bindings, and needs no Kubernetes `get`, `list`, or `watch` on Secrets. Files are bounded, regular, non-empty, and fail closed on unsafe permissions or paths. |
| Long-lived or stale authority | Prefer refreshed AWS ambient or federated identities and scoped Cloudflare API Tokens. Opaque revisions fence cached clients and observations; rotation invalidates earlier authority. |

Standalone aliases are resolved outside SQL business tables. Kubernetes uses projected identity or a
namespace-scoped mounted Secret/external-secret volume; the Center business API does not read or
return Secret data. See the checked-in [Cloudflare mounted credential example](../../cicd/deploy/examples/cloudflare-mounted-credentials/README.md), [AWS ambient identity example](../../cicd/deploy/examples/aws-route53-ambient/README.md), and [Route 53 zone IAM policy](../../tasks/cloud-integration/examples/route53-zone-iam-policy.md).

## Network egress and SSRF boundary

| Egress seam | Production target | Test seam and safeguards |
|---|---|---|
| Cloudflare credential inspection, DNS, and zone WAF | Fixed HTTPS `api.cloudflare.com/client/v4`; no production endpoint override | A custom endpoint is only for tests or private compatible gateways; plain HTTP is loopback-only. Redirects are disabled, production requires TLS, sizes/timeouts are bounded, and Authorization never follows a redirect. |
| AWS Route 53 and STS | AWS SDK-selected Route 53 and STS endpoints; inherited endpoint overrides are rejected | The only override seam is an explicitly named loopback HTTPS-or-HTTP test endpoint, validated before credential resolution or network I/O. |
| CloudFront distribution lifecycle | AWS SDK CloudFront endpoint selected by the SDK after ambient-credential identity verification; inherited endpoint overrides are rejected. | Explicit loopback-only SDK test seams remain separate from production composition. |
| AWS WAFv2 | Scope-specific AWS WAF endpoint selected by the SDK after STS account verification; CloudFront scope always uses `us-east-1`. | Explicit loopback-only SDK test seams; no user-configurable production endpoint. |
| DNS propagation verification | Resolver profiles name concrete `SocketAddr` targets. Public mode permits public-unicast addresses only on port 53; private/split-horizon mode requires explicit CIDR and port allowlists. | Each UDP/TCP exchange rechecks the resolved socket immediately before connect, preventing DNS rebinding. Query count, message size, timeout, retry/backoff, nameserver-address count, and profile endpoint count are bounded. |

Provider webhooks are not implemented or accepted. No external callback, signature verification,
listener exposure, or callback-derived authority exists in this boundary.

## Authorization, ownership, and mutation safety

The Admin API authenticates before provider work. Standalone evaluates database RBAC; Kubernetes
evaluates SubjectAccessReview for the authenticated subject and exact resource scope. Provider
routes mount only when their independently default-off service is composed. Read,
credential-reference use, credential inspection, DNS mutation, remote-control marking, WAF write,
WAF ordering/exception, WAF attachment/detachment, security weakening, and deletion remain
separate permissions. Existing Cloudflare DNS reads are high-trust across all configured
Cloudflare accounts and are not account-scoped access.

Resources distinguish `Managed` from `ObserveOnly`; observation never authorizes mutation.
`Retain` is the default deletion policy. A mutation must establish provider-specific authority over
the exact target, reject wrong-kind/dangling references, and use a fresh version/revision/ETag or
provider lock token where available. Destructive actions require exact confirmation when their
route defines one. Audit records capture actor, method/path, correlation, sanitized provider
account/resource identifiers, and a bounded action summary; they never capture credentials,
request bodies, provider payloads, WAF expressions, or internal authority tokens.

Direct mutations are synchronous and single-attempt. Center performs pre-dispatch scope and
revision checks, sends at most one provider mutation, and never automatically retries after a
possible dispatch. If timeout, disconnect, malformed acknowledgement, or post-dispatch authority
change leaves acceptance ambiguous, the response is `UnknownOutcome`; the caller must re-read the
provider target before a safe next action. No generic operation queue or worker converts ambiguity
into background work.

## DNS and WAF-specific threats

DNS writes preserve provider-specific semantics rather than using a generic endpoint. They reject
stale revisions, mismatched account/zone scope, forbidden apex SOA/delegation NS edits, and unsafe
unsupported settings. Cloudflare writes fresh-read the zone and complete canonical RRset before one
batch; a remote-control marker is an opaque display hint, never ownership or authorization proof.
Signed pagination and mutation receipts bind exact scope and key purpose; a key or inventory change
fails closed and asks the client to restart.

Cloudflare WAF is separately composed and independently default-off for reads and writes. It keeps
managed, custom, and rate-limit rules in typed phase-specific operations; it does not reuse retired
Origin Rules or accept arbitrary Rulesets phase/action JSON. Center ownership uses a dedicated,
mounted HMAC key ring whose signed reference binds the exact ProviderAccount, Zone, phase, and user
reference. The API token and active/fallback ownership keys must be distinct authorities. Duplicate,
forged, unparseable, or type-mismatched references stay opaque and cannot be mutated. Before a
mutation, Center fresh-reads the Zone and ruleset version, preserves unowned rules and their relative
order, and rejects stale ordering or ownership. Detaching protection, disabling a rule, weakening
default action, promoting preview/count behavior, or creating an exception requires dedicated
security-weaken intent, permission, confirmation, and sanitized audit. Provider expressions remain
provider-specific and redacted outside verified owned definitions. Entitlement, rule capacity, and
quota are explicit capability evidence: unknown is not available, and stale targets fail before
dispatch.

Cloudflare administrators with direct WAF write authority are trusted co-administrators of this
provider resource plane. A signed `ref` proves that the reference originated from Center; it is not
an immutable provider resource identity and can be transplanted by an actor who can already delete
and recreate Cloudflare rules. Simultaneous copies fail closed as duplicates. Detecting a
delete-and-recreate transplant would require a durable provider-rule ownership registry in both
deployment modes, which is intentionally outside this independent synchronous API boundary.

This model is enforced incrementally by provider-specific tasks; it does not create a shared
credential service, webhook listener, DNS abstraction, dashboard menu, or Edgion coupling.

AWS WAF is also provider-specific and default-off. Its composition accepts only an AWS
`ProviderAccount` with ambient credentials, verifies the account through STS, and applies one
typed scope to each request. Web ACL, IP-set, and regional-association mutations use fresh WAF
lock tokens, single dispatch, operation deadlines, and post-operation authority checks. A timeout
before dispatch is unavailable; any timeout or authority drift after possible dispatch is an
unknown outcome requiring provider observation. AWS WAF rule ownership is not inferred from a
name prefix: Center rules use an HMAC-protected provider rule name bound to provider account, AWS
account, scope, immutable Web ACL name, and Center reference. Invalid, duplicate, forged, or
transplanted proofs remain external and cannot be mutated. The ownership HMAC is a distinct
mounted authority from AWS credentials. CloudFront attachment remains a separate minimal
CloudFront operation rather than a shared gateway policy or Edgion integration.
