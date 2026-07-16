//! Authorization middleware.
//!
//! Applied INSIDE `unified_auth` (so the `UnifiedAuthClaims` it injects are
//! visible here) and AROUND both the shared auth routes and business routes
//! (so `/auth/me` can read the resolved permissions). For each request it:
//!
//! 1. Reads `UnifiedAuthClaims` from extensions. If absent — a public skip-path
//!    such as login/status reached without a token — it runs the next layer
//!    with no enforcement and no injection.
//! 2. Otherwise builds a core [`Principal`], resolves a [`PermissionSet`] via
//!    the installed [`Authorizer`], and injects the set into request extensions so
//!    downstream handlers (notably `/auth/me`) can report it.
//! 3. Enforces: if `route_permission(method, path)` is `Some(key)` and the set
//!    does not contain it, responds `403`; otherwise continues.

use std::sync::Arc;

use axum::{
    body::Body,
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};

use edgion_center_core::{Action, ActionOperation, Authorizer, ControllerId, Principal};

use super::{catalog, PermissionSet};
use crate::common::unified_auth::{AuthProvider, UnifiedAuthClaims};

/// Build the 403 JSON body returned when a caller lacks the required key.
fn forbidden_response(required: &str) -> Response {
    let body = Json(serde_json::json!({
        "success": false,
        "error": "forbidden: missing permission",
        "required_permission": required,
    }));
    (StatusCode::FORBIDDEN, body).into_response()
}

/// Keep policy denials (403) distinct from an unavailable policy backend.
/// The response intentionally omits adapter details, which may contain
/// API-server or identity-provider internals.
fn authorization_unavailable_response() -> Response {
    let body = Json(serde_json::json!({
        "success": false,
        "error": "authorization service unavailable",
    }));
    (StatusCode::SERVICE_UNAVAILABLE, body).into_response()
}

fn invalid_principal_response() -> Response {
    let body = Json(serde_json::json!({
        "success": false,
        "error": "invalid authenticated principal",
    }));
    (StatusCode::UNAUTHORIZED, body).into_response()
}

/// Authorization middleware. See the module docs for the full contract.
///
/// AuthBypass invariant: when `unified_auth` honors an `AuthBypass` extension it
/// returns early WITHOUT injecting `UnifiedAuthClaims`. Such a request reaches
/// this middleware with no claims and so takes the no-claims branch below —
/// passing through with NO authz enforcement at all. AuthBypass therefore means
/// full trust by design. No Center admin route injects AuthBypass today; this
/// note records the invariant so a future bypass path isn't added blindly.
pub async fn authz_middleware(
    State(authorizer): State<Arc<dyn Authorizer>>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    // No authenticated claims (public skip-path without a token, or an
    // AuthBypass'd request — see the fn doc) → pass through untouched: no
    // enforcement, no injection.
    let principal = {
        let Some(claims) = request.extensions().get::<UnifiedAuthClaims>() else {
            return next.run(request).await;
        };
        // Borrow the claims: copy only the provider tag and clone only `sub`,
        // never the (potentially large) `claims: serde_json::Value`. The borrow
        // is dropped at the end of this block, before `next.run`.
        let provider = match claims.provider {
            AuthProvider::Oidc => "oidc",
            AuthProvider::Local => "local",
        };
        let Some(subject) = claims.sub.clone() else {
            tracing::warn!(component = "authz", "authenticated claims omitted subject");
            return invalid_principal_response();
        };
        Principal {
            subject,
            provider: provider.to_string(),
            issuer: claims.iss.clone(),
            groups: claims.groups.clone(),
        }
    };

    // Enforcement against the route's required key.
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let perms = match catalog::route_permission(&method, &path) {
        Some(key) => {
            let action = match action_for_request(key, &method, &path) {
                Ok(action) => action,
                Err(()) => return forbidden_response("<invalid-controller-id>"),
            };
            match authorizer.authorize(&principal, &action).await {
                Ok(decision) if decision.allowed => PermissionSet::from_keys([key.to_string()]),
                Ok(_) => {
                    tracing::debug!(
                        component = "authz",
                        subject = %principal.subject,
                        provider = %principal.provider,
                        method = %method,
                        path = %path,
                        required = key,
                        "authorization denied: missing permission"
                    );
                    return forbidden_response(key);
                }
                Err(error) => {
                    tracing::warn!(component = "authz", %error, "authorization failed closed");
                    return authorization_unavailable_response();
                }
            }
        }
        None => {
            // Unmapped route. Fail CLOSED for business paths: a future business
            // route nobody added to `route_permission` must NOT be silently
            // reachable by everyone once real RBAC lands. The selected policy
            // must explicitly authorize the synthetic fail-closed action.
            if catalog::is_business_path(&path) {
                let action = Action {
                    permission: "<unmapped-business-route>".to_string(),
                    controller_id: None,
                    operation: Some(ActionOperation::Execute),
                    request_path: Some(path.clone()),
                    request_verb: Some(method.as_str().to_ascii_lowercase()),
                };
                let allowed = match authorizer.authorize(&principal, &action).await {
                    Ok(decision) => decision.allowed,
                    Err(error) => {
                        tracing::warn!(component = "authz", %error, "authorization failed closed");
                        return authorization_unavailable_response();
                    }
                };
                if !allowed {
                    tracing::debug!(
                        component = "authz",
                        subject = %principal.subject,
                        provider = %principal.provider,
                        method = %method,
                        path = %path,
                        "authorization denied: unmapped business route (fail-closed)"
                    );
                    return forbidden_response("<unmapped-business-route>");
                }
            }

            if path == "/api/v1/auth/me" {
                let candidates: Vec<String> = catalog::all_keys()
                    .iter()
                    .map(|key| (*key).to_string())
                    .collect();
                match authorizer
                    .granted_permissions(&principal, &candidates)
                    .await
                {
                    Ok(Some(allowed)) => PermissionSet::from_keys(allowed),
                    Ok(None) => PermissionSet::from_keys(Vec::<String>::new()),
                    Err(error) => {
                        tracing::warn!(component = "authz", %error, "permission enumeration failed closed");
                        return authorization_unavailable_response();
                    }
                }
            } else {
                PermissionSet::from_keys(Vec::<String>::new())
            }
        }
    };

    // Make the resolved permissions available to downstream handlers.
    request.extensions_mut().insert(perms);
    next.run(request).await
}

fn action_for_request(
    permission: &str,
    method: &axum::http::Method,
    path: &str,
) -> Result<Action, ()> {
    let operation = if method == axum::http::Method::GET {
        if path == "/api/v1/controllers"
            || path == "/api/v1/clusters"
            || path == "/api/v1/center/admin/controllers"
        {
            ActionOperation::List
        } else {
            ActionOperation::Get
        }
    } else if method == axum::http::Method::DELETE {
        ActionOperation::Delete
    } else if method == axum::http::Method::PATCH || method == axum::http::Method::PUT {
        ActionOperation::Update
    } else if path.ends_with("/reload")
        || path.ends_with("/sync")
        || path.ends_with("/failover")
        || path.starts_with("/api/v1/proxy/")
    {
        ActionOperation::Execute
    } else {
        ActionOperation::Create
    };
    let encoded_controller_id = path
        .strip_prefix("/api/v1/center/admin/controllers/")
        .or_else(|| path.strip_prefix("/api/v1/controllers/"))
        .and_then(|suffix| suffix.split('/').next())
        .filter(|id| !id.is_empty());
    let controller_id = encoded_controller_id
        .map(|id| {
            let decoded = percent_encoding::percent_decode_str(id)
                .decode_utf8()
                .map_err(|_| ())?;
            // API controller paths use `~` as the slash-safe canonical-ID
            // separator. Authorization must target the exact ID that the
            // downstream handler will mutate or execute against.
            ControllerId::new(decoded.replace('~', "/"))
                .map(|id| id.to_string())
                .map_err(|_| ())
        })
        .transpose()?;
    Ok(Action {
        permission: permission.to_string(),
        controller_id,
        operation: Some(operation),
        request_path: Some(path.to_string()),
        request_verb: Some(method.as_str().to_ascii_lowercase()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::to_bytes,
        http::Request,
        middleware,
        routing::{delete, get, post},
        Router,
    };
    use edgion_center_core::{AllowAllAuthorizer, CoreError, CoreResult, Decision};
    use serde_json::Value;
    use std::sync::Mutex;
    use tower::ServiceExt;

    /// A store that grants no permissions, for the deny path.
    struct EmptyAuthz;
    #[async_trait::async_trait]
    impl Authorizer for EmptyAuthz {
        async fn authorize(&self, _p: &Principal, _action: &Action) -> CoreResult<Decision> {
            Ok(Decision::deny("test policy"))
        }
    }

    struct UnavailableAuthz;
    #[async_trait::async_trait]
    impl Authorizer for UnavailableAuthz {
        async fn authorize(&self, _p: &Principal, _action: &Action) -> CoreResult<Decision> {
            Err(CoreError::Adapter("sensitive backend detail".to_string()))
        }
    }

    struct ControllerDiscoveryAuthz;

    #[async_trait::async_trait]
    impl Authorizer for ControllerDiscoveryAuthz {
        async fn authorize(&self, _p: &Principal, _action: &Action) -> CoreResult<Decision> {
            Ok(Decision::deny("not used by identity discovery"))
        }

        async fn granted_permissions(
            &self,
            _principal: &Principal,
            candidates: &[String],
        ) -> CoreResult<Option<Vec<String>>> {
            assert!(candidates.iter().any(|value| value == "controllers:read"));
            Ok(Some(vec!["controllers:read".to_string()]))
        }
    }

    /// Inject a `UnifiedAuthClaims` (simulating unified_auth) before authz runs.
    fn claims_injecting_layer_with_groups(router: Router, groups: Vec<String>) -> Router {
        router.layer(middleware::from_fn(
            move |mut req: Request<Body>, next: Next| {
                let groups = groups.clone();
                async move {
                    req.extensions_mut().insert(UnifiedAuthClaims {
                        provider: AuthProvider::Local,
                        sub: Some("tester".to_string()),
                        iss: None,
                        groups,
                        claims: serde_json::Value::Null,
                    });
                    next.run(req).await
                }
            },
        ))
    }

    fn claims_injecting_layer(router: Router) -> Router {
        claims_injecting_layer_with_groups(router, Vec::new())
    }

    fn app_with(authz: Arc<dyn Authorizer>, inner: Router) -> Router {
        // authz inner, claims-injection outer — mirrors unified_auth wrapping authz.
        let with_authz = inner.layer(middleware::from_fn_with_state(authz, authz_middleware));
        claims_injecting_layer(with_authz)
    }

    struct CapturingAuthorizer {
        principals: Mutex<Vec<Principal>>,
        actions: Mutex<Vec<Action>>,
    }

    #[async_trait::async_trait]
    impl Authorizer for CapturingAuthorizer {
        async fn authorize(&self, principal: &Principal, action: &Action) -> CoreResult<Decision> {
            self.principals.lock().unwrap().push(principal.clone());
            self.actions.lock().unwrap().push(action.clone());
            Ok(Decision::allow())
        }
    }

    #[tokio::test]
    async fn denies_without_key() {
        // A mapped route (GET /api/v1/controllers → controllers:read) with an
        // empty permission set must be rejected with 403.
        let inner = Router::new().route("/api/v1/controllers", get(|| async { "ok" }));
        let app = app_with(Arc::new(EmptyAuthz), inner);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/controllers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn authorization_backend_failure_is_generic_service_unavailable() {
        let inner = Router::new().route("/api/v1/controllers", get(|| async { "ok" }));
        let app = app_with(Arc::new(UnavailableAuthz), inner);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/controllers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert!(body.contains("authorization service unavailable"));
        assert!(!body.contains("sensitive backend detail"));
    }

    #[tokio::test]
    async fn mapped_route_authorizes_only_its_required_permission() {
        // A normal business request must not enumerate the complete catalog;
        // this keeps Kubernetes mode to one SAR per request.
        let inner = Router::new().route(
            "/api/v1/controllers",
            get(
                |axum::Extension(perms): axum::Extension<PermissionSet>| async move {
                    format!("{}", perms.materialize().len())
                },
            ),
        );
        let app = app_with(Arc::new(AllowAllAuthorizer), inner);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/controllers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), 1024).await.unwrap();
        let count: usize = String::from_utf8(body.to_vec()).unwrap().parse().unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn mapped_route_forwards_validated_groups_in_one_authorization_call() {
        let authorizer = Arc::new(CapturingAuthorizer {
            principals: Mutex::new(Vec::new()),
            actions: Mutex::new(Vec::new()),
        });
        let inner = Router::new().route("/api/v1/controllers", get(|| async { "ok" }));
        let with_authz = inner.layer(middleware::from_fn_with_state(
            authorizer.clone() as Arc<dyn Authorizer>,
            authz_middleware,
        ));
        let app = claims_injecting_layer_with_groups(
            with_authz,
            vec!["platform-admins".to_string(), "developers".to_string()],
        );
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/controllers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let principals = authorizer.principals.lock().unwrap();
        assert_eq!(principals.len(), 1);
        assert_eq!(
            principals[0].groups,
            vec!["platform-admins".to_string(), "developers".to_string()]
        );
    }

    #[tokio::test]
    async fn route_authorization_uses_same_tilde_decoded_id_as_reload_and_delete_handlers() {
        for (method, path) in [
            (
                axum::http::Method::POST,
                "/api/v1/controllers/cluster-a~controller-0/reload",
            ),
            (
                axum::http::Method::DELETE,
                "/api/v1/center/admin/controllers/cluster-a~controller-0",
            ),
        ] {
            let authorizer = Arc::new(CapturingAuthorizer {
                principals: Mutex::new(Vec::new()),
                actions: Mutex::new(Vec::new()),
            });
            let inner = Router::new()
                .route("/api/v1/controllers/{id}/reload", post(|| async { "ok" }))
                .route(
                    "/api/v1/center/admin/controllers/{id}",
                    delete(|| async { "ok" }),
                );
            let with_authz = inner.layer(middleware::from_fn_with_state(
                authorizer.clone() as Arc<dyn Authorizer>,
                authz_middleware,
            ));
            let response = claims_injecting_layer(with_authz)
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let actions = authorizer.actions.lock().unwrap();
            assert_eq!(actions.len(), 1);
            assert_eq!(
                actions[0].controller_id.as_deref(),
                Some("cluster-a/controller-0")
            );
        }
    }

    #[tokio::test]
    async fn auth_me_uses_bulk_permission_enumeration_when_supported() {
        let inner = Router::new().route(
            "/api/v1/auth/me",
            get(
                |axum::Extension(perms): axum::Extension<PermissionSet>| async move {
                    format!("{}", perms.materialize().len())
                },
            ),
        );
        let app = app_with(Arc::new(AllowAllAuthorizer), inner);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/auth/me")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), 1024).await.unwrap();
        let count: usize = String::from_utf8(body.to_vec()).unwrap().parse().unwrap();
        assert_eq!(count, catalog::all_keys().len());
    }

    #[tokio::test]
    async fn auth_me_exposes_native_controller_permission_for_ui_gates() {
        let inner = Router::new().route(
            "/api/v1/auth/me",
            get(crate::common::auth::session::me_handler),
        );
        let app = app_with(Arc::new(ControllerDiscoveryAuthz), inner);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/auth/me")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 4096).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json["data"]["permissions"],
            serde_json::json!(["controllers:read"])
        );
    }

    #[tokio::test]
    async fn no_required_key_passes() {
        // An unmapped NON-business path (route_permission == None, and not under
        // /api/v1/) passes even with an empty set.
        let inner = Router::new().route("/auth/me", get(|| async { "me" }));
        let app = app_with(Arc::new(EmptyAuthz), inner);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/auth/me")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_route_passes_without_key() {
        // A public auth route under /api/v1/auth/ is unmapped (None) but is NOT a
        // business path → it must pass even for an empty (non-superuser) set.
        let inner = Router::new().route("/api/v1/auth/status", get(|| async { "status" }));
        let app = app_with(Arc::new(EmptyAuthz), inner);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/auth/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn unmapped_business_route_denied_for_non_superuser() {
        // A future business route nobody mapped (route_permission == None but it
        // IS a business path) must fail CLOSED for a non-superuser set.
        let inner = Router::new().route("/api/v1/center/something-new", get(|| async { "new" }));
        let app = app_with(Arc::new(EmptyAuthz), inner);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/center/something-new")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn unmapped_business_route_allowed_for_superuser() {
        // Same unmapped business route, but a superuser (AllowAll, all=true) set
        // still reaches it → allow_all mode is unchanged.
        let inner = Router::new().route("/api/v1/center/something-new", get(|| async { "new" }));
        let app = app_with(Arc::new(AllowAllAuthorizer), inner);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/center/something-new")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn forbidden_body_is_json() {
        let inner = Router::new().route("/api/v1/controllers", get(|| async { "ok" }));
        let app = app_with(Arc::new(EmptyAuthz), inner);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/controllers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let body = to_bytes(resp.into_body(), 1024).await.unwrap();
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v.get("success").and_then(|x| x.as_bool()), Some(false));
        assert_eq!(
            v.get("required_permission").and_then(|x| x.as_str()),
            Some("controllers:read")
        );
    }

    #[test]
    fn controller_action_decodes_canonical_id_and_rejects_invalid_utf8() {
        let collection = action_for_request(
            "controllers:read",
            &axum::http::Method::GET,
            "/api/v1/center/admin/controllers",
        )
        .unwrap();
        assert_eq!(collection.operation, Some(ActionOperation::List));
        assert!(collection.controller_id.is_none());

        let action = action_for_request(
            "controllers:write",
            &axum::http::Method::DELETE,
            "/api/v1/center/admin/controllers/cluster-a%2Fcontroller-0",
        )
        .unwrap();
        assert_eq!(
            action.controller_id.as_deref(),
            Some("cluster-a/controller-0")
        );
        assert!(action_for_request(
            "controllers:write",
            &axum::http::Method::DELETE,
            "/api/v1/center/admin/controllers/%FF",
        )
        .is_err());
    }
}
