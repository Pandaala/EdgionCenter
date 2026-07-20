use std::sync::Arc;

use async_trait::async_trait;
use edgion_center_core::{
    Action, ActionOperation, Authorizer, CoreError, CoreResult, Decision, Principal,
};
use k8s_openapi::api::authorization::v1::{
    NonResourceAttributes, ResourceAttributes, SubjectAccessReview, SubjectAccessReviewSpec,
};

// Kubernetes' default `system:discovery` role grants every authenticated
// principal GET access to `/api/*`. Never authorize Center HTTP routes under
// that namespace; project them into a dedicated virtual RBAC namespace first.
const CENTER_AUTHZ_PATH_PREFIX: &str = "/edgion-center-authz";
use kube::{api::PostParams, Api, Client};

use crate::controller_resource_name;

#[async_trait]
trait SarReviews: Send + Sync {
    async fn create(&self, review: &SubjectAccessReview) -> Result<SubjectAccessReview, CoreError>;
}

struct KubernetesSarReviews {
    api: Api<SubjectAccessReview>,
}

#[async_trait]
impl SarReviews for KubernetesSarReviews {
    async fn create(&self, review: &SubjectAccessReview) -> Result<SubjectAccessReview, CoreError> {
        self.api
            .create(&PostParams::default(), review)
            .await
            .map_err(|error| CoreError::Adapter(error.to_string()))
    }
}

/// Native Kubernetes authorization through SubjectAccessReview.
///
/// Each business request produces exactly one SAR. `/auth/me` discovery checks
/// only the permission candidates supplied by the application, using explicit
/// virtual non-resource URLs so UI capabilities match reviewed RBAC grants.
#[derive(Clone)]
pub struct KubernetesSarAuthorizer {
    reviews: Arc<dyn SarReviews>,
    namespace: String,
}

impl KubernetesSarAuthorizer {
    pub fn new(client: Client, namespace: impl Into<String>) -> Self {
        Self {
            reviews: Arc::new(KubernetesSarReviews {
                api: Api::all(client),
            }),
            namespace: namespace.into(),
        }
    }

    #[cfg(test)]
    fn with_reviews(reviews: Arc<dyn SarReviews>, namespace: impl Into<String>) -> Self {
        Self {
            reviews,
            namespace: namespace.into(),
        }
    }

    fn review(&self, principal: &Principal, action: &Action) -> CoreResult<SubjectAccessReview> {
        // Provider-tagged, length-delimited identities cannot collide across
        // local/OIDC providers or through delimiter characters in `iss`/`sub`.
        let (user, groups) = match (principal.provider.as_str(), principal.issuer.as_deref()) {
            ("oidc", Some(issuer)) if !issuer.is_empty() => {
                let prefix = format!("oidc:{}:{issuer}", issuer.len());
                (
                    format!("{prefix}:user:{}", principal.subject),
                    principal
                        .groups
                        .iter()
                        .map(|group| format!("{prefix}:group:{group}"))
                        .collect(),
                )
            }
            ("oidc", _) => {
                return Err(CoreError::Adapter(
                    "OIDC authorization requires a validated issuer".to_string(),
                ));
            }
            ("local", None) => (
                format!("local:user:{}", principal.subject),
                principal
                    .groups
                    .iter()
                    .map(|group| format!("local:group:{group}"))
                    .collect(),
            ),
            (provider, _) => {
                return Err(CoreError::Adapter(format!(
                    "unsupported Kubernetes principal provider {provider:?}"
                )));
            }
        };
        let (resource_attributes, non_resource_attributes) = if action
            .permission
            .starts_with("controllers:")
        {
            let operation = action.operation.ok_or_else(|| {
                CoreError::Adapter("controller authorization requires an operation".to_string())
            })?;
            let verb = match operation {
                ActionOperation::List => "list",
                ActionOperation::Get => "get",
                ActionOperation::Create => "create",
                ActionOperation::Update | ActionOperation::Execute => "update",
                ActionOperation::Delete => "delete",
            };
            (
                Some(ResourceAttributes {
                    group: Some("center.edgion.io".to_string()),
                    version: Some("v1alpha1".to_string()),
                    resource: Some("edgioncontrollers".to_string()),
                    namespace: Some(self.namespace.clone()),
                    name: action
                        .controller_id
                        .as_deref()
                        .map(controller_resource_name),
                    verb: Some(verb.to_string()),
                    ..ResourceAttributes::default()
                }),
                None,
            )
        } else {
            let request_path = action.request_path.as_deref().ok_or_else(|| {
                CoreError::Adapter("non-resource authorization requires a request path".to_string())
            })?;
            if !request_path.starts_with('/') {
                return Err(CoreError::Adapter(
                    "non-resource authorization requires an absolute request path".to_string(),
                ));
            }
            let path = format!("{CENTER_AUTHZ_PATH_PREFIX}{request_path}");
            let verb = action.request_verb.clone().ok_or_else(|| {
                CoreError::Adapter("non-resource authorization requires a request verb".to_string())
            })?;
            (
                None,
                Some(NonResourceAttributes {
                    path: Some(path),
                    verb: Some(verb),
                }),
            )
        };
        Ok(SubjectAccessReview {
            spec: SubjectAccessReviewSpec {
                user: Some(user),
                groups: Some(groups),
                resource_attributes,
                non_resource_attributes,
                ..SubjectAccessReviewSpec::default()
            },
            ..SubjectAccessReview::default()
        })
    }
}

#[async_trait]
impl Authorizer for KubernetesSarAuthorizer {
    async fn authorize(&self, principal: &Principal, action: &Action) -> CoreResult<Decision> {
        let response = self
            .reviews
            .create(&self.review(principal, action)?)
            .await?;
        let status = response.status.ok_or_else(|| {
            CoreError::Adapter("SubjectAccessReview response omitted status".to_string())
        })?;
        if let Some(error) = status.evaluation_error.filter(|error| !error.is_empty()) {
            return Err(CoreError::Adapter(format!(
                "SubjectAccessReview evaluation error: {error}"
            )));
        }
        if status.allowed {
            Ok(Decision::allow())
        } else {
            let reason = status
                .reason
                .unwrap_or_else(|| "Kubernetes authorization denied".to_string());
            Ok(Decision::deny(reason))
        }
    }

    async fn granted_permissions(
        &self,
        principal: &Principal,
        candidates: &[String],
    ) -> CoreResult<Option<Vec<String>>> {
        let mut granted = Vec::with_capacity(candidates.len());
        for permission in candidates {
            let decision = self
                .authorize(
                    principal,
                    &Action {
                        permission: "<permission-discovery>".to_string(),
                        controller_id: None,
                        operation: Some(ActionOperation::Get),
                        request_path: Some(format!("/permissions/{permission}")),
                        request_verb: Some("get".to_string()),
                    },
                )
                .await?;
            if decision.allowed {
                granted.push(permission.clone());
            }
        }
        Ok(Some(granted))
    }
}

#[cfg(test)]
mod tests {
    use std::{pin::pin, sync::Mutex};

    use http::{Request, Response};
    use k8s_openapi::api::authorization::v1::SubjectAccessReviewStatus;
    use kube::client::Body;
    use tower_test::mock;

    use super::*;

    struct FakeReviews {
        response: Result<SubjectAccessReviewStatus, String>,
        requests: Mutex<Vec<SubjectAccessReview>>,
    }

    struct MissingStatusReviews;

    #[async_trait]
    impl SarReviews for MissingStatusReviews {
        async fn create(
            &self,
            review: &SubjectAccessReview,
        ) -> Result<SubjectAccessReview, CoreError> {
            Ok(review.clone())
        }
    }

    #[async_trait]
    impl SarReviews for FakeReviews {
        async fn create(
            &self,
            review: &SubjectAccessReview,
        ) -> Result<SubjectAccessReview, CoreError> {
            self.requests.lock().unwrap().push(review.clone());
            match &self.response {
                Ok(status) => Ok(SubjectAccessReview {
                    status: Some(status.clone()),
                    ..review.clone()
                }),
                Err(error) => Err(CoreError::Adapter(error.clone())),
            }
        }
    }

    fn principal() -> Principal {
        Principal {
            subject: "alice".to_string(),
            provider: "oidc".to_string(),
            issuer: Some("https://issuer.example".to_string()),
            groups: vec!["platform-admins".to_string(), "developers".to_string()],
        }
    }

    fn fake(status: SubjectAccessReviewStatus) -> (Arc<FakeReviews>, KubernetesSarAuthorizer) {
        let reviews = Arc::new(FakeReviews {
            response: Ok(status),
            requests: Mutex::new(Vec::new()),
        });
        let authorizer = KubernetesSarAuthorizer::with_reviews(reviews.clone(), "management");
        (reviews, authorizer)
    }

    #[tokio::test]
    async fn controller_delete_maps_to_namespaced_resource_sar_with_oidc_identity() {
        let (reviews, authorizer) = fake(SubjectAccessReviewStatus {
            allowed: true,
            ..SubjectAccessReviewStatus::default()
        });
        let decision = authorizer
            .authorize(
                &principal(),
                &Action {
                    permission: "controllers:write".to_string(),
                    controller_id: Some("cluster-a/controller-0".to_string()),
                    operation: Some(ActionOperation::Delete),
                    request_path: Some(
                        "/api/v1/center/admin/controllers/cluster-a%2Fcontroller-0".to_string(),
                    ),
                    request_verb: Some("delete".to_string()),
                },
            )
            .await
            .unwrap();
        assert!(decision.allowed);
        let request = reviews.requests.lock().unwrap().pop().unwrap();
        assert_eq!(
            request.spec.user.as_deref(),
            Some("oidc:22:https://issuer.example:user:alice")
        );
        assert_eq!(
            request.spec.groups.as_deref(),
            Some(
                [
                    "oidc:22:https://issuer.example:group:platform-admins".to_string(),
                    "oidc:22:https://issuer.example:group:developers".to_string(),
                ]
                .as_slice()
            )
        );
        let attributes = request.spec.resource_attributes.unwrap();
        assert_eq!(attributes.verb.as_deref(), Some("delete"));
        assert_eq!(attributes.namespace.as_deref(), Some("management"));
        assert_eq!(
            attributes.name.as_deref(),
            Some(controller_resource_name("cluster-a/controller-0").as_str())
        );
        assert!(request.spec.non_resource_attributes.is_none());
    }

    #[tokio::test]
    async fn controller_history_collection_maps_to_list_resource_sar() {
        let (reviews, authorizer) = fake(SubjectAccessReviewStatus {
            allowed: true,
            ..SubjectAccessReviewStatus::default()
        });
        let decision = authorizer
            .authorize(
                &principal(),
                &Action {
                    permission: "controllers:read".to_string(),
                    controller_id: None,
                    operation: Some(ActionOperation::List),
                    request_path: Some("/api/v1/center/admin/controllers".to_string()),
                    request_verb: Some("get".to_string()),
                },
            )
            .await
            .unwrap();
        assert!(decision.allowed);
        let attributes = reviews
            .requests
            .lock()
            .unwrap()
            .pop()
            .unwrap()
            .spec
            .resource_attributes
            .unwrap();
        assert_eq!(attributes.verb.as_deref(), Some("list"));
        assert!(attributes.name.is_none());
        assert_eq!(attributes.resource.as_deref(), Some("edgioncontrollers"));
    }

    #[tokio::test]
    async fn non_controller_action_uses_exact_non_resource_path_and_verb() {
        let (reviews, authorizer) = fake(SubjectAccessReviewStatus {
            allowed: false,
            denied: Some(true),
            reason: Some("RBAC deny".to_string()),
            ..SubjectAccessReviewStatus::default()
        });
        let decision = authorizer
            .authorize(
                &principal(),
                &Action {
                    permission: "region_routes:write".to_string(),
                    controller_id: None,
                    operation: Some(ActionOperation::Execute),
                    request_path: Some("/api/v1/center/cluster-region-routes/sync".to_string()),
                    request_verb: Some("post".to_string()),
                },
            )
            .await
            .unwrap();
        assert!(!decision.allowed);
        assert_eq!(decision.reason.as_deref(), Some("RBAC deny"));
        let attributes = reviews
            .requests
            .lock()
            .unwrap()
            .pop()
            .unwrap()
            .spec
            .non_resource_attributes
            .unwrap();
        assert_eq!(
            attributes.path.as_deref(),
            Some("/edgion-center-authz/api/v1/center/cluster-region-routes/sync")
        );
        assert_eq!(attributes.verb.as_deref(), Some("post"));
    }

    #[tokio::test]
    async fn provider_capability_read_uses_an_independent_non_resource_path() {
        let (reviews, authorizer) = fake(SubjectAccessReviewStatus {
            allowed: true,
            ..SubjectAccessReviewStatus::default()
        });
        let decision = authorizer
            .authorize(
                &principal(),
                &Action {
                    permission: "provider-capabilities:read".to_string(),
                    controller_id: None,
                    operation: Some(ActionOperation::Get),
                    request_path: Some(
                        "/api/v1/center/cloud/provider-capabilities/accounts/cf-main".to_string(),
                    ),
                    request_verb: Some("get".to_string()),
                },
            )
            .await
            .unwrap();
        assert!(decision.allowed);
        let attributes = reviews
            .requests
            .lock()
            .unwrap()
            .pop()
            .unwrap()
            .spec
            .non_resource_attributes
            .unwrap();
        assert_eq!(
            attributes.path.as_deref(),
            Some("/edgion-center-authz/api/v1/center/cloud/provider-capabilities/accounts/cf-main")
        );
        assert_eq!(attributes.verb.as_deref(), Some("get"));
        assert!(!attributes.path.unwrap().contains("provider-accounts"));
    }

    #[tokio::test]
    async fn api_errors_and_missing_status_fail_closed() {
        let reviews = Arc::new(FakeReviews {
            response: Err("apiserver unavailable".to_string()),
            requests: Mutex::new(Vec::new()),
        });
        let authorizer = KubernetesSarAuthorizer::with_reviews(reviews, "management");
        let action = Action {
            permission: "controllers:read".to_string(),
            controller_id: None,
            operation: Some(ActionOperation::List),
            request_path: Some("/api/v1/controllers".to_string()),
            request_verb: Some("get".to_string()),
        };
        assert!(authorizer.authorize(&principal(), &action).await.is_err());
        let missing =
            KubernetesSarAuthorizer::with_reviews(Arc::new(MissingStatusReviews), "management");
        assert!(missing.authorize(&principal(), &action).await.is_err());
        assert!(authorizer
            .granted_permissions(&principal(), &["controllers:read".to_string()])
            .await
            .is_err());
    }

    #[tokio::test]
    async fn permission_discovery_checks_each_requested_capability_in_the_virtual_namespace() {
        let (reviews, authorizer) = fake(SubjectAccessReviewStatus {
            allowed: true,
            ..SubjectAccessReviewStatus::default()
        });
        let granted = authorizer
            .granted_permissions(
                &principal(),
                &[
                    "controllers:read".to_string(),
                    "controllers:write".to_string(),
                    "users:manage".to_string(),
                    "audit:read".to_string(),
                ],
            )
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            granted,
            vec![
                "controllers:read".to_string(),
                "controllers:write".to_string(),
                "users:manage".to_string(),
                "audit:read".to_string(),
            ]
        );

        let requests = reviews.requests.lock().unwrap();
        assert_eq!(
            requests.len(),
            4,
            "discovery must stay bounded to candidates"
        );
        let paths = requests
            .iter()
            .map(|request| {
                request
                    .spec
                    .non_resource_attributes
                    .as_ref()
                    .and_then(|attributes| attributes.path.clone())
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                "/edgion-center-authz/permissions/controllers:read",
                "/edgion-center-authz/permissions/controllers:write",
                "/edgion-center-authz/permissions/users:manage",
                "/edgion-center-authz/permissions/audit:read",
            ]
        );
    }

    #[tokio::test]
    async fn evaluation_errors_are_adapter_failures_even_when_allowed_is_true() {
        let (_, authorizer) = fake(SubjectAccessReviewStatus {
            allowed: true,
            evaluation_error: Some("webhook unavailable".to_string()),
            ..SubjectAccessReviewStatus::default()
        });
        let result = authorizer
            .authorize(
                &principal(),
                &Action {
                    permission: "controllers:read".to_string(),
                    controller_id: None,
                    operation: Some(ActionOperation::List),
                    request_path: Some("/api/v1/controllers".to_string()),
                    request_verb: Some("get".to_string()),
                },
            )
            .await;
        assert!(
            matches!(result, Err(CoreError::Adapter(message)) if message.contains("webhook unavailable"))
        );
    }

    #[test]
    fn oidc_identity_is_provider_scoped_and_requires_issuer() {
        let (_, authorizer) = fake(SubjectAccessReviewStatus {
            allowed: true,
            ..SubjectAccessReviewStatus::default()
        });
        let action = Action {
            permission: "controllers:read".to_string(),
            controller_id: None,
            operation: Some(ActionOperation::List),
            request_path: Some("/api/v1/controllers".to_string()),
            request_verb: Some("get".to_string()),
        };
        let first = Principal {
            issuer: Some("a:b".to_string()),
            subject: "c:d".to_string(),
            ..principal()
        };
        let second = Principal {
            issuer: Some("a".to_string()),
            subject: "b:c:d".to_string(),
            ..principal()
        };
        assert_ne!(
            authorizer.review(&first, &action).unwrap().spec.user,
            authorizer.review(&second, &action).unwrap().spec.user
        );
        let missing_issuer = Principal {
            issuer: None,
            ..principal()
        };
        assert!(authorizer.review(&missing_issuer, &action).is_err());
    }

    #[test]
    fn oidc_groups_are_issuer_scoped_and_never_forward_reserved_names_raw() {
        let (_, authorizer) = fake(SubjectAccessReviewStatus {
            allowed: true,
            ..SubjectAccessReviewStatus::default()
        });
        let principal = Principal {
            groups: vec!["system:masters".to_string()],
            ..principal()
        };
        let review = authorizer
            .review(
                &principal,
                &Action {
                    permission: "controllers:read".to_string(),
                    controller_id: None,
                    operation: Some(ActionOperation::List),
                    request_path: Some("/api/v1/controllers".to_string()),
                    request_verb: Some("get".to_string()),
                },
            )
            .unwrap();
        assert_eq!(
            review.spec.groups,
            Some(vec![
                "oidc:22:https://issuer.example:group:system:masters".to_string()
            ])
        );
        assert!(!review
            .spec
            .groups
            .unwrap()
            .iter()
            .any(|group| group.starts_with("system:")));
    }

    #[tokio::test]
    async fn real_kube_client_posts_cluster_scoped_subject_access_review() {
        let (service, handle) = mock::pair::<Request<Body>, Response<Body>>();
        let server = tokio::spawn(async move {
            let mut handle = pin!(handle);
            let (request, send) = handle.next_request().await.unwrap();
            assert_eq!(request.method(), http::Method::POST);
            assert_eq!(
                request.uri().path(),
                "/apis/authorization.k8s.io/v1/subjectaccessreviews"
            );
            let response = SubjectAccessReview {
                status: Some(SubjectAccessReviewStatus {
                    allowed: true,
                    reason: Some("allowed by test RBAC".to_string()),
                    ..SubjectAccessReviewStatus::default()
                }),
                ..SubjectAccessReview::default()
            };
            send.send_response(
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&response).unwrap()))
                    .unwrap(),
            );
        });
        let authorizer =
            KubernetesSarAuthorizer::new(Client::new(service, "management"), "management");
        let decision = authorizer
            .authorize(
                &principal(),
                &Action {
                    permission: "controllers:read".to_string(),
                    controller_id: None,
                    operation: Some(ActionOperation::List),
                    request_path: Some("/api/v1/controllers".to_string()),
                    request_verb: Some("get".to_string()),
                },
            )
            .await
            .unwrap();
        assert!(decision.allowed);
        server.await.unwrap();
    }
}
