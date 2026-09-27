use super::*;

#[test]
fn logout_path_is_optional_and_restricted_to_same_origin() {
    for path in [
        None,
        Some("/oauth2/sign_out"),
        Some("/session/end?return=%2F"),
    ] {
        let config = AdminAuthConfig {
            discovery: "https://idp.example.test/.well-known/openid-configuration".into(),
            logout_path: path.map(str::to_owned),
            ..Default::default()
        };
        assert!(config.validate().is_none(), "{path:?}");
    }
    for path in [
        "",
        "logout",
        "https://other.example/logout",
        "//other.example/logout",
        "/\\other.example",
        "/logout\n",
        "/logout\t",
        "javascript:alert(1)",
    ] {
        let config = AdminAuthConfig {
            discovery: "https://idp.example.test/.well-known/openid-configuration".into(),
            logout_path: Some(path.into()),
            ..Default::default()
        };
        assert!(config.validate().is_some(), "{path:?}");
    }
}
