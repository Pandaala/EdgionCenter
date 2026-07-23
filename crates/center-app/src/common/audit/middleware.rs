//! Axum middleware that records mutating admin actions into the audit log.
//!
//! Placement contract: this layer MUST run *inside* `unified_auth` so the
//! injected [`UnifiedAuthClaims`] are present in the request extensions. It is
//! applied to the business router before `compose_admin_routes` wraps it with
//! `unified_auth` (outermost), which places it correctly. See `cli::run`.
//!
//! The middleware runs the inner handler, captures the response status, builds
//! an [`AuditRecord`], and hands it to the [`AuditSink`] (non-blocking). It
//! never adds request latency: the actual DB write happens off-path in the
//! sink's background task.

use std::net::SocketAddr;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::Method;
use axum::middleware::Next;
use axum::response::Response;

use super::AuditSink;
use crate::common::authz::catalog;
use crate::common::unified_auth::{AuthProvider, UnifiedAuthClaims};
use edgion_center_core::AuditEvent;

/// The audit-read endpoint (added by a later task). It is always excluded from
/// recording so listing audit logs does not generate fresh audit entries.
const AUDIT_READ_PATH: &str = "/api/v1/center/admin/audit-logs";

/// Prefix of the controller proxy routes, used to extract `target_controller`.
const PROXY_PREFIX: &str = "/api/v1/proxy/";

/// State for the audit middleware: the sink plus the read-logging policy.
#[derive(Clone)]
pub struct AuditLayerState {
    /// Where records are handed off (non-blocking).
    pub sink: AuditSink,
    /// Whether GET requests are recorded (mutations are always recorded).
    pub log_reads: bool,
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Whether a request should be recorded: mutations always; GET only when
/// `log_reads`; the audit-read path never (avoids self-logging loops).
fn should_record(method: &Method, path: &str, log_reads: bool) -> bool {
    if path == AUDIT_READ_PATH {
        return false;
    }
    match *method {
        Method::POST | Method::PUT | Method::DELETE | Method::PATCH => true,
        Method::GET => log_reads,
        _ => false,
    }
}

/// For `/api/v1/proxy/{controller_id}/...`, extract and decode the first path
/// segment (`~` -> `/`, mirroring `api::proxy_handler`). `None` for other routes.
///
/// `path` here is `req.uri().path()`, which is NOT percent-decoded; for ordinary
/// controller ids this matches `proxy_handler`'s `~`->`/` decode, but a controller
/// id containing percent-encoded bytes could differ between the two.
fn parse_target_controller(path: &str) -> Option<String> {
    let rest = path.strip_prefix(PROXY_PREFIX)?;
    let seg = rest.split('/').next()?;
    if seg.is_empty() {
        return None;
    }
    Some(seg.replace('~', "/"))
}

/// Return a stable, body-independent action summary for Cloudflare WAF writes.
///
/// Keeping this path-derived is deliberate: WAF request bodies can contain
/// security expressions and must never be copied into the audit stream.
fn cloudflare_waf_action(method: &Method, path: &str) -> Option<String> {
    const PREFIX: &str = "/api/v1/center/cloudflare/waf/";
    if !path.starts_with(PREFIX) {
        return None;
    }

    let resource = if path.ends_with("/security-weaken") {
        "security_weaken"
    } else if path.ends_with("/order") {
        "order"
    } else if path.ends_with("/exceptions") {
        "managed_exception"
    } else if path.contains("/managed-rules") {
        "managed_rule"
    } else if path.contains("/custom-rules") {
        "custom_rule"
    } else if path.contains("/rate-limits") {
        "rate_limit"
    } else {
        "ruleset"
    };

    let operation = match *method {
        Method::POST => "create",
        Method::PUT | Method::PATCH => "update",
        Method::DELETE => "delete",
        _ => return None,
    };
    Some(format!("cloudflare_waf_{resource}_{operation}"))
}

/// Return stable, body-independent AWS WAF mutation summaries. Paths may
/// contain provider ARNs or opaque identifiers, so only route shape and method
/// contribute to audit detail.
fn aws_waf_action(method: &Method, path: &str) -> Option<String> {
    const PREFIX: &str = "/api/v1/center/aws/waf/accounts/";
    if !path.starts_with(PREFIX) {
        return None;
    }
    let resource = if path.ends_with("/security-weaken") && path.contains("/ip-sets/") {
        "ip_set_security_weaken"
    } else if path.ends_with("/security-weaken") && path.contains("/rules/") {
        "rule_security_weaken"
    } else if path.ends_with("/security-weaken") {
        "web_acl_security_weaken"
    } else if path.ends_with("/exceptions") {
        "managed_exception"
    } else if path.ends_with("/associations") {
        "association"
    } else if path.ends_with("/capacity") {
        "capacity"
    } else if path.contains("/ip-sets") {
        "ip_set"
    } else if path.contains("/rules") {
        "rule"
    } else {
        "web_acl"
    };
    let operation = match *method {
        Method::POST => "create",
        Method::PUT | Method::PATCH => "update",
        Method::DELETE => "delete",
        _ => return None,
    };
    Some(format!("aws_waf_{resource}_{operation}"))
}

/// Return body-independent lifecycle actions for provider operations that may carry opaque
/// concurrency tokens, association identifiers, or signed confirmations in their request body.
fn cloud_lifecycle_action(method: &Method, path: &str) -> Option<String> {
    const CLOUDFRONT: &str = "/api/v1/center/aws/cloudfront/accounts/";
    const ROUTE53: &str = "/api/v1/center/aws/route53/accounts/";
    if path.starts_with(CLOUDFRONT) {
        let action = match *method {
            Method::POST if path.ends_with("/distributions") => "distribution_create",
            Method::PUT if path.ends_with("/origin") => "origin_update",
            Method::POST if path.ends_with("/enable") => "distribution_enable",
            Method::POST if path.ends_with("/disable") => "distribution_disable",
            Method::PUT if path.ends_with("/web-acl") => "web_acl_attach_or_replace",
            Method::DELETE if path.ends_with("/web-acl") => "web_acl_detach",
            Method::DELETE => "distribution_delete",
            _ => return None,
        };
        return Some(format!("cloudfront_{action}"));
    }
    if path.starts_with(ROUTE53) {
        let action = match *method {
            Method::POST if path.ends_with("/hosted-zones") => "zone_create",
            Method::GET if path.ends_with("/lifecycle") => "zone_observe",
            Method::DELETE if path.ends_with("/lifecycle") => "zone_delete",
            _ => return None,
        };
        return Some(format!("route53_{action}"));
    }
    None
}

/// Classify a cloud route without retaining provider account IDs, zone IDs,
/// distribution IDs, ARNs, rule IDs, or other user-controlled path segments.
fn cloud_audit_target(path: &str) -> Option<&'static str> {
    if path.starts_with("/api/v1/center/cloud/provider-accounts") {
        return Some("provider_account");
    }
    if path.starts_with("/api/v1/center/cloud/provider-capabilities/") {
        return Some("provider_capability");
    }
    if path.starts_with("/api/v1/center/cloud/provider-credential-inspections/") {
        return Some("provider_credential_inspection");
    }
    if path.starts_with("/api/v1/center/cloudflare/dns/") {
        return Some(if path.contains("/record-sets/") {
            "cloudflare_dns_record_set"
        } else {
            "cloudflare_dns_zone"
        });
    }
    if path.starts_with("/api/v1/center/cloudflare/waf/") {
        return Some(if path.contains("/managed-rules") {
            "cloudflare_waf_managed_rule"
        } else if path.contains("/custom-rules") {
            "cloudflare_waf_custom_rule"
        } else if path.contains("/rate-limits") {
            "cloudflare_waf_rate_limit"
        } else {
            "cloudflare_waf_ruleset"
        });
    }
    if path.starts_with("/api/v1/center/aws/route53/") {
        return Some(
            if path.contains("/record-sets") || path.contains("/change-batches") {
                "route53_record_set"
            } else if path.contains("/changes/") {
                "route53_change"
            } else {
                "route53_hosted_zone"
            },
        );
    }
    if path.starts_with("/api/v1/center/aws/cloudfront/") {
        return Some(if path.ends_with("/origin") {
            "cloudfront_origin"
        } else if path.ends_with("/web-acl") {
            "cloudfront_web_acl_association"
        } else {
            "cloudfront_distribution"
        });
    }
    if path.starts_with("/api/v1/center/aws/waf/") {
        return Some(if path.contains("/ip-sets") {
            "aws_waf_ip_set"
        } else if path.contains("/rules/") || path.ends_with("/rules") {
            "aws_waf_rule"
        } else if path.contains("/associations") {
            "aws_waf_association"
        } else if path.ends_with("/capacity") {
            "aws_waf_capacity_check"
        } else {
            "aws_waf_web_acl"
        });
    }
    None
}

/// Produce the stored path and structured detail for cloud API calls.
///
/// The original path is deliberately discarded because it may carry AWS ARNs
/// and provider resource identifiers. The exact permission remains linked to
/// the actor and request ID through the surrounding `AuditEvent`.
fn sanitized_cloud_audit(
    method: &Method,
    path: &str,
    action: Option<String>,
) -> Option<(String, String)> {
    let target = cloud_audit_target(path)?;
    let permission = catalog::route_permission(method, path).unwrap_or("<unmapped-cloud-route>");
    let operation = action.unwrap_or_else(|| {
        if matches!(*method, Method::GET | Method::HEAD) {
            "read".to_string()
        } else {
            method.as_str().to_ascii_lowercase()
        }
    });
    let detail = serde_json::json!({
        "permission": permission,
        "target": target,
        "action": operation,
    })
    .to_string();
    Some((format!("/api/v1/center/cloud-audit/{target}"), detail))
}

/// Resolve `(actor, provider)` from the unified-auth claims, falling back to
/// `<unknown>` / empty when claims are absent.
fn actor_and_provider(req: &Request) -> (String, String) {
    match req.extensions().get::<UnifiedAuthClaims>() {
        Some(claims) => {
            let actor = claims
                .sub
                .clone()
                .unwrap_or_else(|| "<unknown>".to_string());
            let provider = match claims.provider {
                AuthProvider::Oidc => "oidc",
                AuthProvider::Local => "local",
            }
            .to_string();
            (actor, provider)
        }
        None => ("<unknown>".to_string(), String::new()),
    }
}

/// Audit middleware. Captures attribution before running the handler, then —
/// for recordable requests — builds and hands off an [`AuditRecord`].
pub async fn audit_middleware(
    State(state): State<AuditLayerState>,
    req: Request,
    next: Next,
) -> Response {
    // Decide recordability first, from borrowed inputs only (no allocation): the
    // common GET path (default `log_reads=false`) skips all attribution work below.
    if !should_record(req.method(), req.uri().path(), state.log_reads) {
        return next.run(req).await;
    }

    // Recordable: capture everything we need from the request before it is consumed.
    let method = req.method().clone();
    let original_path = req.uri().path().to_string();
    // Source IP comes from the TCP peer (ConnectInfo) only — never X-Forwarded-For.
    let source_ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ci| ci.0.ip().to_canonical().to_string());
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let (actor, provider) = actor_and_provider(&req);
    let target_controller = parse_target_controller(&original_path);
    let action = cloudflare_waf_action(&method, &original_path)
        .or_else(|| aws_waf_action(&method, &original_path))
        .or_else(|| cloud_lifecycle_action(&method, &original_path));
    let (path, detail) = sanitized_cloud_audit(&method, &original_path, action)
        .map(|(path, detail)| (path, Some(detail)))
        .unwrap_or((original_path, None));

    let resp = next.run(req).await;

    let rec = AuditEvent {
        ts: unix_now(),
        actor,
        provider,
        method: method.to_string(),
        path,
        target_controller,
        status: resp.status().as_u16() as i32,
        source_ip,
        request_id,
        detail,
    };
    state.sink.record(rec);

    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request as HttpRequest, StatusCode};
    use axum::routing::{get, post, put};
    use axum::Router;
    use tokio::sync::mpsc;
    use tower::ServiceExt;

    #[test]
    fn waf_action_is_stable_and_contains_no_request_data() {
        assert_eq!(
            cloudflare_waf_action(
                &Method::PUT,
                "/api/v1/center/cloudflare/waf/accounts/account-1/zones/zone-1/custom-rules/rule-1/order",
            ),
            Some("cloudflare_waf_order_update".to_string())
        );
        assert_eq!(
            cloudflare_waf_action(
                &Method::DELETE,
                "/api/v1/center/cloudflare/waf/accounts/account-1/zones/zone-1/rate-limits/rule-1/security-weaken",
            ),
            Some("cloudflare_waf_security_weaken_delete".to_string())
        );
        assert_eq!(
            cloudflare_waf_action(&Method::POST, "/api/v1/center/admin/controllers"),
            None
        );
    }

    #[test]
    fn aws_waf_actions_are_path_only_and_cover_mutation_families() {
        let root = "/api/v1/center/aws/waf/accounts/aws-main/scopes/regional";
        assert_eq!(
            aws_waf_action(&Method::POST, &format!("{root}/web-acls")),
            Some("aws_waf_web_acl_create".to_string())
        );
        assert_eq!(
            aws_waf_action(
                &Method::PUT,
                &format!("{root}/web-acls/acl/rules/ref/security-weaken")
            ),
            Some("aws_waf_rule_security_weaken_update".to_string())
        );
        assert_eq!(
            aws_waf_action(
                &Method::PUT,
                &format!("{root}/web-acls/acl/rules/ref/exceptions")
            ),
            Some("aws_waf_managed_exception_update".to_string())
        );
        assert_eq!(
            aws_waf_action(
                &Method::PUT,
                &format!("{root}/web-acls/acl/security-weaken")
            ),
            Some("aws_waf_web_acl_security_weaken_update".to_string())
        );
        assert_eq!(
            aws_waf_action(
                &Method::DELETE,
                &format!("{root}/ip-sets/id/security-weaken")
            ),
            Some("aws_waf_ip_set_security_weaken_delete".to_string())
        );
        assert_eq!(
            aws_waf_action(&Method::POST, &format!("{root}/web-acls/acl/associations")),
            Some("aws_waf_association_create".to_string())
        );
        assert_eq!(
            aws_waf_action(&Method::POST, &format!("{root}/capacity")),
            Some("aws_waf_capacity_create".to_string())
        );
    }

    #[test]
    fn cloud_lifecycle_actions_are_path_only_and_stable() {
        let cloudfront = "/api/v1/center/aws/cloudfront/accounts/aws-main/distributions/E123";
        assert_eq!(
            cloud_lifecycle_action(&Method::PUT, &format!("{cloudfront}/web-acl")),
            Some("cloudfront_web_acl_attach_or_replace".to_string())
        );
        assert_eq!(
            cloud_lifecycle_action(&Method::DELETE, &format!("{cloudfront}/web-acl")),
            Some("cloudfront_web_acl_detach".to_string())
        );
        assert_eq!(
            cloud_lifecycle_action(
                &Method::DELETE,
                "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z123/lifecycle"
            ),
            Some("route53_zone_delete".to_string())
        );
        assert_eq!(
            cloud_lifecycle_action(
                &Method::POST,
                "/api/v1/center/aws/route53/accounts/aws-main/hosted-zones/Z123/lifecycle"
            ),
            None,
            "only mounted lifecycle methods are attributed"
        );
    }

    #[test]
    fn cloud_audit_discards_raw_identifiers_and_links_permission_to_target() {
        let raw_path = "/api/v1/center/aws/waf/accounts/123456789012/scopes/regional/web-acls/arn%3Aaws%3Awafv2%3Aus-east-1%3A123456789012%3Aregional%2Fwebacl%2Fsecret/rules/private-rule/security-weaken";
        let action = aws_waf_action(&Method::PUT, raw_path);
        let (path, detail) =
            sanitized_cloud_audit(&Method::PUT, raw_path, action).expect("cloud route");

        assert_eq!(path, "/api/v1/center/cloud-audit/aws_waf_rule");
        assert!(!path.contains("123456789012"));
        assert!(!detail.contains("123456789012"));
        assert!(!detail.contains("private-rule"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&detail).unwrap(),
            serde_json::json!({
                "permission": catalog::AWS_WAF_SECURITY_WEAKEN,
                "target": "aws_waf_rule",
                "action": "aws_waf_rule_security_weaken_update",
            })
        );
    }

    #[test]
    fn cloud_audit_covers_every_retained_provider_family() {
        let cases = [
            (
                Method::PUT,
                "/api/v1/center/cloudflare/dns/accounts/cf/zones/z/record-sets/A",
                "cloudflare_dns_record_set",
                catalog::CLOUDFLARE_DNS_WRITE,
            ),
            (
                Method::POST,
                "/api/v1/center/cloudflare/waf/accounts/cf/zones/z/custom-rules",
                "cloudflare_waf_custom_rule",
                catalog::CLOUDFLARE_WAF_WRITE,
            ),
            (
                Method::POST,
                "/api/v1/center/aws/route53/accounts/aws/hosted-zones",
                "route53_hosted_zone",
                catalog::ROUTE53_ZONES_WRITE,
            ),
            (
                Method::PUT,
                "/api/v1/center/aws/cloudfront/accounts/aws/distributions/d/origin",
                "cloudfront_origin",
                catalog::CLOUDFRONT_WRITE,
            ),
            (
                Method::POST,
                "/api/v1/center/cloud/provider-credential-inspections/accounts/aws/refresh",
                "provider_credential_inspection",
                catalog::PROVIDER_CREDENTIALS_INSPECT,
            ),
        ];

        for (method, raw_path, target, permission) in cases {
            let (stored_path, detail) =
                sanitized_cloud_audit(&method, raw_path, None).expect("cloud route");
            assert_eq!(stored_path, format!("/api/v1/center/cloud-audit/{target}"));
            let detail: serde_json::Value = serde_json::from_str(&detail).unwrap();
            assert_eq!(detail["target"], target);
            assert_eq!(detail["permission"], permission);
            assert!(!stored_path.contains("aws"));
        }
    }

    /// Build a test app: an inner layer injects fake claims, the audit middleware
    /// sits inside it, and routes echo 200. Returns the app + the drained receiver.
    struct ChannelWriter(mpsc::Sender<AuditEvent>);

    impl edgion_center_core::AuditWriter for ChannelWriter {
        fn record(&self, event: AuditEvent) {
            let _ = self.0.try_send(event);
        }
    }

    fn app_with_claims(log_reads: bool) -> (Router, mpsc::Receiver<AuditEvent>) {
        let (tx, rx) = mpsc::channel::<AuditEvent>(16);
        let sink: AuditSink = std::sync::Arc::new(ChannelWriter(tx));
        let state = AuditLayerState { sink, log_reads };

        let app = Router::new()
            .route("/api/v1/center/admin/controllers", post(|| async { "ok" }))
            .route("/api/v1/clusters", get(|| async { "ok" }))
            .route(
                "/api/v1/proxy/{controller_id}/{*rest}",
                post(|| async { "ok" }),
            )
            .route(
                "/api/v1/center/aws/cloudfront/accounts/{account_id}/distributions/{distribution_id}/origin",
                put(|| async { "ok" }),
            )
            // Audit layer (inner)...
            .layer(axum::middleware::from_fn_with_state(
                state,
                audit_middleware,
            ))
            // ...wrapped by a claims-injecting layer (outer, runs first).
            .layer(axum::middleware::from_fn(
                |mut req: Request, next: Next| async move {
                    req.extensions_mut().insert(UnifiedAuthClaims {
                        provider: AuthProvider::Local,
                        sub: Some("alice".to_string()),
                        iss: None,
                        groups: Vec::new(),
                        claims: serde_json::Value::Null,
                    });
                    next.run(req).await
                },
            ));
        (app, rx)
    }

    #[tokio::test]
    async fn middleware_records_mutation() {
        let (app, mut rx) = app_with_claims(false);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/v1/center/admin/controllers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let rec = rx.try_recv().expect("a mutation must be recorded");
        assert_eq!(rec.actor, "alice");
        assert_eq!(rec.provider, "local");
        assert_eq!(rec.method, "POST");
        assert_eq!(rec.path, "/api/v1/center/admin/controllers");
        assert_eq!(rec.status, 200);
        assert_eq!(rec.target_controller, None);
        assert!(rx.try_recv().is_err(), "exactly one record expected");
    }

    #[tokio::test]
    async fn middleware_skips_get_when_log_reads_false() {
        let (app, mut rx) = app_with_claims(false);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::GET)
                    .uri("/api/v1/clusters")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(
            rx.try_recv().is_err(),
            "GET must not be recorded when log_reads=false"
        );
    }

    #[tokio::test]
    async fn middleware_records_get_when_log_reads_true() {
        let (app, mut rx) = app_with_claims(true);
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::GET)
                    .uri("/api/v1/clusters")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let rec = rx
            .try_recv()
            .expect("GET must be recorded when log_reads=true");
        assert_eq!(rec.method, "GET");
    }

    #[tokio::test]
    async fn middleware_extracts_target_controller_from_proxy_path() {
        let (app, mut rx) = app_with_claims(false);
        // "~" encodes "/" in the controller id segment.
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/v1/proxy/cluster-a~ctrl-1/api/v1/foo")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let rec = rx.try_recv().expect("proxy mutation must be recorded");
        assert_eq!(rec.target_controller.as_deref(), Some("cluster-a/ctrl-1"));
    }

    #[tokio::test]
    async fn middleware_links_cloud_actor_permission_target_and_request_id() {
        let (app, mut rx) = app_with_claims(false);
        let raw_path =
            "/api/v1/center/aws/cloudfront/accounts/123456789012/distributions/SECRET/origin";
        let resp = app
            .oneshot(
                HttpRequest::builder()
                    .method(Method::PUT)
                    .uri(raw_path)
                    .header("x-request-id", "req-cloud-1")
                    .body(Body::from(r#"{"origin":"sensitive.example"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let rec = rx.try_recv().expect("cloud mutation must be recorded");
        assert_eq!(rec.actor, "alice");
        assert_eq!(rec.request_id.as_deref(), Some("req-cloud-1"));
        assert_eq!(rec.path, "/api/v1/center/cloud-audit/cloudfront_origin");
        let detail: serde_json::Value =
            serde_json::from_str(rec.detail.as_deref().expect("cloud detail")).unwrap();
        assert_eq!(detail["permission"], catalog::CLOUDFRONT_WRITE);
        assert_eq!(detail["target"], "cloudfront_origin");
        let serialized = serde_json::to_string(&rec).unwrap();
        assert!(!serialized.contains("123456789012"));
        assert!(!serialized.contains("SECRET"));
        assert!(!serialized.contains("sensitive.example"));
    }

    #[test]
    fn should_record_policy() {
        assert!(should_record(&Method::POST, "/x", false));
        assert!(should_record(&Method::DELETE, "/x", false));
        assert!(!should_record(&Method::GET, "/x", false));
        assert!(should_record(&Method::GET, "/x", true));
        // Audit-read path is never recorded, regardless of method or log_reads.
        assert!(!should_record(&Method::GET, AUDIT_READ_PATH, true));
        assert!(!should_record(&Method::POST, AUDIT_READ_PATH, true));
    }
}
