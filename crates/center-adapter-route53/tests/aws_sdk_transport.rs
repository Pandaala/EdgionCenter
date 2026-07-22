use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::{Duration, UNIX_EPOCH};

use aws_config::BehaviorVersion;
use aws_sdk_route53::config::{Credentials, Region};
use edgion_center_adapter_route53::{
    AwsAssumeRoleSpec, AwsRoute53Api, AwsRoute53ApiOptions, AwsRoute53SdkConfigFactory, Route53Api,
    Route53ChangeAction, Route53ChangeBatch, Route53CreateHostedZoneRequest, Route53RecordChange,
    Route53RecordCursor, Route53RecordSet,
};
use edgion_center_core::ProviderErrorCategory;
use wiremock::{
    matchers::{body_string_contains, header_regex, method, path, query_param},
    Mock, MockServer, Request, Respond, ResponseTemplate,
};

const ACCOUNT_ID: &str = "123456789012";

async fn sdk_config() -> aws_config::SdkConfig {
    aws_config::defaults(BehaviorVersion::latest())
        .credentials_provider(Credentials::new(
            "test-access-key",
            "test-secret-key",
            Some("test-session-token".to_string()),
            None,
            "route53-hermetic-test",
        ))
        .region(Region::new("us-east-1"))
        .load()
        .await
}

async fn mount_identity(server: &MockServer, account_id: &str) {
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<GetCallerIdentityResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <GetCallerIdentityResult>
    <Arn>arn:aws:iam::{account_id}:role/hermetic-test</Arn>
    <UserId>TESTUSER:hermetic</UserId>
    <Account>{account_id}</Account>
  </GetCallerIdentityResult>
  <ResponseMetadata><RequestId>sts-request-id</RequestId></ResponseMetadata>
</GetCallerIdentityResponse>"#
        )))
        .mount(server)
        .await;
}

async fn api(server: &MockServer) -> AwsRoute53Api {
    mount_identity(server, ACCOUNT_ID).await;
    AwsRoute53Api::with_options(
        &sdk_config().await,
        AwsRoute53ApiOptions {
            route53_endpoint_url: Some(server.uri()),
            sts_endpoint_url: Some(server.uri()),
        },
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn sts_identity_requires_an_exact_twelve_digit_account() {
    let valid = MockServer::start().await;
    let transport = api(&valid).await;
    assert_eq!(transport.verified_account_id(), ACCOUNT_ID);

    for invalid in ["", "12345678901", "1234567890123", "12345678901X"] {
        let server = MockServer::start().await;
        mount_identity(&server, invalid).await;
        let error = AwsRoute53Api::with_options(
            &sdk_config().await,
            AwsRoute53ApiOptions {
                route53_endpoint_url: Some(server.uri()),
                sts_endpoint_url: Some(server.uri()),
            },
        )
        .await
        .err()
        .expect("invalid STS account must fail construction");
        assert_eq!(error.category(), ProviderErrorCategory::Validation);
        assert_eq!(error.code(), "invalid_sts_account_id");
    }
}

#[tokio::test]
async fn inherited_global_endpoint_override_is_rejected_before_credentials_or_network() {
    let config = sdk_config()
        .await
        .to_builder()
        .endpoint_url("https://credential-sink.example.test")
        .build();
    let error = AwsRoute53Api::new(&config)
        .await
        .err()
        .expect("global endpoint override must fail before client construction");
    assert_eq!(error.category(), ProviderErrorCategory::Validation);
    assert_eq!(error.code(), "configured_aws_endpoint_override_forbidden");
}

#[tokio::test]
async fn refreshable_assume_role_credentials_feed_the_verified_transport() {
    let server = MockServer::start().await;
    let assume_calls = Arc::new(AtomicUsize::new(0));
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_string_contains("Action=AssumeRole"))
        .and(body_string_contains("ExternalId=tenant%2Froute53%3Acanary"))
        .and(body_string_contains("RoleSessionName=edgion-route53"))
        .respond_with(AssumeRoleSequence {
            calls: assume_calls.clone(),
        })
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_string_contains("Action=GetCallerIdentity"))
        .and(header_regex(
            "authorization",
            "Credential=assumed-access-key-1/",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<GetCallerIdentityResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <GetCallerIdentityResult>
    <Arn>arn:aws:sts::{ACCOUNT_ID}:assumed-role/dns-manager/edgion-route53</Arn>
    <UserId>AROAEXAMPLE:edgion-route53</UserId>
    <Account>{ACCOUNT_ID}</Account>
  </GetCallerIdentityResult>
  <ResponseMetadata><RequestId>identity-request-id</RequestId></ResponseMetadata>
</GetCallerIdentityResponse>"#
        )))
        .expect(1)
        .mount(&server)
        .await;

    let time = aws_smithy_async::test_util::ManualTimeSource::new(
        UNIX_EPOCH + Duration::from_secs(2_000_000_000),
    );
    let base = aws_config::defaults(BehaviorVersion::latest())
        .credentials_provider(Credentials::new(
            "base-access-key",
            "base-secret-key",
            None,
            None,
            "route53-assume-role-test",
        ))
        .region(Region::new("us-east-1"))
        .time_source(time.clone())
        .load()
        .await;
    let spec = AwsAssumeRoleSpec::new(
        "arn:aws:iam::123456789012:role/dns-manager",
        "edgion-route53",
    )
    .unwrap();
    let assumed = AwsRoute53SdkConfigFactory::assume_role_with_sts_endpoint_for_test(
        &base,
        &spec,
        Some("tenant/route53:canary".to_string()),
        &server.uri(),
    )
    .await
    .unwrap();
    let debug = format!("{assumed:?}");
    assert!(!debug.contains("base-secret-key"));
    assert!(!debug.contains("tenant/route53:canary"));

    // The AssumeRole provider was intentionally bootstrapped against the mock endpoint, while
    // production Route 53 construction rejects every inherited endpoint override. Recompose only
    // the refreshable credential provider and non-endpoint SDK settings, then inject loopback
    // endpoints through the explicit hermetic transport seam below.
    let transport_config = aws_config::SdkConfig::builder()
        .behavior_version(BehaviorVersion::latest())
        .credentials_provider(assumed.credentials_provider().unwrap().clone())
        .region(Region::new("us-east-1"))
        .time_source(time.clone())
        .build();

    let transport = AwsRoute53Api::with_options(
        &transport_config,
        AwsRoute53ApiOptions {
            route53_endpoint_url: Some(server.uri()),
            sts_endpoint_url: Some(server.uri()),
        },
    )
    .await
    .unwrap();
    assert_eq!(transport.verified_account_id(), ACCOUNT_ID);
    time.advance(Duration::from_secs(3_700));
    const LIST_RESPONSE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListHostedZonesResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <HostedZones />
  <IsTruncated>false</IsTruncated>
  <MaxItems>1</MaxItems>
</ListHostedZonesResponse>"#;
    Mock::given(method("GET"))
        .and(path("/2013-04-01/hostedzone"))
        .and(header_regex(
            "authorization",
            "Credential=assumed-access-key-2/",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_string(LIST_RESPONSE))
        .expect(1)
        .mount(&server)
        .await;
    transport.list_hosted_zones(None, 1).await.unwrap();
    assert_eq!(assume_calls.load(Ordering::SeqCst), 2);
    let debug = format!("{assumed:?}");
    for secret in [
        "base-secret-key",
        "tenant/route53:canary",
        "assumed-secret-key-1",
        "assumed-session-token-1",
        "assumed-secret-key-2",
        "assumed-session-token-2",
    ] {
        assert!(!debug.contains(secret));
    }
}

#[derive(Clone)]
struct AssumeRoleSequence {
    calls: Arc<AtomicUsize>,
}

impl Respond for AssumeRoleSequence {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        let attempt = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        let expiration = match attempt {
            1 => "2033-05-18T04:33:20Z",
            _ => "2033-05-18T06:33:20Z",
        };
        ResponseTemplate::new(200).set_body_string(format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<AssumeRoleResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <AssumeRoleResult>
    <Credentials>
      <AccessKeyId>assumed-access-key-{attempt}</AccessKeyId>
      <SecretAccessKey>assumed-secret-key-{attempt}</SecretAccessKey>
      <SessionToken>assumed-session-token-{attempt}</SessionToken>
      <Expiration>{expiration}</Expiration>
    </Credentials>
    <AssumedRoleUser>
      <Arn>arn:aws:sts::123456789012:assumed-role/dns-manager/edgion-route53</Arn>
      <AssumedRoleId>AROAEXAMPLE:edgion-route53</AssumedRoleId>
    </AssumedRoleUser>
  </AssumeRoleResult>
  <ResponseMetadata><RequestId>assume-request-{attempt}</RequestId></ResponseMetadata>
</AssumeRoleResponse>"#
        ))
    }
}

#[derive(Clone)]
struct RetryThenSuccess {
    calls: Arc<AtomicUsize>,
    failures: usize,
    success_body: &'static str,
}

impl Respond for RetryThenSuccess {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        let attempt = self.calls.fetch_add(1, Ordering::SeqCst);
        if attempt < self.failures {
            ResponseTemplate::new(503).set_body_string("unavailable")
        } else {
            ResponseTemplate::new(200).set_body_string(self.success_body)
        }
    }
}

#[tokio::test]
async fn route53_reads_retry_transient_responses_with_the_bounded_policy() {
    const LIST_RESPONSE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListHostedZonesResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <HostedZones />
  <IsTruncated>false</IsTruncated>
  <MaxItems>100</MaxItems>
</ListHostedZonesResponse>"#;

    let server = MockServer::start().await;
    let transport = api(&server).await;
    let calls = Arc::new(AtomicUsize::new(0));
    Mock::given(method("GET"))
        .and(path("/2013-04-01/hostedzone"))
        .respond_with(RetryThenSuccess {
            calls: calls.clone(),
            failures: 2,
            success_body: LIST_RESPONSE,
        })
        .mount(&server)
        .await;

    let page = transport.list_hosted_zones(None, 100).await.unwrap();
    assert!(page.items.is_empty());
    assert!(!page.is_truncated);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn get_hosted_zone_maps_public_zone_and_no_such_zone() {
    const GET_ZONE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<GetHostedZoneResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <HostedZone>
    <Id>/hostedzone/ZPUBLIC</Id>
    <Name>example.test.</Name>
    <CallerReference>hermetic</CallerReference>
    <Config><Comment>public test zone</Comment><PrivateZone>false</PrivateZone></Config>
    <ResourceRecordSetCount>2</ResourceRecordSetCount>
  </HostedZone>
  <DelegationSet><NameServers><NameServer>ns.example.test.</NameServer></NameServers></DelegationSet>
</GetHostedZoneResponse>"#;
    const NO_SUCH_ZONE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ErrorResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <Error><Type>Sender</Type><Code>NoSuchHostedZone</Code><Message>absent</Message></Error>
  <RequestId>route53-request-id</RequestId>
</ErrorResponse>"#;

    let server = MockServer::start().await;
    let transport = api(&server).await;
    Mock::given(method("GET"))
        .and(path("/2013-04-01/hostedzone/ZPUBLIC"))
        .respond_with(ResponseTemplate::new(200).set_body_string(GET_ZONE))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/2013-04-01/hostedzone/ZMISSING"))
        .respond_with(ResponseTemplate::new(404).set_body_string(NO_SUCH_ZONE))
        .expect(1)
        .mount(&server)
        .await;

    let zone = transport
        .get_hosted_zone("ZPUBLIC")
        .await
        .unwrap()
        .expect("zone must be present");
    assert_eq!(zone.id, "/hostedzone/ZPUBLIC");
    assert_eq!(zone.name, "example.test.");
    assert!(!zone.private_zone);
    assert!(!zone.has_linked_service);
    assert!(!zone.has_unsupported_features);
    assert!(transport
        .get_hosted_zone("ZMISSING")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn lifecycle_transport_uses_typed_create_and_dnssec_paths() {
    const CREATE_RESPONSE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<CreateHostedZoneResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <HostedZone>
    <Id>/hostedzone/ZCREATED</Id><Name>created.test.</Name>
    <CallerReference>edgion-stable-reference</CallerReference>
    <Config><PrivateZone>false</PrivateZone></Config><ResourceRecordSetCount>2</ResourceRecordSetCount>
  </HostedZone>
  <ChangeInfo><Id>/change/CCREATE</Id><Status>PENDING</Status><SubmittedAt>2026-01-02T03:04:05Z</SubmittedAt></ChangeInfo>
  <DelegationSet><NameServers><NameServer>ns-1.awsdns.test.</NameServer></NameServers></DelegationSet>
</CreateHostedZoneResponse>"#;
    const DNSSEC_RESPONSE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<GetDNSSECResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <Status><ServeSignature>SIGNING</ServeSignature></Status>
  <KeySigningKeys><member>
    <Name>edgion-ksk</Name><KmsArn>arn:aws:kms:us-east-1:123456789012:key/test</KmsArn>
    <Flag>257</Flag><SigningAlgorithmMnemonic>13</SigningAlgorithmMnemonic>
    <DigestAlgorithmMnemonic>2</DigestAlgorithmMnemonic><KeyTag>2371</KeyTag>
    <DigestValue>AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA</DigestValue><PublicKey>PUBLIC</PublicKey>
    <DSRecord>2371 13 2 AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA</DSRecord><DNSKEYRecord>257 3 13 PUBLIC</DNSKEYRecord>
    <Status>ACTIVE</Status>
  </member></KeySigningKeys>
</GetDNSSECResponse>"#;

    let server = MockServer::start().await;
    let transport = api(&server).await;
    Mock::given(method("POST"))
        .and(path("/2013-04-01/hostedzone"))
        .respond_with(ResponseTemplate::new(201).set_body_string(CREATE_RESPONSE))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/2013-04-01/hostedzone/ZCREATED/dnssec"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DNSSEC_RESPONSE))
        .expect(1)
        .mount(&server)
        .await;

    let created = transport
        .create_hosted_zone(&Route53CreateHostedZoneRequest {
            name: "created.test".to_string(),
            caller_reference: "edgion-stable-reference".to_string(),
        })
        .await
        .unwrap();
    assert_eq!(created.hosted_zone.id, "/hostedzone/ZCREATED");
    assert_eq!(created.hosted_zone.name_servers, ["ns-1.awsdns.test."]);
    let requests = server.received_requests().await.unwrap();
    let create_body = String::from_utf8_lossy(&requests[1].body);
    assert!(create_body.contains("<Name>created.test</Name>"));
    assert!(create_body.contains("<CallerReference>edgion-stable-reference</CallerReference>"));
    let dnssec = transport.get_dnssec("ZCREATED").await.unwrap();
    assert_eq!(dnssec.serve_signature, "SIGNING");
    assert_eq!(dnssec.key_signing_keys[0].status, "ACTIVE");
    assert_eq!(
        dnssec.key_signing_keys[0].ds_record.as_deref(),
        Some("2371 13 2 AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA")
    );
}

#[tokio::test]
async fn dnssec_transport_never_defaults_missing_status_to_disabled() {
    const INCOMPLETE_DNSSEC: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<GetDNSSECResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <KeySigningKeys />
</GetDNSSECResponse>"#;
    let server = MockServer::start().await;
    let transport = api(&server).await;
    Mock::given(method("GET"))
        .and(path("/2013-04-01/hostedzone/ZINCOMPLETE/dnssec"))
        .respond_with(ResponseTemplate::new(200).set_body_string(INCOMPLETE_DNSSEC))
        .expect(1)
        .mount(&server)
        .await;

    let error = transport.get_dnssec("ZINCOMPLETE").await.unwrap_err();
    assert_eq!(error.category(), ProviderErrorCategory::Validation);
    assert_eq!(error.code(), "missing_route53_dnssec_serve_signature");
}

#[tokio::test]
async fn list_record_sets_sends_compound_cursor_and_maps_alias_and_routing_fields() {
    const RECORDS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListResourceRecordSetsResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <ResourceRecordSets>
    <ResourceRecordSet>
      <Name>alias.example.test.</Name><Type>A</Type><SetIdentifier>primary</SetIdentifier>
      <Failover>PRIMARY</Failover><HealthCheckId>health-check-a</HealthCheckId>
      <AliasTarget><HostedZoneId>ZTARGET</HostedZoneId><DNSName>target.example.net.</DNSName><EvaluateTargetHealth>true</EvaluateTargetHealth></AliasTarget>
    </ResourceRecordSet>
    <ResourceRecordSet>
      <Name>weighted.example.test.</Name><Type>A</Type><SetIdentifier>blue</SetIdentifier>
      <Weight>10</Weight><TTL>60</TTL>
      <ResourceRecords><ResourceRecord><Value>192.0.2.10</Value></ResourceRecord></ResourceRecords>
    </ResourceRecordSet>
  </ResourceRecordSets>
  <IsTruncated>true</IsTruncated>
  <NextRecordName>weighted.example.test.</NextRecordName>
  <NextRecordType>A</NextRecordType>
  <NextRecordIdentifier>green</NextRecordIdentifier>
  <MaxItems>2</MaxItems>
</ListResourceRecordSetsResponse>"#;

    let server = MockServer::start().await;
    let transport = api(&server).await;
    Mock::given(method("GET"))
        .and(path("/2013-04-01/hostedzone/ZPUBLIC/rrset"))
        .and(query_param("name", "start.example.test."))
        .and(query_param("type", "A"))
        .and(query_param("identifier", "start-blue"))
        .and(query_param("maxitems", "2"))
        .respond_with(ResponseTemplate::new(200).set_body_string(RECORDS))
        .expect(1)
        .mount(&server)
        .await;

    let page = transport
        .list_record_sets(
            "ZPUBLIC",
            Some(&Route53RecordCursor {
                name: "start.example.test.".to_string(),
                record_type: "A".to_string(),
                set_identifier: Some("start-blue".to_string()),
            }),
            2,
        )
        .await
        .unwrap();
    assert!(page.is_truncated);
    assert_eq!(
        page.next,
        Some(Route53RecordCursor {
            name: "weighted.example.test.".to_string(),
            record_type: "A".to_string(),
            set_identifier: Some("green".to_string()),
        })
    );
    assert_eq!(page.items.len(), 2);
    let alias = &page.items[0];
    assert_eq!(alias.set_identifier.as_deref(), Some("primary"));
    assert_eq!(alias.failover.as_deref(), Some("PRIMARY"));
    assert_eq!(alias.health_check_id.as_deref(), Some("health-check-a"));
    let target = alias.alias_target.as_ref().expect("alias target");
    assert_eq!(target.hosted_zone_id, "ZTARGET");
    assert_eq!(target.dns_name, "target.example.net.");
    assert!(target.evaluate_target_health);
    assert_eq!(alias.ttl, None);
    let weighted = &page.items[1];
    assert_eq!(weighted.set_identifier.as_deref(), Some("blue"));
    assert_eq!(weighted.weight, Some(10));
    assert_eq!(weighted.ttl, Some(60));
    assert_eq!(weighted.resource_records, ["192.0.2.10"]);
}

#[tokio::test]
async fn change_record_sets_maps_success_and_serializes_exact_create_shape() {
    const CHANGE_RESPONSE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ChangeResourceRecordSetsResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <ChangeInfo>
    <Id>/change/C123</Id><Status>PENDING</Status><SubmittedAt>2026-01-02T03:04:05Z</SubmittedAt>
    <Comment>hermetic mutation test</Comment>
  </ChangeInfo>
</ChangeResourceRecordSetsResponse>"#;

    let server = MockServer::start().await;
    let transport = api(&server).await;
    Mock::given(method("POST"))
        .and(path("/2013-04-01/hostedzone/ZPRIMARY/rrset"))
        .and(body_string_contains(
            "<Comment>hermetic mutation test</Comment>",
        ))
        .and(body_string_contains("<Action>CREATE</Action>"))
        .and(body_string_contains("<Name>txt.example.test.</Name>"))
        .and(body_string_contains("<Type>TXT</Type>"))
        .and(body_string_contains("<TTL>300</TTL>"))
        .and(body_string_contains("<Value>&quot;value&quot;</Value>"))
        .respond_with(ResponseTemplate::new(200).set_body_string(CHANGE_RESPONSE))
        .expect(1)
        .mount(&server)
        .await;

    let info = transport
        .change_record_sets("ZPRIMARY", &create_batch())
        .await
        .unwrap();
    assert_eq!(info.id, "/change/C123");
    assert_eq!(info.status, "PENDING");
    assert_eq!(info.comment.as_deref(), Some("hermetic mutation test"));
    assert!(info.submitted_at_unix_seconds > 0);
}

#[tokio::test]
async fn route53_mutations_never_retry_an_ambiguous_server_failure() {
    let server = MockServer::start().await;
    let transport = api(&server).await;
    let calls = Arc::new(AtomicUsize::new(0));
    Mock::given(method("POST"))
        .and(path("/2013-04-01/hostedzone/ZPRIMARY/rrset"))
        .respond_with(RetryThenSuccess {
            calls: calls.clone(),
            failures: 1,
            success_body: "this response must never be reached",
        })
        .mount(&server)
        .await;

    let error = transport
        .change_record_sets("ZPRIMARY", &create_batch())
        .await
        .unwrap_err();
    assert_eq!(error.category(), ProviderErrorCategory::UnknownOutcome);
    assert_eq!(error.retry_after_ms(), None);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn route53_mutation_preserves_an_explicit_invalid_input_rejection() {
    const INVALID_INPUT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ErrorResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <Error>
    <Type>Sender</Type>
    <Code>InvalidInput</Code>
    <Message>sensitive provider message</Message>
  </Error>
  <RequestId>route53-request-id</RequestId>
</ErrorResponse>"#;

    let server = MockServer::start().await;
    let transport = api(&server).await;
    Mock::given(method("POST"))
        .and(path("/2013-04-01/hostedzone/ZPRIMARY/rrset"))
        .respond_with(ResponseTemplate::new(400).set_body_string(INVALID_INPUT))
        .expect(1)
        .mount(&server)
        .await;

    let error = transport
        .change_record_sets("ZPRIMARY", &create_batch())
        .await
        .unwrap_err();
    assert_eq!(error.category(), ProviderErrorCategory::Validation);
    assert_eq!(error.code(), "route53_service_error");
    assert!(!error.message().contains("sensitive provider message"));
}

#[tokio::test]
async fn get_change_maps_pending_insync_and_no_such_change() {
    for (change_id, status) in [("CPENDING", "PENDING"), ("CINSYNC", "INSYNC")] {
        let server = MockServer::start().await;
        let transport = api(&server).await;
        let body = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<GetChangeResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <ChangeInfo>
    <Id>/change/{change_id}</Id><Status>{status}</Status><SubmittedAt>2026-01-02T03:04:05Z</SubmittedAt>
    <Comment>edgion:digest</Comment>
  </ChangeInfo>
</GetChangeResponse>"#
        );
        Mock::given(method("GET"))
            .and(path(format!("/2013-04-01/change/{change_id}")))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .expect(1)
            .mount(&server)
            .await;

        let info = transport
            .get_change(change_id)
            .await
            .unwrap()
            .expect("change must exist");
        assert_eq!(info.id, format!("/change/{change_id}"));
        assert_eq!(info.status, status);
        assert_eq!(info.comment.as_deref(), Some("edgion:digest"));
        assert!(info.submitted_at_unix_seconds > 0);
    }

    const NO_SUCH_CHANGE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ErrorResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <Error><Type>Sender</Type><Code>NoSuchChange</Code><Message>absent</Message></Error>
  <RequestId>route53-request-id</RequestId>
</ErrorResponse>"#;
    let server = MockServer::start().await;
    let transport = api(&server).await;
    Mock::given(method("GET"))
        .and(path("/2013-04-01/change/CMISSING"))
        .respond_with(ResponseTemplate::new(404).set_body_string(NO_SUCH_CHANGE))
        .expect(1)
        .mount(&server)
        .await;
    assert!(transport.get_change("CMISSING").await.unwrap().is_none());
}

#[tokio::test]
async fn mutation_408_and_parseable_5xx_are_always_unknown_outcomes() {
    for (status, code) in [(408, "InvalidInput"), (500, "InternalFailure")] {
        let server = MockServer::start().await;
        let transport = api(&server).await;
        let body = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<ErrorResponse xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <Error><Type>Receiver</Type><Code>{code}</Code><Message>sensitive</Message></Error>
  <RequestId>route53-request-id</RequestId>
</ErrorResponse>"#
        );
        Mock::given(method("POST"))
            .and(path("/2013-04-01/hostedzone/ZPRIMARY/rrset"))
            .respond_with(ResponseTemplate::new(status).set_body_string(body))
            .expect(1)
            .mount(&server)
            .await;

        let error = transport
            .change_record_sets("ZPRIMARY", &create_batch())
            .await
            .unwrap_err();
        assert_eq!(error.category(), ProviderErrorCategory::UnknownOutcome);
        assert_eq!(error.code(), "route53_mutation_outcome_unknown");
        assert_eq!(error.retry_after_ms(), None);
    }
}

fn create_batch() -> Route53ChangeBatch {
    Route53ChangeBatch {
        changes: vec![Route53RecordChange {
            action: Route53ChangeAction::Create,
            record_set: Route53RecordSet {
                name: "txt.example.test.".to_string(),
                record_type: "TXT".to_string(),
                ttl: Some(300),
                resource_records: vec![r#""value""#.to_string()],
                alias_target: None,
                set_identifier: None,
                weight: None,
                failover: None,
                region: None,
                geolocation: None,
                multivalue_answer: None,
                health_check_id: None,
                traffic_policy_instance_id: None,
                has_cidr_routing_config: false,
                has_geoproximity_location: false,
            },
        }],
        comment: "hermetic mutation test".to_string(),
    }
}
