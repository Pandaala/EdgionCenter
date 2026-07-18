use aws_config::BehaviorVersion;
use aws_sdk_cloudfront::config::{Credentials, Region};
use edgion_center_adapter_cloudfront::{
    AcmCertificateKeyAlgorithm, AcmCertificateStatus, AcmCertificateType, AwsCloudFrontApi,
    AwsCloudFrontApiOptions, CloudFrontApi, CloudFrontDomainConflictResourceType,
    CloudFrontFingerprintKey, CloudFrontInvalidationStatus, CloudFrontPolicyKind,
    CloudFrontPolicyScope,
};
use edgion_center_core::ProviderErrorCategory;
use wiremock::{
    matchers::{body_string_contains, header, method, path, query_param},
    Mock, MockServer, ResponseTemplate,
};

const ACCOUNT_ID: &str = "123456789012";
const DISTRIBUTION_ID: &str = "E123EXAMPLE";
const DISTRIBUTION_ARN: &str = "arn:aws:cloudfront::123456789012:distribution/E123EXAMPLE";
const INVALIDATION_ID: &str = "I123EXAMPLE";
const CERTIFICATE_ARN: &str =
    "arn:aws:acm:us-east-1:123456789012:certificate/12345678-1234-1234-1234-123456789012";

const DOMAIN_CONFLICTS_PAGE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListDomainConflictsResult xmlns="http://cloudfront.amazonaws.com/doc/2020-05-31/">
  <DomainConflicts>
    <DomainConflicts>
      <Domain>*.example.test</Domain>
      <ResourceType>distribution-tenant</ResourceType>
      <ResourceId>***************ohWzv1b9It8J1AB</ResourceId>
      <AccountId>******789012</AccountId>
    </DomainConflicts>
  </DomainConflicts>
  <NextMarker>next-conflict-page</NextMarker>
</ListDomainConflictsResult>"#;

async fn sdk_config() -> aws_config::SdkConfig {
    aws_config::defaults(BehaviorVersion::latest())
        .credentials_provider(Credentials::new(
            "test-access-key",
            "test-secret-key",
            Some("test-session-token".to_string()),
            None,
            "cloudfront-hermetic-test",
        ))
        .region(Region::new("us-east-1"))
        .load()
        .await
}

async fn mount_identity(server: &MockServer, account_id: &str, partition: &str) {
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_string_contains("Action=GetCallerIdentity"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<GetCallerIdentityResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <GetCallerIdentityResult>
    <Arn>arn:{partition}:iam::{account_id}:role/cloudfront-reader</Arn>
    <UserId>TESTUSER:hermetic</UserId>
    <Account>{account_id}</Account>
  </GetCallerIdentityResult>
  <ResponseMetadata><RequestId>sts-request-id</RequestId></ResponseMetadata>
</GetCallerIdentityResponse>"#
        )))
        .mount(server)
        .await;
}

async fn api(server: &MockServer) -> AwsCloudFrontApi {
    mount_identity(server, ACCOUNT_ID, "aws").await;
    AwsCloudFrontApi::with_options(
        &sdk_config().await,
        CloudFrontFingerprintKey::new([7; 32]).unwrap(),
        "credential-test-1",
        AwsCloudFrontApiOptions {
            acm_endpoint_url: Some(server.uri()),
            cloudfront_endpoint_url: Some(server.uri()),
            sts_endpoint_url: Some(server.uri()),
        },
    )
    .await
    .unwrap()
}

const EMPTY_LIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DistributionList xmlns="http://cloudfront.amazonaws.com/doc/2020-05-31/">
  <Marker></Marker><MaxItems>25</MaxItems><IsTruncated>false</IsTruncated>
  <Quantity>0</Quantity><Items></Items>
</DistributionList>"#;

fn distribution_config(secret: &str) -> String {
    format!(
        r#"<DistributionConfig>
  <CallerReference>caller-reference</CallerReference>
  <Aliases><Quantity>0</Quantity><Items></Items></Aliases>
  <DefaultRootObject>index.html</DefaultRootObject>
  <Origins><Quantity>1</Quantity><Items><Origin>
    <Id>origin-1</Id><DomainName>origin.example.test</DomainName><OriginPath></OriginPath>
    <CustomHeaders><Quantity>1</Quantity><Items><OriginCustomHeader>
      <HeaderName>X-Origin-Secret</HeaderName><HeaderValue>{secret}</HeaderValue>
    </OriginCustomHeader></Items></CustomHeaders>
    <CustomOriginConfig><HTTPPort>80</HTTPPort><HTTPSPort>443</HTTPSPort>
      <OriginProtocolPolicy>https-only</OriginProtocolPolicy>
      <OriginSslProtocols><Quantity>1</Quantity><Items><SslProtocol>TLSv1.2</SslProtocol></Items></OriginSslProtocols>
      <OriginReadTimeout>30</OriginReadTimeout><OriginKeepaliveTimeout>5</OriginKeepaliveTimeout>
    </CustomOriginConfig><ConnectionAttempts>3</ConnectionAttempts><ConnectionTimeout>10</ConnectionTimeout>
  </Origin></Items></Origins>
  <OriginGroups><Quantity>0</Quantity><Items></Items></OriginGroups>
  <DefaultCacheBehavior><TargetOriginId>origin-1</TargetOriginId>
    <TrustedSigners><Enabled>false</Enabled><Quantity>0</Quantity><Items></Items></TrustedSigners>
    <TrustedKeyGroups><Enabled>false</Enabled><Quantity>0</Quantity><Items></Items></TrustedKeyGroups>
    <ViewerProtocolPolicy>redirect-to-https</ViewerProtocolPolicy>
    <AllowedMethods><Quantity>2</Quantity><Items><Method>GET</Method><Method>HEAD</Method></Items>
      <CachedMethods><Quantity>2</Quantity><Items><Method>GET</Method><Method>HEAD</Method></Items></CachedMethods>
    </AllowedMethods><SmoothStreaming>false</SmoothStreaming><Compress>true</Compress>
    <LambdaFunctionAssociations><Quantity>0</Quantity><Items></Items></LambdaFunctionAssociations>
    <FunctionAssociations><Quantity>0</Quantity><Items></Items></FunctionAssociations>
    <CachePolicyId>managed-cache-policy</CachePolicyId>
  </DefaultCacheBehavior>
  <CacheBehaviors><Quantity>0</Quantity><Items></Items></CacheBehaviors>
  <CustomErrorResponses><Quantity>0</Quantity><Items></Items></CustomErrorResponses>
  <Comment>inventory fixture</Comment>
  <Logging><Enabled>false</Enabled><IncludeCookies>false</IncludeCookies><Bucket></Bucket><Prefix></Prefix></Logging>
  <PriceClass>PriceClass_All</PriceClass><Enabled>true</Enabled>
  <ViewerCertificate><CloudFrontDefaultCertificate>true</CloudFrontDefaultCertificate><MinimumProtocolVersion>TLSv1</MinimumProtocolVersion></ViewerCertificate>
  <Restrictions><GeoRestriction><RestrictionType>none</RestrictionType><Quantity>0</Quantity><Items></Items></GeoRestriction></Restrictions>
  <WebACLId></WebACLId><HttpVersion>http2</HttpVersion><IsIPV6Enabled>true</IsIPV6Enabled><Staging>false</Staging>
</DistributionConfig>"#
    )
}

fn get_distribution_body(secret: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Distribution xmlns="http://cloudfront.amazonaws.com/doc/2020-05-31/">
  <Id>{DISTRIBUTION_ID}</Id><ARN>{DISTRIBUTION_ARN}</ARN><Status>Deployed</Status>
  <LastModifiedTime>2026-07-18T05:00:00Z</LastModifiedTime>
  <InProgressInvalidationBatches>0</InProgressInvalidationBatches>
  <DomainName>d123example.cloudfront.net</DomainName>
  {}
</Distribution>"#,
        distribution_config(secret)
    )
}

fn policy_config(kind: CloudFrontPolicyKind, name: &str) -> String {
    match kind {
        CloudFrontPolicyKind::Cache => format!(
            r#"<CachePolicyConfig><Comment>fixture</Comment><Name>{name}</Name>
<DefaultTTL>60</DefaultTTL><MaxTTL>300</MaxTTL><MinTTL>0</MinTTL>
<ParametersInCacheKeyAndForwardedToOrigin>
<EnableAcceptEncodingGzip>true</EnableAcceptEncodingGzip><EnableAcceptEncodingBrotli>true</EnableAcceptEncodingBrotli>
<HeadersConfig><HeaderBehavior>none</HeaderBehavior></HeadersConfig>
<CookiesConfig><CookieBehavior>none</CookieBehavior></CookiesConfig>
<QueryStringsConfig><QueryStringBehavior>none</QueryStringBehavior></QueryStringsConfig>
</ParametersInCacheKeyAndForwardedToOrigin></CachePolicyConfig>"#
        ),
        CloudFrontPolicyKind::OriginRequest => format!(
            r#"<OriginRequestPolicyConfig><Comment>fixture</Comment><Name>{name}</Name>
<HeadersConfig><HeaderBehavior>none</HeaderBehavior></HeadersConfig>
<CookiesConfig><CookieBehavior>none</CookieBehavior></CookiesConfig>
<QueryStringsConfig><QueryStringBehavior>none</QueryStringBehavior></QueryStringsConfig>
</OriginRequestPolicyConfig>"#
        ),
        CloudFrontPolicyKind::ResponseHeaders => format!(
            r#"<ResponseHeadersPolicyConfig><Comment>fixture</Comment><Name>{name}</Name></ResponseHeadersPolicyConfig>"#
        ),
    }
}

fn policy_fixture(
    kind: CloudFrontPolicyKind,
    id: &str,
    name: &str,
) -> (&'static str, &'static str, String, String) {
    let (resource, summary, entity, list) = match kind {
        CloudFrontPolicyKind::Cache => (
            "cache-policy",
            "CachePolicySummary",
            "CachePolicy",
            "CachePolicyList",
        ),
        CloudFrontPolicyKind::OriginRequest => (
            "origin-request-policy",
            "OriginRequestPolicySummary",
            "OriginRequestPolicy",
            "OriginRequestPolicyList",
        ),
        CloudFrontPolicyKind::ResponseHeaders => (
            "response-headers-policy",
            "ResponseHeadersPolicySummary",
            "ResponseHeadersPolicy",
            "ResponseHeadersPolicyList",
        ),
    };
    let config = policy_config(kind, name);
    let policy = format!(
        r#"<{entity}><Id>{id}</Id><LastModifiedTime>2026-07-18T05:00:00Z</LastModifiedTime>{config}</{entity}>"#
    );
    let list_body = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<{list} xmlns="http://cloudfront.amazonaws.com/doc/2020-05-31/">
<NextMarker>next-page</NextMarker><MaxItems>10</MaxItems><Quantity>1</Quantity><Items>
<{summary}><Type>managed</Type>{policy}</{summary}>
</Items></{list}>"#
    );
    let get_body = format!(r#"<?xml version="1.0" encoding="UTF-8"?>{policy}"#);
    (resource, entity, list_body, get_body)
}

fn invalidation_list_body(status: &str, create_time: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<InvalidationList xmlns="http://cloudfront.amazonaws.com/doc/2020-05-31/">
  <Marker></Marker><NextMarker>{INVALIDATION_ID}</NextMarker><MaxItems>1</MaxItems>
  <IsTruncated>true</IsTruncated><Quantity>1</Quantity><Items><InvalidationSummary>
    <Id>{INVALIDATION_ID}</Id><CreateTime>{create_time}</CreateTime><Status>{status}</Status>
  </InvalidationSummary></Items>
</InvalidationList>"#
    )
}

fn invalidation_detail_body(
    id: &str,
    status: &str,
    create_time: &str,
    caller_reference: &str,
    paths: &[&str],
) -> String {
    let path_items = paths
        .iter()
        .map(|path| format!("<Path>{path}</Path>"))
        .collect::<String>();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Invalidation xmlns="http://cloudfront.amazonaws.com/doc/2020-05-31/">
  <Id>{id}</Id><Status>{status}</Status><CreateTime>{create_time}</CreateTime>
  <InvalidationBatch><Paths><Quantity>{}</Quantity><Items>{path_items}</Items></Paths>
    <CallerReference>{caller_reference}</CallerReference>
  </InvalidationBatch>
</Invalidation>"#,
        paths.len()
    )
}

#[tokio::test]
async fn empty_list_uses_only_the_read_operation() {
    let server = MockServer::start().await;
    let transport = api(&server).await;
    Mock::given(method("GET"))
        .and(path("/2020-05-31/distribution"))
        .respond_with(ResponseTemplate::new(200).set_body_string(EMPTY_LIST))
        .expect(1)
        .mount(&server)
        .await;

    let page = transport.list_distributions(None, 25).await.unwrap();
    assert!(page.items.is_empty());
    assert!(!page.is_truncated);
    let requests = server.received_requests().await.unwrap();
    assert!(requests
        .iter()
        .filter(|request| request.url.path() != "/")
        .all(|request| request.method.as_str() == "GET"));
}

#[tokio::test]
async fn invalidation_list_is_distribution_scoped_typed_and_get_only() {
    let server = MockServer::start().await;
    let transport = api(&server).await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/2020-05-31/distribution/{DISTRIBUTION_ID}/invalidation"
        )))
        .and(query_param("MaxItems", "1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(invalidation_list_body("InProgress", "2026-07-18T05:00:00Z")),
        )
        .expect(1)
        .mount(&server)
        .await;

    let page = transport
        .list_invalidations(DISTRIBUTION_ID, None, 1)
        .await
        .unwrap();
    assert_eq!(page.distribution_id, DISTRIBUTION_ID);
    assert!(page.is_truncated);
    assert_eq!(page.next_marker.as_deref(), Some(INVALIDATION_ID));
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, INVALIDATION_ID);
    assert_eq!(
        page.items[0].status,
        CloudFrontInvalidationStatus::InProgress
    );
    assert!(page.items[0].created_at_unix_seconds > 0);
    assert!(server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.url.path() != "/")
        .all(|request| request.method.as_str() == "GET"));
}

#[tokio::test]
async fn invalidation_get_maps_caller_reference_paths_and_completion() {
    let server = MockServer::start().await;
    let transport = api(&server).await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/2020-05-31/distribution/{DISTRIBUTION_ID}/invalidation/{INVALIDATION_ID}"
        )))
        .respond_with(
            ResponseTemplate::new(200).set_body_string(invalidation_detail_body(
                INVALIDATION_ID,
                "Completed",
                "2026-07-18T05:00:00Z",
                "operation-123",
                &["/assets/*", "/image.jpg?a=1", "/literal*middle", "#tag"],
            )),
        )
        .expect(1)
        .mount(&server)
        .await;

    let detail = transport
        .get_invalidation(DISTRIBUTION_ID, INVALIDATION_ID)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail.distribution_id, DISTRIBUTION_ID);
    assert_eq!(detail.id, INVALIDATION_ID);
    assert_eq!(detail.status, CloudFrontInvalidationStatus::Completed);
    assert_eq!(detail.caller_reference, "operation-123");
    assert_eq!(
        detail.paths,
        ["/assets/*", "/image.jpg?a=1", "/literal*middle", "#tag"]
    );
    assert!(server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|request| request.url.path() != "/")
        .all(|request| request.method.as_str() == "GET"));
}

#[tokio::test]
async fn invalidation_requests_reject_invalid_scope_before_network_io() {
    let server = MockServer::start().await;
    let transport = api(&server).await;
    let bad_distribution = transport
        .list_invalidations("bad/distribution", None, 1)
        .await
        .unwrap_err();
    assert_eq!(
        bad_distribution.code(),
        "invalid_cloudfront_distribution_id"
    );
    let bad_marker = transport
        .list_invalidations(DISTRIBUTION_ID, Some(" unsafe"), 1)
        .await
        .unwrap_err();
    assert_eq!(bad_marker.code(), "invalid_cloudfront_invalidation_marker");
    let bad_size = transport
        .list_invalidations(DISTRIBUTION_ID, None, 101)
        .await
        .unwrap_err();
    assert_eq!(bad_size.code(), "invalid_cloudfront_invalidation_page_size");
    let bad_id = transport
        .get_invalidation(DISTRIBUTION_ID, " bad-id")
        .await
        .unwrap_err();
    assert_eq!(bad_id.code(), "invalid_cloudfront_invalidation_id");
    assert!(server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .all(|request| request.url.path() == "/"));
}

#[tokio::test]
async fn invalidation_list_rejects_page_status_time_and_marker_drift() {
    for (body, expected_code) in [
        (
            invalidation_list_body("Unknown", "2026-07-18T05:00:00Z"),
            "unknown_cloudfront_invalidation_status",
        ),
        (
            invalidation_list_body("Completed", "1969-12-31T23:59:59Z"),
            "invalid_cloudfront_invalidation_create_time",
        ),
        (
            invalidation_list_body("Completed", "2026-07-18T05:00:00Z").replace(
                "<NextMarker>I123EXAMPLE</NextMarker>",
                "<NextMarker>IOTHER</NextMarker>",
            ),
            "invalid_cloudfront_invalidation_next_marker",
        ),
        (
            invalidation_list_body("Completed", "2026-07-18T05:00:00Z")
                .replace("<MaxItems>1</MaxItems>", "<MaxItems>2</MaxItems>"),
            "cloudfront_invalidation_page_scope_mismatch",
        ),
    ] {
        let server = MockServer::start().await;
        let transport = api(&server).await;
        Mock::given(method("GET"))
            .and(path(format!(
                "/2020-05-31/distribution/{DISTRIBUTION_ID}/invalidation"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;
        let error = transport
            .list_invalidations(DISTRIBUTION_ID, None, 1)
            .await
            .unwrap_err();
        assert_eq!(error.code(), expected_code);
    }
}

#[tokio::test]
async fn invalidation_get_rejects_identity_and_unsafe_opaque_fields() {
    for (body, expected_code) in [
        (
            invalidation_detail_body(
                "IOTHER",
                "Completed",
                "2026-07-18T05:00:00Z",
                "operation-123",
                &["/index.html"],
            ),
            "cloudfront_invalidation_id_mismatch",
        ),
        (
            invalidation_detail_body(
                INVALIDATION_ID,
                "Completed",
                "2026-07-18T05:00:00Z",
                " unsafe",
                &["/index.html"],
            ),
            "invalid_cloudfront_invalidation_caller_reference",
        ),
        (
            invalidation_detail_body(
                INVALIDATION_ID,
                "Completed",
                "2026-07-18T05:00:00Z",
                "operation-123",
                &[""],
            ),
            "invalid_cloudfront_invalidation_item",
        ),
    ] {
        let server = MockServer::start().await;
        let transport = api(&server).await;
        Mock::given(method("GET"))
            .and(path(format!(
                "/2020-05-31/distribution/{DISTRIBUTION_ID}/invalidation/{INVALIDATION_ID}"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;
        let error = transport
            .get_invalidation(DISTRIBUTION_ID, INVALIDATION_ID)
            .await
            .unwrap_err();
        assert_eq!(error.code(), expected_code);
    }
}

#[tokio::test]
async fn policy_inventory_is_scope_filtered_and_exact_revision_verified() {
    for (index, kind) in [
        CloudFrontPolicyKind::Cache,
        CloudFrontPolicyKind::OriginRequest,
        CloudFrontPolicyKind::ResponseHeaders,
    ]
    .into_iter()
    .enumerate()
    {
        let server = MockServer::start().await;
        let transport = api(&server).await;
        let id = format!("policy-{index}");
        let name = format!("ManagedPolicy{index}");
        let (resource, _, list_body, get_body) = policy_fixture(kind, &id, &name);
        Mock::given(method("GET"))
            .and(path(format!("/2020-05-31/{resource}")))
            .and(query_param("Type", "managed"))
            .and(query_param("MaxItems", "10"))
            .respond_with(ResponseTemplate::new(200).set_body_string(list_body))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/2020-05-31/{resource}/{id}")))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("etag", format!("ETAG-{index}"))
                    .set_body_string(get_body),
            )
            .expect(1)
            .mount(&server)
            .await;

        let page = transport
            .list_policies(kind, CloudFrontPolicyScope::AwsManaged, None, 10)
            .await
            .unwrap();
        assert_eq!(page.next_marker.as_deref(), Some("next-page"));
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].id, id);
        assert_eq!(page.items[0].name, name);
        assert_eq!(page.items[0].kind, kind);
        assert_eq!(page.items[0].scope, CloudFrontPolicyScope::AwsManaged);
        assert_eq!(page.items[0].etag, format!("ETAG-{index}"));
        assert!(server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path() != "/")
            .all(|request| request.method.as_str() == "GET"));
    }
}

#[tokio::test]
async fn policy_inventory_has_a_hard_page_bound() {
    let server = MockServer::start().await;
    let transport = api(&server).await;
    for size in [0, 101] {
        let error = transport
            .list_policies(
                CloudFrontPolicyKind::Cache,
                CloudFrontPolicyScope::AwsManaged,
                None,
                size,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), "invalid_cloudfront_policy_page_size");
    }
    assert!(server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .all(|request| request.url.path() == "/"));
}

#[tokio::test]
async fn policy_inventory_rejects_an_unsafe_request_marker_before_network_io() {
    let server = MockServer::start().await;
    let transport = api(&server).await;
    let error = transport
        .list_policies(
            CloudFrontPolicyKind::Cache,
            CloudFrontPolicyScope::AwsManaged,
            Some(" unsafe-marker"),
            10,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "invalid_cloudfront_policy_marker");
    assert!(server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .all(|request| request.url.path() == "/"));
}

#[tokio::test]
async fn policy_inventory_rejects_invalid_list_ids_before_exact_get() {
    let server = MockServer::start().await;
    let transport = api(&server).await;
    let (resource, _, list_body, _) =
        policy_fixture(CloudFrontPolicyKind::Cache, "bad/id", "InvalidId");
    Mock::given(method("GET"))
        .and(path(format!("/2020-05-31/{resource}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(list_body))
        .mount(&server)
        .await;

    let error = transport
        .list_policies(
            CloudFrontPolicyKind::Cache,
            CloudFrontPolicyScope::AwsManaged,
            None,
            10,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "invalid_cloudfront_policy_id");
    assert_eq!(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path() != "/")
            .count(),
        1
    );
}

#[tokio::test]
async fn policy_inventory_rejects_provider_max_items_drift() {
    let server = MockServer::start().await;
    let transport = api(&server).await;
    let (resource, _, list_body, _) =
        policy_fixture(CloudFrontPolicyKind::Cache, "policy-max", "MaxItems");
    Mock::given(method("GET"))
        .and(path(format!("/2020-05-31/{resource}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            list_body.replace("<MaxItems>10</MaxItems>", "<MaxItems>1</MaxItems>"),
        ))
        .mount(&server)
        .await;

    let error = transport
        .list_policies(
            CloudFrontPolicyKind::Cache,
            CloudFrontPolicyScope::AwsManaged,
            None,
            10,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "cloudfront_policy_page_scope_mismatch");
}

#[tokio::test]
async fn policy_inventory_rejects_list_get_revision_drift() {
    let server = MockServer::start().await;
    let transport = api(&server).await;
    let (resource, _, list_body, _) =
        policy_fixture(CloudFrontPolicyKind::Cache, "policy-drift", "Before");
    let (_, _, _, get_body) = policy_fixture(CloudFrontPolicyKind::Cache, "policy-drift", "After");
    Mock::given(method("GET"))
        .and(path(format!("/2020-05-31/{resource}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(list_body))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/2020-05-31/{resource}/policy-drift")))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "ETAG-DRIFT")
                .set_body_string(get_body),
        )
        .mount(&server)
        .await;

    let error = transport
        .list_policies(
            CloudFrontPolicyKind::Cache,
            CloudFrontPolicyScope::AwsManaged,
            None,
            10,
        )
        .await
        .unwrap_err();
    assert_eq!(error.category(), ProviderErrorCategory::Conflict);
    assert_eq!(error.code(), "cloudfront_policy_observation_changed");
}

#[tokio::test]
async fn policy_inventory_rejects_a_provider_scope_mismatch_before_get() {
    let server = MockServer::start().await;
    let transport = api(&server).await;
    let (resource, _, list_body, _) =
        policy_fixture(CloudFrontPolicyKind::Cache, "policy-scope", "ScopeMismatch");
    Mock::given(method("GET"))
        .and(path(format!("/2020-05-31/{resource}")))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(list_body.replace("<Type>managed</Type>", "<Type>custom</Type>")),
        )
        .mount(&server)
        .await;

    let error = transport
        .list_policies(
            CloudFrontPolicyKind::Cache,
            CloudFrontPolicyScope::AwsManaged,
            None,
            10,
        )
        .await
        .unwrap_err();
    assert_eq!(error.code(), "cloudfront_policy_scope_mismatch");
    assert_eq!(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.url.path() != "/")
            .count(),
        1
    );
}

#[tokio::test]
async fn policy_inventory_rejects_invalid_revision_metadata() {
    for (etag, time, expected_code) in [
        (
            "E".repeat(257),
            "2026-07-18T05:00:00Z",
            "invalid_cloudfront_policy_etag",
        ),
        (
            "ETAG".to_string(),
            "1969-12-31T23:59:59Z",
            "invalid_cloudfront_policy_last_modified",
        ),
    ] {
        let server = MockServer::start().await;
        let transport = api(&server).await;
        let (resource, _, list_body, get_body) =
            policy_fixture(CloudFrontPolicyKind::Cache, "policy-revision", "Revision");
        let list_body = list_body.replace("2026-07-18T05:00:00Z", time);
        let get_body = get_body.replace("2026-07-18T05:00:00Z", time);
        Mock::given(method("GET"))
            .and(path(format!("/2020-05-31/{resource}")))
            .respond_with(ResponseTemplate::new(200).set_body_string(list_body))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/2020-05-31/{resource}/policy-revision")))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("etag", etag.as_str())
                    .set_body_string(get_body),
            )
            .mount(&server)
            .await;

        let error = transport
            .list_policies(
                CloudFrontPolicyKind::Cache,
                CloudFrontPolicyScope::AwsManaged,
                None,
                10,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), expected_code);
    }
}

#[tokio::test]
async fn policy_inventory_rejects_unsafe_next_markers() {
    for marker in [" next-page", "next-page ", "next&#10;page"] {
        let server = MockServer::start().await;
        let transport = api(&server).await;
        let (resource, _, list_body, _) =
            policy_fixture(CloudFrontPolicyKind::Cache, "policy-marker", "Marker");
        Mock::given(method("GET"))
            .and(path(format!("/2020-05-31/{resource}")))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(list_body.replace("next-page", marker)),
            )
            .mount(&server)
            .await;

        let error = transport
            .list_policies(
                CloudFrontPolicyKind::Cache,
                CloudFrontPolicyScope::AwsManaged,
                None,
                10,
            )
            .await
            .unwrap_err();
        assert_eq!(error.code(), "invalid_cloudfront_policy_next_marker");
    }
}

#[tokio::test]
async fn list_rejects_a_page_that_does_not_echo_the_requested_marker() {
    let server = MockServer::start().await;
    let transport = api(&server).await;
    let body = EMPTY_LIST.replace("<Marker></Marker>", "<Marker>wrong</Marker>");
    Mock::given(method("GET"))
        .and(path("/2020-05-31/distribution"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .mount(&server)
        .await;

    let error = transport
        .list_distributions(Some("expected"), 25)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "cloudfront_distribution_page_scope_mismatch");
}

#[tokio::test]
async fn oversized_provider_body_fails_before_xml_deserialization() {
    let server = MockServer::start().await;
    let transport = api(&server).await;
    Mock::given(method("GET"))
        .and(path("/2020-05-31/distribution"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![b'x'; 4 * 1024 * 1024 + 1]))
        .mount(&server)
        .await;

    let error = transport.list_distributions(None, 25).await.unwrap_err();
    assert_eq!(error.code(), "cloudfront_transport_error");
    assert!(!format!("{error:?}").contains("xxxxxxxx"));
}

#[tokio::test]
async fn detail_requires_matching_etags_and_redacts_origin_header_values() {
    let server = MockServer::start().await;
    let transport = api(&server).await;
    const SECRET: &str = "do-not-cross-the-api-seam";
    Mock::given(method("GET"))
        .and(path(format!("/2020-05-31/distribution/{DISTRIBUTION_ID}")))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "E2MATCH")
                .set_body_string(get_distribution_body(SECRET)),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/2020-05-31/distribution/{DISTRIBUTION_ID}/config"
        )))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "E2MATCH")
                .set_body_string(format!(
                    r#"<?xml version="1.0" encoding="UTF-8"?>{}"#,
                    distribution_config(SECRET)
                )),
        )
        .expect(1)
        .mount(&server)
        .await;

    let detail = transport
        .get_distribution(DISTRIBUTION_ID)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(detail.etag, "E2MATCH");
    assert_eq!(detail.etag_revision_mac.as_str().len(), 64);
    assert_eq!(detail.config.origins[0].custom_header_count, 1);
    assert!(!format!("{detail:?}").contains("X-Origin-Secret"));
    assert!(detail.config.origins[0]
        .unsupported_features
        .contains("custom_origin_headers"));
    assert!(!format!("{detail:?}").contains(SECRET));
}

#[tokio::test]
async fn mismatched_detail_and_config_etags_fail_closed() {
    let server = MockServer::start().await;
    let transport = api(&server).await;
    Mock::given(method("GET"))
        .and(path(format!("/2020-05-31/distribution/{DISTRIBUTION_ID}")))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "E2OLD")
                .set_body_string(get_distribution_body("redacted")),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/2020-05-31/distribution/{DISTRIBUTION_ID}/config"
        )))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("etag", "E2NEW")
                .set_body_string(distribution_config("redacted")),
        )
        .mount(&server)
        .await;

    let error = transport
        .get_distribution(DISTRIBUTION_ID)
        .await
        .unwrap_err();
    assert_eq!(error.category(), ProviderErrorCategory::Conflict);
}

#[tokio::test]
async fn sts_identity_requires_an_exact_account_and_known_partition() {
    for (account, partition) in [("123", "aws"), (ACCOUNT_ID, "not-aws")] {
        let server = MockServer::start().await;
        mount_identity(&server, account, partition).await;
        let error = AwsCloudFrontApi::with_options(
            &sdk_config().await,
            CloudFrontFingerprintKey::new([9; 32]).unwrap(),
            "credential-test-1",
            AwsCloudFrontApiOptions {
                acm_endpoint_url: Some(server.uri()),
                cloudfront_endpoint_url: Some(server.uri()),
                sts_endpoint_url: Some(server.uri()),
            },
        )
        .await
        .err()
        .expect("invalid STS scope must fail construction");
        assert_eq!(error.category(), ProviderErrorCategory::Validation);
    }
}

#[tokio::test]
async fn describes_existing_us_east_1_certificate_without_mutation() {
    let server = MockServer::start().await;
    let transport = api(&server).await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(header(
            "x-amz-target",
            "CertificateManager.DescribeCertificate",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/x-amz-json-1.1")
                .set_body_json(serde_json::json!({
                    "Certificate": {
                        "CertificateArn": CERTIFICATE_ARN,
                        "DomainName": "example.com",
                        "SubjectAlternativeNames": ["example.com", "*.example.com"],
                        "Status": "ISSUED",
                        "Type": "AMAZON_ISSUED",
                        "KeyAlgorithm": "RSA_2048",
                        "ManagedBy": "CLOUDFRONT",
                        "NotBefore": 1784332800,
                        "NotAfter": 1815868800,
                        "InUseBy": [DISTRIBUTION_ARN]
                    }
                })),
        )
        .expect(1)
        .mount(&server)
        .await;

    let certificate = transport
        .describe_acm_certificate(CERTIFICATE_ARN)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(certificate.arn, CERTIFICATE_ARN);
    assert_eq!(certificate.account_id, ACCOUNT_ID);
    assert_eq!(certificate.region, "us-east-1");
    assert_eq!(certificate.domain_name, "example.com");
    assert_eq!(certificate.status, AcmCertificateStatus::Issued);
    assert_eq!(
        certificate.certificate_type,
        AcmCertificateType::AmazonIssued
    );
    assert_eq!(
        certificate.key_algorithm,
        AcmCertificateKeyAlgorithm::Rsa2048
    );
    assert_eq!(certificate.managed_by.as_deref(), Some("CLOUDFRONT"));
    assert_eq!(certificate.not_before_unix_seconds, Some(1_784_332_800));
    assert_eq!(certificate.not_after_unix_seconds, Some(1_815_868_800));
    assert!(certificate
        .subject_alternative_names
        .contains("*.example.com"));
    assert!(certificate.in_use_by.contains(DISTRIBUTION_ARN));
}

#[tokio::test]
async fn certificate_arn_must_match_verified_partition_account_and_us_east_1() {
    let server = MockServer::start().await;
    let transport = api(&server).await;
    for arn in [
        "arn:aws:acm:us-west-2:123456789012:certificate/12345678-1234-1234-1234-123456789012",
        "arn:aws:acm:us-east-1:999999999999:certificate/12345678-1234-1234-1234-123456789012",
        "arn:aws-cn:acm:us-east-1:123456789012:certificate/12345678-1234-1234-1234-123456789012",
    ] {
        let error = transport.describe_acm_certificate(arn).await.unwrap_err();
        assert_eq!(error.code(), "acm_certificate_arn_scope_mismatch");
    }
}

#[tokio::test]
async fn missing_certificate_is_an_optional_absence() {
    let server = MockServer::start().await;
    let transport = api(&server).await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(header(
            "x-amz-target",
            "CertificateManager.DescribeCertificate",
        ))
        .respond_with(
            ResponseTemplate::new(400)
                .insert_header("content-type", "application/x-amz-json-1.1")
                .insert_header("x-amzn-errortype", "ResourceNotFoundException")
                .set_body_json(serde_json::json!({
                    "__type": "ResourceNotFoundException",
                    "message": "not found"
                })),
        )
        .expect(1)
        .mount(&server)
        .await;

    assert!(transport
        .describe_acm_certificate(CERTIFICATE_ARN)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn certificate_response_cannot_change_the_requested_identity() {
    let server = MockServer::start().await;
    let transport = api(&server).await;
    Mock::given(method("POST"))
        .and(path("/"))
        .and(header(
            "x-amz-target",
            "CertificateManager.DescribeCertificate",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "application/x-amz-json-1.1")
                .set_body_json(serde_json::json!({
                    "Certificate": {
                        "CertificateArn": "arn:aws:acm:us-east-1:123456789012:certificate/87654321-4321-4321-4321-210987654321",
                        "DomainName": "example.com",
                        "SubjectAlternativeNames": ["example.com"],
                        "Status": "ISSUED",
                        "Type": "IMPORTED"
                    }
                })),
        )
        .mount(&server)
        .await;

    let error = transport
        .describe_acm_certificate(CERTIFICATE_ARN)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "acm_certificate_arn_mismatch");
}

#[tokio::test]
async fn domain_conflict_page_is_exact_scoped_read_only_and_preserves_masking() {
    let server = MockServer::start().await;
    let transport = api(&server).await;
    Mock::given(method("POST"))
        .and(path("/2020-05-31/domain-conflicts"))
        .and(body_string_contains("<Domain>www.example.test</Domain>"))
        .and(body_string_contains(format!(
            "<DistributionId>{DISTRIBUTION_ID}</DistributionId>"
        )))
        .and(body_string_contains("<MaxItems>100</MaxItems>"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DOMAIN_CONFLICTS_PAGE))
        .expect(1)
        .mount(&server)
        .await;

    let page = transport
        .list_domain_conflicts("www.example.test", DISTRIBUTION_ID, None, 100)
        .await
        .unwrap();
    assert_eq!(page.queried_domain, "www.example.test");
    assert_eq!(page.validation_distribution_id, DISTRIBUTION_ID);
    assert_eq!(page.next_marker.as_deref(), Some("next-conflict-page"));
    assert_eq!(page.items.len(), 1);
    assert_eq!(
        page.items[0].resource_type,
        CloudFrontDomainConflictResourceType::DistributionTenant
    );
    assert_eq!(page.items[0].account_id, "******789012");
    assert_eq!(page.items[0].resource_id, "***************ohWzv1b9It8J1AB");
}

#[tokio::test]
async fn domain_conflict_requests_and_unrelated_provider_items_fail_closed() {
    let server = MockServer::start().await;
    let transport = api(&server).await;
    for (domain, distribution_id, marker, max_items) in [
        ("*.example.test", DISTRIBUTION_ID, None, 100),
        ("WWW.example.test", DISTRIBUTION_ID, None, 100),
        ("www.exämple.test", DISTRIBUTION_ID, None, 100),
        ("www.example.test", "bad/id", None, 100),
        ("www.example.test", DISTRIBUTION_ID, Some("\n"), 100),
        ("www.example.test", DISTRIBUTION_ID, None, 0),
        ("www.example.test", DISTRIBUTION_ID, None, 101),
    ] {
        assert!(transport
            .list_domain_conflicts(domain, distribution_id, marker, max_items)
            .await
            .is_err());
    }

    let unrelated = DOMAIN_CONFLICTS_PAGE.replace("*.example.test", "other.example.test");
    Mock::given(method("POST"))
        .and(path("/2020-05-31/domain-conflicts"))
        .respond_with(ResponseTemplate::new(200).set_body_string(unrelated))
        .expect(1)
        .mount(&server)
        .await;
    let error = transport
        .list_domain_conflicts("www.example.test", DISTRIBUTION_ID, None, 100)
        .await
        .unwrap_err();
    assert_eq!(error.code(), "unrelated_cloudfront_domain_conflict");
}

#[tokio::test]
async fn missing_domain_conflict_validation_distribution_is_not_transient() {
    let server = MockServer::start().await;
    let transport = api(&server).await;
    Mock::given(method("POST"))
        .and(path("/2020-05-31/domain-conflicts"))
        .respond_with(ResponseTemplate::new(404).set_body_string(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<ErrorResponse xmlns="http://cloudfront.amazonaws.com/doc/2020-05-31/">
  <Error><Type>Sender</Type><Code>EntityNotFound</Code><Message>missing</Message></Error>
  <RequestId>request-id</RequestId>
</ErrorResponse>"#,
        ))
        .expect(1)
        .mount(&server)
        .await;

    let error = transport
        .list_domain_conflicts("www.example.test", DISTRIBUTION_ID, None, 100)
        .await
        .unwrap_err();
    assert_eq!(error.category(), ProviderErrorCategory::NotFound);
    assert_eq!(error.code(), "cloudfront_service_error");
}
