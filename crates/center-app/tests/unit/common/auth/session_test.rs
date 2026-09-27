use super::*;
use crate::common::auth::AdminAuthConfig;
use axum::{body::to_bytes, Extension};

#[tokio::test]
async fn logout_uses_the_authenticated_provider_not_available_providers() {
    for (provider, configured_path, expected_path) in [
        (
            AuthProvider::Oidc,
            Some("/oauth2/sign_out"),
            Some("/oauth2/sign_out"),
        ),
        (AuthProvider::Oidc, None, None),
        (AuthProvider::Local, Some("/oauth2/sign_out"), None),
    ] {
        let config = AdminAuthConfig {
            discovery: "https://idp.example.test/.well-known/openid-configuration".into(),
            logout_path: configured_path.map(str::to_owned),
            ..Default::default()
        };
        let state = UnifiedAuthState::from_configs(Some(&config), None, true, "test").unwrap();
        let claims = UnifiedAuthClaims {
            provider: provider.clone(),
            sub: Some("test-subject".into()),
            iss: None,
            groups: vec![],
            claims: serde_json::json!({}),
        };
        let response = me_handler(State(state), Extension(claims), None)
            .await
            .into_response();
        let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            value["data"]["authProvider"],
            serde_json::to_value(provider).unwrap()
        );
        assert_eq!(value["data"]["logoutPath"].as_str(), expected_path);
        assert_eq!(value["data"]["username"], "test-subject");
        assert_eq!(value["data"]["localLogoutAvailable"], false);
    }
}

#[cfg(feature = "password-auth")]
#[tokio::test]
async fn oidc_logout_reports_a_coexisting_password_session() {
    use crate::common::local_auth::LocalAuthConfig;
    let oidc = AdminAuthConfig {
        discovery: "https://idp.example.test/.well-known/openid-configuration".into(),
        logout_path: Some("/oauth2/sign_out".into()),
        ..Default::default()
    };
    let local = LocalAuthConfig {
        enabled: true,
        username: "admin".into(),
        password: "test-password-for-local-auth".into(),
        jwt_secret: "test-signing-secret-long-enough-for-auth".into(),
        ..Default::default()
    };
    let state = UnifiedAuthState::from_configs(Some(&oidc), Some(&local), true, "test").unwrap();
    let claims = UnifiedAuthClaims {
        provider: AuthProvider::Oidc,
        sub: Some("external".into()),
        iss: None,
        groups: vec![],
        claims: serde_json::json!({}),
    };
    let response = me_handler(State(state), Extension(claims), None)
        .await
        .into_response();
    let bytes = to_bytes(response.into_body(), 4096).await.unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["data"]["authProvider"], "oidc");
    assert_eq!(value["data"]["localLogoutAvailable"], true);
    assert_eq!(value["data"]["logoutPath"], "/oauth2/sign_out");
}
