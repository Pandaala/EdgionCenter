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
    let path = req.uri().path().to_string();
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
    let target_controller = parse_target_controller(&path);
    let detail = cloudflare_waf_action(&method, &path);

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
    use axum::routing::{get, post};
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
