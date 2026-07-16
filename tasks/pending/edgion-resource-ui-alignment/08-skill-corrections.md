# Skill Correction Ledger

| Repository/source | Verified implementation | Required correction | Status |
|---|---|---|---|
| Edgion auth architecture skill | missing Role returns 401; authenticated deny returns 403 | document the distinct status semantics | done |
| Edgion Controller config/auth skill | default policy reads all non-Secret kinds, writes EdgionConfigData, permits RegionRoute `list`/`failover`, and permits NonResource `server-info` | remove RegionRoute `get`; add `server-info`; keep explicit-policy replacement semantics | done |
| Edgion ReferenceGrant skill | canonical v1 and detectable v1beta1; writer now projects a single active version and CRUD tests pass | document the verified alternate-version behavior | done |
| Edgion TCPRoute feature skill | runtime supports StreamPlugins and TCP keepalive, not Proxy Protocol/connect retries | remove TLS-only annotations and add the three keepalive keys | done |
| Edgion TLSRoute feature skill | runtime recognizes only `proxy-protocol: v2` and also supports TCP keepalive | replace old `1/2` values and add the three keepalive keys | done |
| Edgion UDPRoute feature skill | runtime resolves Stage-1 StreamPlugins | add the supported annotation and UDP stage boundary | done |
| Edgion EdgionGatewayConfig feature skill | current Rust/CRD removed `pathNormalization`, added `tcpTimeout`, `outboundTls`, `dnsResolver`, and `securityProtect.rejectDuplicateHost` | replace the stale full schema and field reference with the current contract | done |
| Edgion GatewayConfig CRD | Rust/serde and feature skill default `enableReferenceGrantValidation` to false while the committed CRD defaulted it to true | align the CRD default to false and pin a cross-mode regression test | done |
| Edgion HTTP plugin feature skill | Rust has 39 variants and four explicit stage arrays; skill said 35, used stale `spec.plugins`, and omitted four variants | correct cardinality, missing variants, field names, flattened entry shape, and stage table | done |
| Edgion Stream plugin feature skill | Rust uses flattened entries, 3 Stage-1 variants, and only IpRestriction in Stage 2 | remove nested `plugin` shape and correct the stage/variant matrix | done |
| Edgion LinkSys skill | serde discriminator and examples already use lowercase `httpdns` | keep UI wire value lowercase while display label may be HTTPDNS | done |
| Center web skill index | testing subtree is `05-testing` | corrected | done |
| Center types/utils skill | destructive normalization can erase new fields | replaced with lossless view/edit/mutation adapter rule | done |
| Center testing skill | stale absolute paths/ports and incomplete safety rules | replaced with repository-relative commands, two modes, preservation/browser/safety gates | done |
Every row was changed in the isolated companion worktree, independently reviewed
with its implementation increment, and marked done only after source and tests
agreed. No repository-local skill claims a capability that has not shipped.
