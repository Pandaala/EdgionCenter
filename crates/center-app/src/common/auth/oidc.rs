//! OIDC bearer-token validation via the 3rd-party `openidconnect` crate.
//!
//! Center is a resource server: it validates incoming bearer **access tokens**
//! (JWTs) issued by an external OIDC provider. `openidconnect` is used for the
//! parts it does well — OpenID discovery, JWKS retrieval, and JWS signature
//! verification (`JsonWebKey::verify_signature`). The crate does not validate
//! arbitrary access-token claims (it targets ID tokens tied to a client_id /
//! nonce), so the standard claim checks (`exp` / `nbf` / `aud` / `iss`) are done
//! explicitly here, preserving Center's existing resource-server semantics
//! (custom audiences, custom issuers, configurable allowed algorithms, clock
//! skew). This replaces the previous hand-rolled `jwks_provider` + jsonwebtoken
//! decode path.

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use openidconnect::core::{
    CoreJsonWebKey, CoreJsonWebKeySet, CoreJwsSigningAlgorithm, CoreProviderMetadata,
};
use openidconnect::{IssuerUrl, JsonWebKey};
use parking_lot::RwLock;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

use super::config::AdminAuthConfig;

/// Default signing algorithms accepted when the operator does not pin a list.
/// Mirrors the previous implementation: RSA (PKCS#1 v1.5) + ECDSA P-256/P-384.
const DEFAULT_ALLOWED_ALGS: &[&str] = &["RS256", "RS384", "RS512", "ES256", "ES384"];

/// Minimal JOSE header parsed from the compact JWT (no signature verification).
#[derive(Debug, Deserialize)]
struct JoseHeader {
    alg: String,
    #[serde(default)]
    kid: Option<String>,
}

/// Successfully validated OIDC token: the subject, issuer, and full claims.
#[derive(Debug)]
pub struct OidcClaims {
    pub sub: Option<String>,
    pub iss: Option<String>,
    pub claims: Value,
}

/// Cached discovery result: the provider's JWKS and its canonical issuer.
struct CachedKeys {
    jwks: CoreJsonWebKeySet,
    issuer: String,
    fetched_at: Instant,
}

/// OIDC provider state: discovery + JWKS with caching, plus the validation
/// policy (audiences / issuers / allowed algorithms / clock skew).
pub struct OidcProvider {
    /// Bare issuer URL (the `discovery` config value with any
    /// `/.well-known/openid-configuration` suffix stripped). `openidconnect`'s
    /// `discover_async` appends the well-known suffix itself.
    issuer_url: IssuerUrl,
    http_client: Client,
    cache: RwLock<Option<CachedKeys>>,
    /// Singleflight guard so concurrent validations share one network fetch.
    fetch_lock: tokio::sync::Mutex<()>,
    ttl: Duration,
    min_refresh_interval: Duration,

    pub audiences: Vec<String>,
    pub issuers: Vec<String>,
    pub allowed_algorithms: Vec<String>,
    pub clock_skew_seconds: u64,
}

impl OidcProvider {
    /// Build the provider from `AdminAuthConfig`. Returns `Err` on HTTP-client
    /// construction failure or an unparseable discovery/issuer URL.
    pub fn from_config(config: &AdminAuthConfig) -> Result<Arc<Self>, String> {
        let mut http_client = Client::builder()
            .pool_max_idle_per_host(4)
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .redirect(reqwest::redirect::Policy::none())
            .danger_accept_invalid_certs(!config.ssl_verify);
        if let Some(ca_file) = config.ca_file.as_deref() {
            let pem = std::fs::read(ca_file)
                .map_err(|e| format!("Failed to read OIDC CA file {ca_file}: {e}"))?;
            let certificates = reqwest::Certificate::from_pem_bundle(&pem)
                .map_err(|e| format!("Failed to parse OIDC CA file {ca_file}: {e}"))?;
            if certificates.is_empty() {
                return Err(format!("OIDC CA file {ca_file} contains no certificates"));
            }
            for certificate in certificates {
                http_client = http_client.add_root_certificate(certificate);
            }
        }
        let http_client = http_client
            .build()
            .map_err(|e| format!("Failed to build OIDC HTTP client: {e}"))?;

        let issuer = derive_issuer(&config.discovery);
        let issuer_url =
            IssuerUrl::new(issuer).map_err(|e| format!("Invalid OIDC issuer URL: {e}"))?;

        Ok(Arc::new(Self {
            issuer_url,
            http_client,
            cache: RwLock::new(None),
            fetch_lock: tokio::sync::Mutex::new(()),
            ttl: Duration::from_secs(config.jwks_cache_ttl),
            min_refresh_interval: Duration::from_secs(config.jwks_min_refresh_interval),
            audiences: config.audiences.clone(),
            issuers: config.issuers.clone(),
            allowed_algorithms: config.allowed_algorithms.clone(),
            clock_skew_seconds: config.clock_skew_seconds,
        }))
    }

    /// The provider's canonical issuer (from the last successful discovery), if any.
    #[allow(dead_code)]
    pub fn cached_issuer(&self) -> Option<String> {
        self.cache.read().as_ref().map(|c| c.issuer.clone())
    }

    /// Run OpenID discovery + JWKS fetch via `openidconnect` and refresh the cache.
    async fn fetch_and_cache(&self) -> Result<(), String> {
        // Singleflight: one in-flight fetch at a time. After acquiring the lock,
        // re-check whether another caller already refreshed within min_refresh.
        let _guard = self.fetch_lock.lock().await;
        if let Some(cached) = self.cache.read().as_ref() {
            if cached.fetched_at.elapsed() < self.min_refresh_interval {
                return Ok(());
            }
        }

        let metadata =
            CoreProviderMetadata::discover_async(self.issuer_url.clone(), &self.http_client)
                .await
                .map_err(|e| format!("OIDC discovery/JWKS fetch failed: {e}"))?;

        let jwks = metadata.jwks().clone();
        if jwks.keys().is_empty() {
            return Err("OIDC JWKS contains no keys".to_string());
        }
        let issuer = metadata.issuer().as_str().to_string();

        *self.cache.write() = Some(CachedKeys {
            jwks,
            issuer,
            fetched_at: Instant::now(),
        });
        Ok(())
    }

    /// Return cached keys+issuer, refreshing when the cache is empty, expired, or
    /// `force` is set. On refresh failure, fall back to a still-present cached set
    /// (stale-while-revalidate) so a transient IdP outage does not reject all
    /// traffic; only a cold cache propagates the error (fail-close).
    async fn keys(&self, force: bool) -> Result<(CoreJsonWebKeySet, String), String> {
        let needs_refresh = force
            || match self.cache.read().as_ref() {
                None => true,
                Some(c) => c.fetched_at.elapsed() >= self.ttl,
            };

        if needs_refresh {
            if let Err(e) = self.fetch_and_cache().await {
                // Serve a stale cache if we have one; otherwise propagate.
                if self.cache.read().is_none() {
                    return Err(e);
                }
                tracing::warn!(component = "oidc", error = %e, "OIDC refresh failed; serving cached JWKS");
            }
        }

        let guard = self.cache.read();
        let cached = guard
            .as_ref()
            .ok_or_else(|| "OIDC JWKS unavailable".to_string())?;
        Ok((cached.jwks.clone(), cached.issuer.clone()))
    }

    /// Validate a bearer JWT: algorithm policy → signature (openidconnect) →
    /// `exp`/`nbf`/`aud`/`iss` claim checks. Returns the subject, issuer, and
    /// full claims on success.
    pub async fn validate(&self, token: &str) -> Result<OidcClaims, String> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return Err("malformed JWT: expected 3 segments".to_string());
        }
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;

        let header_bytes = b64
            .decode(parts[0])
            .map_err(|e| format!("Invalid token header: {e}"))?;
        let header: JoseHeader = serde_json::from_slice(&header_bytes)
            .map_err(|e| format!("Invalid token header: {e}"))?;

        // Algorithm policy: reject `none`, then enforce the allow-list.
        if header.alg.eq_ignore_ascii_case("none") {
            return Err("alg=none is not allowed".to_string());
        }
        let allowed: Vec<&str> = if self.allowed_algorithms.is_empty() {
            DEFAULT_ALLOWED_ALGS.to_vec()
        } else {
            self.allowed_algorithms.iter().map(String::as_str).collect()
        };
        if !allowed.iter().any(|a| a.eq_ignore_ascii_case(&header.alg)) {
            return Err(format!("algorithm {} not allowed", header.alg));
        }
        let alg: CoreJwsSigningAlgorithm =
            serde_json::from_value(Value::String(header.alg.clone()))
                .map_err(|_| format!("unsupported alg: {}", header.alg))?;

        // Fetch keys (cached), select by kid, verify signature. On a kid miss
        // with a cached set, force one refresh to pick up key rotation.
        let (jwks, discovery_issuer) = self.keys(false).await?;
        let signing_input = format!("{}.{}", parts[0], parts[1]);
        let signature = b64
            .decode(parts[2])
            .map_err(|e| format!("Invalid signature encoding: {e}"))?;

        let verified = verify_with_jwks(
            &jwks,
            &alg,
            header.kid.as_deref(),
            signing_input.as_bytes(),
            &signature,
        );
        let discovery_issuer = match verified {
            Ok(()) => discovery_issuer,
            Err(VerifyError::KidNotFound) => {
                // Possible key rotation — refresh once and retry.
                let (jwks2, issuer2) = self.keys(true).await?;
                verify_with_jwks(
                    &jwks2,
                    &alg,
                    header.kid.as_deref(),
                    signing_input.as_bytes(),
                    &signature,
                )
                .map_err(|e| e.message())?;
                issuer2
            }
            Err(e) => return Err(e.message()),
        };

        // Signature verified. Decode and validate claims.
        let payload_bytes = b64
            .decode(parts[1])
            .map_err(|e| format!("Invalid token payload: {e}"))?;
        let claims: Value = serde_json::from_slice(&payload_bytes)
            .map_err(|e| format!("Invalid token payload: {e}"))?;

        self.validate_claims(&claims, &discovery_issuer)?;

        let sub = claims
            .get("sub")
            .and_then(Value::as_str)
            .map(str::to_string);
        let iss = claims
            .get("iss")
            .and_then(Value::as_str)
            .map(str::to_string);
        Ok(OidcClaims { sub, iss, claims })
    }

    /// Validate the time-based and identity claims (`exp`, `nbf`, `aud`, `iss`)
    /// against policy + clock skew. `discovery_issuer` is used when no explicit
    /// `issuers` allow-list is configured.
    fn validate_claims(&self, claims: &Value, discovery_issuer: &str) -> Result<(), String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "system clock before unix epoch".to_string())?
            .as_secs() as i64;
        let leeway = self.clock_skew_seconds as i64;

        // exp (required): now must be <= exp + leeway.
        match claims.get("exp").and_then(Value::as_i64) {
            Some(exp) => {
                if now > exp + leeway {
                    return Err("token expired".to_string());
                }
            }
            None => return Err("token missing exp".to_string()),
        }

        // nbf (optional): now must be >= nbf - leeway.
        if let Some(nbf) = claims.get("nbf").and_then(Value::as_i64) {
            if now < nbf - leeway {
                return Err("token not yet valid (nbf)".to_string());
            }
        }

        // aud: when audiences are configured, the token's aud (string or array)
        // must intersect the allow-list.
        if !self.audiences.is_empty() {
            let token_auds = extract_audiences(claims);
            let ok = token_auds
                .iter()
                .any(|a| self.audiences.iter().any(|x| x == a));
            if !ok {
                return Err("token audience not allowed".to_string());
            }
        }

        // iss: against the configured allow-list if present, otherwise against
        // the discovery document's issuer.
        let token_iss = claims.get("iss").and_then(Value::as_str);
        if !self.issuers.is_empty() {
            match token_iss {
                Some(iss) if self.issuers.iter().any(|x| x == iss) => {}
                _ => return Err("token issuer not allowed".to_string()),
            }
        } else if !discovery_issuer.is_empty() {
            match token_iss {
                Some(iss) if iss == discovery_issuer => {}
                _ => return Err("token issuer does not match provider".to_string()),
            }
        }

        Ok(())
    }
}

/// Errors from JWKS key selection + signature verification.
enum VerifyError {
    KidNotFound,
    Message(String),
}

impl VerifyError {
    fn message(self) -> String {
        match self {
            VerifyError::KidNotFound => "Unknown signing key".to_string(),
            VerifyError::Message(m) => m,
        }
    }
}

/// Select the JWK by `kid` (any key when the token carries no `kid`) and verify
/// the JWS signature via `openidconnect`.
fn verify_with_jwks(
    jwks: &CoreJsonWebKeySet,
    alg: &CoreJwsSigningAlgorithm,
    kid: Option<&str>,
    message: &[u8],
    signature: &[u8],
) -> Result<(), VerifyError> {
    let candidates: Vec<&CoreJsonWebKey> = jwks
        .keys()
        .iter()
        .filter(|k| match (k.key_id(), kid) {
            (Some(k_kid), Some(jwt_kid)) => k_kid.as_str() == jwt_kid,
            (_, None) => true,
            _ => false,
        })
        .collect();

    if candidates.is_empty() {
        return Err(VerifyError::KidNotFound);
    }

    // Accept if any candidate key verifies the signature.
    let mut last_err: Option<String> = None;
    for key in candidates {
        match key.verify_signature(alg, message, signature) {
            Ok(()) => return Ok(()),
            Err(e) => last_err = Some(format!("signature verification failed: {e}")),
        }
    }
    Err(VerifyError::Message(last_err.unwrap_or_else(|| {
        "signature verification failed".to_string()
    })))
}

/// Extract the `aud` claim as a list (it may be a string or an array of strings).
fn extract_audiences(claims: &Value) -> Vec<String> {
    match claims.get("aud") {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

/// Derive the bare issuer URL from the configured discovery URL by stripping a
/// trailing `/.well-known/openid-configuration` (with or without a trailing
/// slash). `openidconnect`'s `discover_async` re-appends the well-known suffix.
fn derive_issuer(discovery: &str) -> String {
    let trimmed = discovery.trim_end_matches('/');
    const SUFFIX: &str = "/.well-known/openid-configuration";
    if let Some(stripped) = trimmed.strip_suffix(SUFFIX) {
        stripped.to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_issuer_strips_well_known() {
        assert_eq!(
            derive_issuer("https://idp.example.com/.well-known/openid-configuration"),
            "https://idp.example.com"
        );
        assert_eq!(
            derive_issuer("https://idp.example.com/realms/x/.well-known/openid-configuration/"),
            "https://idp.example.com/realms/x"
        );
        assert_eq!(
            derive_issuer("https://idp.example.com/"),
            "https://idp.example.com"
        );
    }

    #[test]
    fn extract_audiences_handles_string_and_array() {
        assert_eq!(
            extract_audiences(&serde_json::json!({"aud": "a"})),
            vec!["a".to_string()]
        );
        assert_eq!(
            extract_audiences(&serde_json::json!({"aud": ["a", "b"]})),
            vec!["a".to_string(), "b".to_string()]
        );
        assert!(extract_audiences(&serde_json::json!({})).is_empty());
    }

    // ── End-to-end validation against a mock OIDC provider ──────────────────
    //
    // A fixed test RSA key signs tokens (jsonwebtoken); the matching public JWK
    // is published by a wiremock IdP. Verification goes through `openidconnect`
    // (discovery + JWKS + signature), and the claim checks run on top.

    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const TEST_RSA_PEM: &str = "-----BEGIN PRIVATE KEY-----\n\
MIIEvAIBADANBgkqhkiG9w0BAQEFAASCBKYwggSiAgEAAoIBAQC8jZiYLwE6tV6P\n\
o7Y6HcBh4oaAbRQOF4rZMYu4CZkUk7ZOC3BYxftlGqjvXikAzhA6U6kCCEwTEZFa\n\
W6XbaU7eHkrSPDarcYTj/y0hTRzYejpiz7xvfn16zGX00d52zRlcPqXzAklvn4W3\n\
GXz2bTkY1cZ7aC2GMmOhQhW8mxiKTsj7DMr3dnrb3UpPdP5hmC4zG59PTpJQCAWl\n\
eVItTGXkJRHMjRWXmixp1ku+ARCYQKJT0UgsvbGG6mpzyk14I4DcH63dj2zuqmMC\n\
VRSMLYrSwfJiUR58xCkB2lzMay/AgLR6TR+zBzNHPl3LVMvaZMBqqt7Dw7xnwmDL\n\
+Cs3U7mBAgMBAAECggEAdaR7AujBAZpL558jgVsyv2AQv2xPSQOTVDQ/kpIaeuY2\n\
prcoX5sXYVui78Z2GtR2941fM69tl7AsWo44C4/G73tC/60mBw4K9h4uUErGpqKM\n\
bz5hucbYD5gcPQX8oW8SVaWY8OgKXaTQTw+OEkrPhxlKf5DeZo5l7yVGXqj+RLwU\n\
Be3CciIl/UKzKLUfJAbaVbHE3F5cFN6oczB9uWAQLFczqCjRAG2czhjTSMPTD2mu\n\
wz14IjfjXXd/752mBwlolkNB0jfnaVUJXHqKJGc5ZPfLuOzr/WNORd+8GBPGK2do\n\
5UJevsY37sxiU5ZpxpdhLloG9OllRe8nTpM0Xsbt4QKBgQDwyq0SfVBRYdO9f2DC\n\
6QGRkTT0YhHUWOzoLuewIa6ymDt3why7BjVP6lS6U126WXAue/Zq8MM3ffyrVo5L\n\
nMxnahSFCxmd/lsop0+5Ep8sCqs3Vbyx7GAY/aI5bQbisw2yikIgSFH1sbOqm7uu\n\
pkeXSaiYu4dRqM/9goEQWglHNQKBgQDIdkhAz5D7Nr40x0/yo6/gtTVZQuN2L1W7\n\
ZYJDdaYHqcbeDGoPjpXZjq8zK6k15RTORFbBPQezr/vZcJEhRMlVoBkoaYUjCYZX\n\
wkKu6Vnk8nqSbNpQE26FIl7qyiM2lO70fEmHkT4oWqBq4zaDFXq1nudrY+C/JPGH\n\
U0Qsq4KWnQKBgFWopB0Zu0LYPE0DTVbJMSepsl7lrFYQNGb8mKtNsCoUgcM+qJ3X\n\
vYtqXy3RjlxGiOPgcW7lq2zIQuRo7EH1y7lWQWp64mgUHjW+H1xFRZ6TRQlwVKou\n\
3pjFUbqAEJ0A+XR0PsXhNFblGncs431j5b/qEjITNDZWiXczv9ojTX2pAoGAB2xx\n\
8ox9RwBY/OVgrZCoQ78SMbMLb2YDW8Q/lbX2pxP/fFujVd4m6H6jOFbmlktcgOMA\n\
/3j+HwZmYkAL79p3Rkd+hwOZXZnNstRL2eRkYtkj9uY3E34UurNyJmnD8hKD4uPz\n\
aSTU03O/uxWdAC+8cptm4JA7U3jPxP4taSYU2PUCgYAqC1S/7mkSUILxPL5zK461\n\
X3qCP7WUvqPWv3gKqWjl1cq7pwYvV3zGvd7+qVmgCBYWeqhXyBIXxRZPYa2Pj2bv\n\
LsfuJHCR2szBdUVt2vTO/5Y6M2QbmxpC06/p2X9DYrqPvQ3FOZRLIhuEwZu8Mt2u\n\
cfZ1OqI5Xue6arSxa8eoEA==\n\
-----END PRIVATE KEY-----\n";

    const TEST_KID: &str = "test-key-1";

    /// Build the public JWK JSON for the embedded test key.
    fn test_jwk() -> Value {
        use rsa::pkcs8::DecodePrivateKey;
        use rsa::traits::PublicKeyParts;
        let key = rsa::RsaPrivateKey::from_pkcs8_pem(TEST_RSA_PEM).expect("parse test key");
        let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let n = b64.encode(key.n().to_bytes_be());
        let e = b64.encode(key.e().to_bytes_be());
        json!({ "kty": "RSA", "use": "sig", "alg": "RS256", "kid": TEST_KID, "n": n, "e": e })
    }

    /// Sign a JWT (RS256) over `claims` with the test key and the given kid.
    fn sign(claims: &Value, kid: &str) -> String {
        use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some(kid.to_string());
        let key = EncodingKey::from_rsa_pem(TEST_RSA_PEM.as_bytes()).expect("encoding key");
        encode(&header, claims, &key).expect("sign")
    }

    /// Spin up a mock OIDC IdP serving discovery + JWKS. Returns the server (keep
    /// it alive) and the bare issuer URL.
    async fn mock_idp() -> (MockServer, String) {
        let server = MockServer::start().await;
        let issuer = server.uri();
        let discovery = json!({
            "issuer": issuer,
            "authorization_endpoint": format!("{issuer}/authorize"),
            "token_endpoint": format!("{issuer}/token"),
            "jwks_uri": format!("{issuer}/jwks"),
            "response_types_supported": ["code"],
            "subject_types_supported": ["public"],
            "id_token_signing_alg_values_supported": ["RS256"],
        });
        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(ResponseTemplate::new(200).set_body_json(discovery))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "keys": [test_jwk()] })))
            .mount(&server)
            .await;
        (server, issuer)
    }

    fn now() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    /// Build a provider against the mock issuer with the given aud/iss policy.
    fn provider_for(
        issuer: &str,
        audiences: Vec<String>,
        issuers: Vec<String>,
    ) -> Arc<OidcProvider> {
        let config = AdminAuthConfig {
            enabled: true,
            discovery: format!("{issuer}/.well-known/openid-configuration"),
            audiences,
            issuers,
            ssl_verify: false,
            ..AdminAuthConfig::default()
        };
        OidcProvider::from_config(&config).expect("provider")
    }

    #[test]
    fn configured_ca_file_must_be_readable() {
        let config = AdminAuthConfig {
            discovery: "https://issuer.example/.well-known/openid-configuration".into(),
            ca_file: Some("/definitely/missing/oidc-ca.pem".into()),
            ..AdminAuthConfig::default()
        };
        let error = match OidcProvider::from_config(&config) {
            Ok(_) => panic!("missing CA file unexpectedly accepted"),
            Err(error) => error,
        };
        assert!(error.contains("Failed to read OIDC CA file"), "{error}");
    }

    #[tokio::test]
    async fn valid_token_passes() {
        let (server, issuer) = mock_idp().await;
        let provider = provider_for(&issuer, vec!["edgion".into()], vec![]);
        let token = sign(
            &json!({"sub": "alice", "iss": issuer, "aud": "edgion", "exp": now() + 3600}),
            TEST_KID,
        );
        let claims = provider.validate(&token).await.expect("should validate");
        assert_eq!(claims.sub.as_deref(), Some("alice"));
        assert_eq!(claims.iss.as_deref(), Some(issuer.as_str()));
        drop(server);
    }

    #[tokio::test]
    async fn expired_token_rejected() {
        let (server, issuer) = mock_idp().await;
        let provider = provider_for(&issuer, vec![], vec![]);
        // Beyond the default 120s clock-skew leeway.
        let token = sign(
            &json!({"sub": "alice", "iss": issuer, "exp": now() - 3600}),
            TEST_KID,
        );
        let err = provider.validate(&token).await.unwrap_err();
        assert!(err.contains("expired"), "got: {err}");
        drop(server);
    }

    #[tokio::test]
    async fn wrong_audience_rejected() {
        let (server, issuer) = mock_idp().await;
        let provider = provider_for(&issuer, vec!["edgion".into()], vec![]);
        let token = sign(
            &json!({"sub": "alice", "iss": issuer, "aud": "other", "exp": now() + 3600}),
            TEST_KID,
        );
        let err = provider.validate(&token).await.unwrap_err();
        assert!(err.contains("audience"), "got: {err}");
        drop(server);
    }

    #[tokio::test]
    async fn wrong_issuer_rejected() {
        let (server, issuer) = mock_idp().await;
        // Explicit issuer allow-list that does not include the token's iss.
        let provider = provider_for(&issuer, vec![], vec!["https://other.example.com".into()]);
        let token = sign(
            &json!({"sub": "alice", "iss": issuer, "exp": now() + 3600}),
            TEST_KID,
        );
        let err = provider.validate(&token).await.unwrap_err();
        assert!(err.contains("issuer"), "got: {err}");
        drop(server);
    }

    #[tokio::test]
    async fn issuer_mismatch_vs_discovery_rejected() {
        let (server, issuer) = mock_idp().await;
        let provider = provider_for(&issuer, vec![], vec![]); // no explicit allow-list → use discovery issuer
        let token = sign(
            &json!({"sub": "alice", "iss": "https://evil.example.com", "exp": now() + 3600}),
            TEST_KID,
        );
        let err = provider.validate(&token).await.unwrap_err();
        assert!(err.contains("issuer"), "got: {err}");
        drop(server);
    }

    #[tokio::test]
    async fn tampered_signature_rejected() {
        let (server, issuer) = mock_idp().await;
        let provider = provider_for(&issuer, vec![], vec![]);
        let token = sign(
            &json!({"sub": "alice", "iss": issuer, "exp": now() + 3600}),
            TEST_KID,
        );
        // Flip the last char of the signature segment.
        let mut parts: Vec<&str> = token.split('.').collect();
        let mut sig = parts[2].to_string();
        let last = sig.pop().unwrap();
        sig.push(if last == 'A' { 'B' } else { 'A' });
        let tampered = format!("{}.{}.{}", parts[0], parts[1], sig);
        let _ = &mut parts;
        let err = provider.validate(&tampered).await.unwrap_err();
        assert!(
            err.contains("signature") || err.contains("Unknown"),
            "got: {err}"
        );
        drop(server);
    }

    #[tokio::test]
    async fn algorithm_not_allowed_rejected() {
        let (server, issuer) = mock_idp().await;
        // Only allow ES256; the token is signed RS256.
        let provider = provider_for(&issuer, vec![], vec![]);
        let provider = Arc::new(OidcProvider {
            allowed_algorithms: vec!["ES256".into()],
            ..Arc::try_unwrap(provider).ok().expect("unique")
        });
        let token = sign(
            &json!({"sub": "alice", "iss": issuer, "exp": now() + 3600}),
            TEST_KID,
        );
        let err = provider.validate(&token).await.unwrap_err();
        assert!(err.contains("not allowed"), "got: {err}");
        drop(server);
    }

    #[tokio::test]
    async fn unknown_kid_rejected() {
        let (server, issuer) = mock_idp().await;
        let provider = provider_for(&issuer, vec![], vec![]);
        let token = sign(
            &json!({"sub": "alice", "iss": issuer, "exp": now() + 3600}),
            "nonexistent-kid",
        );
        let err = provider.validate(&token).await.unwrap_err();
        assert!(
            err.contains("signing key") || err.contains("Unknown"),
            "got: {err}"
        );
        drop(server);
    }
}
