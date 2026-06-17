use crate::aggregator::ResourceAggregator;
use crate::api::{router, ApiState};
use crate::commander::Commander;
use crate::config::CenterConfig;
use crate::fed_sync::registry::ControllerRegistry;
use crate::fed_sync::server::FederationGrpcServer;
use crate::metadata_store::CenterMetaDataStore;
use crate::proxy::{PendingProxyMap, ProxyForwarder};
use crate::watch_cache::{CenterSyncClient, CenterWatchCacheRegistry};
use crate::common::config::ConfSyncSecurityConfig;
use crate::common::fed_sync::proto::federation_sync_server::FederationSyncServer;
use anyhow::Result;
use clap::Parser;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Outcome of the federation transport decision at startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportDecision {
    /// mTLS with peer-identity binding.
    Mtls,
    /// Refuse to start.
    FailClose,
}

/// Pure transport decision. Federation requires mTLS; anything else fail-closes.
/// - `sec`: the federation gRPC security config
/// - `trust_domain`: peer_identity.trust_domain (None or empty = absent)
pub fn decide_transport(sec: &ConfSyncSecurityConfig, trust_domain: Option<&str>) -> TransportDecision {
    match sec.resolve_mtls_or_refuse() {
        Err(_) => TransportDecision::FailClose,
        Ok(_) => match trust_domain {
            Some(td) if !td.trim().is_empty() => TransportDecision::Mtls,
            _ => TransportDecision::FailClose,
        },
    }
}

#[derive(Parser, Debug)]
#[command(name = "edgion-center", version, about = "Edgion Federated Center")]
pub struct EdgionCenterCli {
    #[arg(short = 'c', long, default_value = "config/edgion-center.yaml")]
    pub config_file: String,
}

impl EdgionCenterCli {
    pub fn parse_args() -> Self {
        Self::parse()
    }

    pub async fn run(&self) -> Result<()> {
        // Install a default tracing subscriber before any tracing macro fires
        // (Center emits warnings during config parsing, before any config-driven
        // log setup could run). `set_global_default` is one-shot, so this fixes
        // Center's logging for the whole process.
        crate::common::startup::init_default_tracing();

        // Install the process-wide Prometheus recorder before anything emits
        // metrics. Idempotent, so tests and combined modes do not panic on
        // re-install.
        if let Err(e) = crate::common::observe::metrics_api::install_global_recorder(
            crate::common::observe::metrics_api::RecorderConfig {
                service: "edgion-center",
                idle_timeout_secs: 0,
                histogram_buckets: &[],
            },
        ) {
            tracing::error!(
                component = "center",
                event = "metrics_recorder_install_failed",
                error = %e,
                "Failed to install Prometheus recorder; /metrics will return 500"
            );
        }

        let content = match std::fs::read_to_string(&self.config_file) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(
                    component = "center",
                    path = %self.config_file,
                    error = %e,
                    "Config file not readable, using defaults"
                );
                String::new()
            }
        };
        let config: CenterConfig = if content.is_empty() {
            CenterConfig::default()
        } else {
            let de = serde_yaml::Deserializer::from_str(&content);
            serde_yaml::with::singleton_map_recursive::deserialize(de)
                .map_err(|e| anyhow::anyhow!("Center config parse error ({}): {}", self.config_file, e))?
        };

        let registry = ControllerRegistry::new();
        let aggregator = Arc::new(ResourceAggregator::new());
        let pending_proxies: PendingProxyMap = Arc::new(Mutex::new(HashMap::new()));

        let metadata_store = Arc::new(CenterMetaDataStore::new());
        let sync_client = Arc::new(CenterSyncClient {
            plugin_metadata: CenterWatchCacheRegistry::new(metadata_store.clone()),
        });

        let db: Option<Arc<crate::store::Store>> = if config.database.enabled {
            match crate::store::Store::connect(&config.database).await {
                Ok(d) => {
                    tracing::info!(
                        component = "center",
                        backend = ?config.database.backend,
                        "Metadata store opened"
                    );
                    Some(Arc::new(d))
                }
                Err(e) => {
                    // Silent degrade is acceptable for the default embedded SQLite,
                    // but an explicit MySQL backend that fails to connect (down
                    // server / bad URL / missing mysql_url) is an operator
                    // misconfiguration: fail startup instead of silently running
                    // without persistence.
                    if config.database.backend == crate::config::DbBackend::Mysql {
                        return Err(anyhow::anyhow!(
                            "Failed to connect to MySQL metadata store (backend = mysql): {}",
                            e
                        ));
                    }
                    tracing::error!(component = "center", error = %e, "Failed to open metadata store, running without persistence");
                    None
                }
            }
        } else {
            None
        };

        // Audit sink: spawn the background writer only when a Store exists and
        // audit is enabled. When the DB is disabled but audit is on, log a WARN
        // once and skip (audit requires persistence).
        let audit_sink: Option<crate::common::audit::AuditSink> = if config.audit.enabled {
            match db.clone() {
                Some(store) => {
                    tracing::info!(
                        component = "center",
                        log_reads = config.audit.log_reads,
                        "audit logging enabled"
                    );
                    Some(crate::common::audit::AuditSink::spawn(store))
                }
                None => {
                    tracing::warn!(
                        component = "center",
                        "audit enabled but database unavailable; audit logging disabled"
                    );
                    None
                }
            }
        } else {
            None
        };
        let audit_log_reads = config.audit.log_reads;

        // Decide federation transport (fail-close) before constructing the server.
        config
            .grpc_security
            .validate()
            .map_err(|e| anyhow::anyhow!("Invalid grpc_security config: {}", e))?;

        // Validate admin TLS paths up front (cheap, sync) before spawning listeners,
        // mirroring the grpc_security.validate()? gate above.
        if let Some(tls) = config.server.admin_tls.as_ref() {
            tls.validate()
                .map_err(|e| anyhow::anyhow!("Invalid admin_tls config: {}", e))?;
        }

        // Fail-fast: reject malformed allow_admin_ips CIDRs before spawning listeners.
        crate::common::api::ip_allowlist::validate_admin_ips(&config.server.allow_admin_ips)?;
        crate::common::api::ip_acceptor::validate_tcp_ips(&config.server.allow_tcp_ips)?;
        if !config.server.allow_tcp_ips.is_empty() && config.server.admin_tls.is_none() {
            tracing::warn!(
                component = "center",
                "allow_tcp_ips is set but admin_tls is disabled; the L4 filter has no effect"
            );
        }

        let trust_domain = config.peer_identity.as_ref().map(|p| p.trust_domain.clone());
        let transport = decide_transport(&config.grpc_security, trust_domain.as_deref());
        if transport == TransportDecision::FailClose {
            return Err(anyhow::anyhow!(
                "Federation gRPC refuses to start: federation requires mTLS. \
                 Configure grpc_security (certs + ca) and peer_identity.trust_domain."
            ));
        }

        let grpc_server = FederationGrpcServer::new(
            registry.clone(),
            aggregator.clone(),
            pending_proxies.clone(),
            config.sync.clone(),
            sync_client.clone(),
            db.clone(),
            trust_domain.clone(),
        );
        let pending_commands = grpc_server.pending_commands.clone();

        let commander = Arc::new(Commander::new(
            registry.clone(),
            pending_commands,
            config.sync.command_timeout_secs,
        ));

        let proxy = Arc::new(ProxyForwarder::new(
            registry.clone(),
            pending_proxies,
            config.sync.command_timeout_secs,
        ));

        let api_state = ApiState {
            aggregator: aggregator.clone(),
            commander,
            proxy: proxy.clone(),
            db: db.clone(),
            metadata_store: metadata_store.clone(),
            sync_client,
            registry: registry.clone(),
            db_required: config.database.enabled,
            authz_mode: config.authz.mode,
            db_auth_enabled: config.db_auth.enabled,
        };
        let http_addr: std::net::SocketAddr = config.server.http_addr.parse()?;
        let grpc_addr: std::net::SocketAddr = config.server.grpc_addr.parse()?;
        let probe_addr: std::net::SocketAddr = config.server.probe_addr.parse()?;
        let metrics_addr: std::net::SocketAddr = config.server.metrics_addr.parse()?;

        tracing::info!(component = "center", grpc_addr = %grpc_addr, http_addr = %http_addr, "Starting edgion-center");

        // Build gRPC server transport per the decision made above.
        let mut server_builder = tonic::transport::Server::builder()
            .http2_keepalive_interval(Some(Duration::from_secs(10)))
            .http2_keepalive_timeout(Some(Duration::from_secs(5)));
        match transport {
            TransportDecision::Mtls => {
                let tls_cfg = config
                    .grpc_security
                    .resolve_active_tls()
                    .expect("Mtls decision implies resolved TLS config");
                let server_tls = crate::common::grpc_tls::load_server_tls(tls_cfg).await?;
                server_builder = server_builder
                    .tls_config(server_tls)
                    .map_err(|e| anyhow::anyhow!("Failed to configure center gRPC TLS: {}", e))?;
                tracing::info!(
                    component = "center",
                    trust_domain = %trust_domain.as_deref().unwrap_or_default(),
                    "Federation gRPC mTLS enabled"
                );
            }
            TransportDecision::FailClose => unreachable!("FailClose returned earlier"),
        }

        // gRPC server
        let grpc_handle = tokio::spawn(
            server_builder
                .add_service(FederationSyncServer::new(grpc_server))
                .serve(grpc_addr),
        );

        // HTTP Admin API — unified auth supports both OIDC and local auth.
        // Center does not use the Controller's auto-generated Secret path; credentials come only from operator configuration.
        let admin_tls = config.server.admin_tls.clone();
        let allow_admin_ips = config.server.allow_admin_ips.clone();
        let allow_tcp_ips = config.server.allow_tcp_ips.clone();
        let web_dir = config.web.dir.clone();
        // Store handle for the DB-backed axes (DB-user login + RBAC + admin
        // bootstrap). Moved into the admin HTTP task below.
        let store_for_db = db.clone();
        // Startup diagnostic only — does not change cookie runtime behavior.
        if let Some(local) = config.local_auth.as_ref() {
            // Args are (admin_tls_present, cookie_secure) — keep this order.
            let admin_tls_present = admin_tls.is_some();
            if let Some(msg) = admin_tls_cookie_warning(admin_tls_present, local.cookie_secure) {
                tracing::warn!(component = "center", "{}", msg);
            }
        }

        let api_state_for_probe = api_state.clone();
        let http_handle = tokio::spawn(async move {
            // Authentication is mandatory; no-auth mode was removed. require_auth is
            // always true, so when NO authn provider is ready the middleware takes
            // the 503 fail-close branch (prevents "no configuration" from leaving
            // the admin API exposed).
            let require_auth = true;

            let base_router = router(api_state);
            // Audit layer: applied to the business router BEFORE compose wraps it
            // with unified_auth (outermost). A layer already on `base_router` runs
            // INSIDE auth, so the injected UnifiedAuthClaims are present when the
            // audit middleware reads them. Writes are off the request path (sink).
            let base_router = match audit_sink {
                Some(sink) => {
                    let audit_state = crate::common::audit::middleware::AuditLayerState {
                        sink,
                        log_reads: audit_log_reads,
                    };
                    base_router.layer(axum::middleware::from_fn_with_state(
                        audit_state,
                        crate::common::audit::middleware::audit_middleware,
                    ))
                }
                None => base_router,
            };
            // Assemble the admin app from the orthogonal access-control axes
            // (flags + validate_access gate + session secret + UnifiedAuthState/
            // local validator + AuthzStore selection + admin bootstrap + login-route
            // mounting + compose). Extracted into build_access_app so the wiring
            // (which AuthzStore, whether the /login route is mounted) is testable.
            let app = build_access_app(&config, base_router, store_for_db, require_auth).await?;
            // Dashboard UI: a public SPA fallback mounted AFTER compose_admin_routes.
            // Because it is added after compose returns its final (auth-wrapped) router,
            // the fallback is NOT covered by unified_auth — so the login page and its
            // JS/CSS load pre-auth, while every registered /api route (added before)
            // stays auth-protected. The fallback only ever receives unmatched paths.
            // Mounted only when an asset source resolves (embedded feature on, or
            // web.dir/EDGION_WEB_DIR set); otherwise Center runs in pure-API mode.
            let app = match crate::api::web::WebSource::resolve(web_dir.as_deref()) {
                Some(source) => {
                    let source: std::sync::Arc<crate::api::web::WebSource> = std::sync::Arc::new(source);
                    tracing::info!(component = "center", "dashboard UI hosting enabled");
                    app.fallback(move |uri: axum::http::Uri| {
                        let source = source.clone();
                        async move { crate::api::web::serve(source, uri).await }
                    })
                }
                None => {
                    tracing::info!(component = "center", "dashboard UI hosting disabled (pure-API mode)");
                    app
                }
            };
            // admin-api-02: outermost IP allowlist — rejects unauthorized peers before auth.
            // Not mounted when the allowlist is empty (allow-all, backward compatible).
            let app = match crate::common::api::ip_allowlist::build_admin_ip_matcher(&allow_admin_ips)? {
                Some(m) => {
                    tracing::info!(
                        component = "center",
                        entries = allow_admin_ips.len(),
                        "admin IP allowlist active"
                    );
                    app.layer(axum::middleware::from_fn_with_state(
                        m,
                        crate::common::api::ip_allowlist::ip_allowlist_middleware,
                    ))
                }
                None => {
                    tracing::info!(component = "center", "admin IP allowlist inactive (allow all)");
                    app
                }
            };
            // Shared make-service with connect-info, required by the IP allowlist
            // (ConnectInfo<SocketAddr>) and used by both HTTP and HTTPS serve branches.
            let make_service = app.into_make_service_with_connect_info::<std::net::SocketAddr>();
            match admin_tls {
                Some(tls) => {
                    let cert_path = tls.cert_path();
                    let key_path = tls.key_path();
                    let rustls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert_path, &key_path)
                        .await
                        .map_err(|e| {
                            tracing::error!(component = "center", error = %e, "Failed to load admin TLS cert/key");
                            anyhow::anyhow!("admin TLS load failed: {}", e)
                        })?;
                    // Logged before serve() binds the socket; a bind failure (e.g. address
                    // in use) is reported by the error returned from serve() below.
                    tracing::info!(component = "center", addr = %http_addr, "Center admin API HTTPS enabled");
                    // admin-api-04: L4 pre-filter — when allow_tcp_ips is set, reject
                    // unauthorized peers before the rustls handshake. When empty, serve via
                    // the original bind_rustls path (zero overhead, behavior unchanged).
                    match crate::common::api::ip_acceptor::build_tcp_ip_matcher(&allow_tcp_ips)? {
                        Some(m) => {
                            tracing::info!(
                                component = "center",
                                entries = allow_tcp_ips.len(),
                                "L4 TCP IP pre-filter active (HTTPS)"
                            );
                            let acceptor = axum_server::tls_rustls::RustlsAcceptor::new(rustls_config)
                                .acceptor(crate::common::api::ip_acceptor::TcpIpAcceptor::new(m));
                            axum_server::bind(http_addr)
                                .acceptor(acceptor)
                                .serve(make_service)
                                .await
                                .map_err(|e| anyhow::anyhow!("admin HTTPS server error: {}", e))?;
                        }
                        None => {
                            axum_server::bind_rustls(http_addr, rustls_config)
                                .serve(make_service)
                                .await
                                .map_err(|e| anyhow::anyhow!("admin HTTPS server error: {}", e))?;
                        }
                    }
                }
                None => {
                    let listener = tokio::net::TcpListener::bind(http_addr)
                        .await
                        .map_err(|e| anyhow::anyhow!("admin HTTP bind error: {}", e))?;
                    tracing::info!(component = "center", addr = %http_addr, "Center admin API HTTP enabled (no TLS)");
                    axum::serve(listener, make_service)
                        .await
                        .map_err(|e| anyhow::anyhow!("admin HTTP server error: {}", e))?;
                }
            }
            Ok::<(), anyhow::Error>(())
        });

        // Dedicated probe listener (liveness/readiness) — own socket, no auth.
        let _probe_handle = tokio::spawn(async move {
            let app = crate::api::create_probe_router(api_state_for_probe);
            match tokio::net::TcpListener::bind(probe_addr).await {
                Ok(listener) => {
                    let _ = axum::serve(listener, app).await;
                }
                Err(e) => {
                    tracing::error!(component = "center", error = %e, addr = %probe_addr, "Center probe listener failed to bind")
                }
            }
        });

        // Dedicated metrics listener (Prometheus scrape) — own socket, no auth.
        let _metrics_handle = tokio::spawn(async move {
            let app = crate::api::create_metrics_router();
            match tokio::net::TcpListener::bind(metrics_addr).await {
                Ok(listener) => {
                    let _ = axum::serve(listener, app).await;
                }
                Err(e) => {
                    tracing::error!(component = "center", error = %e, addr = %metrics_addr, "Center metrics listener failed to bind")
                }
            }
        });

        // Wait for any task to exit or Ctrl-C
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                tracing::info!(component = "center", "Ctrl-C received, shutting down");
                Ok(())
            }
            r = grpc_handle => {
                tracing::error!(component = "center", "gRPC server exited: {:?}", r);
                Err(anyhow::anyhow!("federation gRPC server exited unexpectedly"))
            }
            r = http_handle => {
                // The admin listener may be HTTP or HTTPS depending on admin_tls.
                tracing::error!(component = "center", "admin server exited: {:?}", r);
                Err(anyhow::anyhow!("admin server exited unexpectedly"))
            }
        }
    }
}

/// Decide which admin-TLS / cookie_secure startup warning to emit, if any.
///
/// Pure (no logging) so it is unit-testable without a tracing capture layer.
/// Returns `None` when the combination is safe. Only meaningful when local auth
/// is configured (otherwise no cookie is ever issued — the caller gates on that).
fn admin_tls_cookie_warning(admin_tls_present: bool, cookie_secure: bool) -> Option<&'static str> {
    match (admin_tls_present, cookie_secure) {
        // Center itself terminates HTTPS but the auth cookie is non-Secure.
        (true, false) => Some(
            "admin_tls is enabled but local_auth.cookie_secure=false: the auth cookie is issued \
             without the Secure attribute, so browsers may transmit it back over plain HTTP. \
             Set cookie_secure=true.",
        ),
        // Secure cookie but plain-HTTP listener: a direct http:// browser reach silently
        // drops the session cookie. Fine behind a TLS-terminating proxy (warn-only).
        (false, true) => Some(
            "local_auth.cookie_secure=true but admin_tls is not configured: if Center is reached \
             directly over http:// the browser will drop the session cookie. Ensure a \
             TLS-terminating proxy fronts the admin listener.",
        ),
        _ => None,
    }
}

/// Assemble the admin HTTP app from the orthogonal access-control axes and
/// return the final composed `Router`.
///
/// Performs the flag derivation, the `validate_access` gate, session-secret
/// resolution, `UnifiedAuthState` construction (plus the local HS256 validator),
/// `AuthzStore` selection, first-run admin bootstrap, and the
/// `add_unified_auth_routes` / `compose_admin_routes` assembly. Extracted from
/// `run()` so the wiring (which `AuthzStore`, whether the `/login` route is
/// mounted) is directly testable without standing up the full server.
///
/// `base_router` is the business router (with any audit layer already applied);
/// `store` is the metadata `Store` handle used by the DB-backed axes (DB-user
/// login, RBAC, admin bootstrap). Returns `Err` from `validate_access` for an
/// invalid axis combination (fail-close, no silent fallback).
async fn build_access_app(
    config: &CenterConfig,
    base_router: axum::Router,
    store: Option<std::sync::Arc<crate::store::Store>>,
    require_auth: bool,
) -> anyhow::Result<axum::Router> {
    let auth_config = config.auth.clone();
    let local_auth_config = config.local_auth.clone();
    let db_auth_config = config.db_auth.clone();
    let authz_mode = config.authz.mode;
    let store_for_db = store;

    // ----------------------------------------------------------------
    // Orthogonal access-control axes (authn providers x authz store).
    //   oidc_on         : OIDC ([auth]) present AND enabled.
    //   single_admin_on : [local_auth] present AND enabled, supplying a
    //                     non-empty username AND password (a usable
    //                     single-admin credential). The `enabled` flag is
    //                     honored symmetrically with OIDC's `auth.enabled`:
    //                     enabled:false disables the provider even when creds
    //                     are present (default is true, so omitting it is the
    //                     common case and unaffected).
    //   db_auth_on      : DB-user login enabled.
    //   rbac            : authz.mode == rbac (install DbAuthz).
    // any_password_login = single_admin_on || db_auth_on (mounts /login).
    // ----------------------------------------------------------------
    let oidc_on = auth_config.as_ref().is_some_and(|c| c.enabled);
    let single_admin_on = local_auth_config
        .as_ref()
        .is_some_and(|c| c.enabled && !c.username.is_empty() && !c.password.is_empty());
    let db_auth_on = db_auth_config.enabled;
    let rbac = authz_mode == crate::config::AuthzMode::Rbac;
    let any_password_login = single_admin_on || db_auth_on;

    // Make `enabled: false` observable instead of silent. A provider block that
    // is present but disabled is HONORED (the provider does not load), mirroring
    // OIDC's `auth.enabled` and the single-admin `local_auth.enabled`.
    if auth_config.as_ref().is_some_and(|c| !c.enabled) {
        tracing::warn!(
            component = "center",
            "[auth] OIDC provider disabled via enabled:false; it will not load."
        );
    }
    if local_auth_config.as_ref().is_some_and(|c| !c.enabled) {
        tracing::warn!(
            component = "center",
            "[local_auth] single-admin provider disabled via enabled:false; it will not load."
        );
    }

    // Resolve the session signing secret: prefer [local_auth].jwt_secret, else
    // db_auth.jwt_secret (non-empty wins). When BOTH [local_auth].jwt_secret and
    // db_auth.jwt_secret are set, local_auth wins. The same precedence applies to
    // jwt_expiry_hours / cookie_secure when both single-admin and db_auth are on:
    // the [local_auth] admin's values are used (the db_auth fallbacks below only
    // apply on the DB-users-only path).
    let local_secret = local_auth_config
        .as_ref()
        .map(|c| c.jwt_secret.clone())
        .filter(|s| !s.is_empty());
    let db_secret = db_auth_config.jwt_secret.clone().filter(|s| !s.is_empty());
    let has_secret = local_secret.is_some() || db_secret.is_some();
    let store_present = store_for_db.is_some();

    // Fail-fast validation of the axis combination.
    validate_access(oidc_on, single_admin_on, db_auth_on, rbac, has_secret, store_present)?;

    if !oidc_on && !any_password_login {
        tracing::warn!(
            component = "center",
            "No authentication provider configured. Admin endpoints will return 503 \
             until you configure [auth] (OIDC), [local_auth] (single admin), or [db_auth]."
        );
    }

    // Assemble from the orthogonal axes, then compose. Authentication
    // (OIDC, single admin, DB users) and authorization (AllowAll vs RBAC)
    // are independent: any authn provider coexists with either authz
    // store, and DbAuthz resolves permissions by subject regardless of
    // which provider issued the token.
    //
    // compose hard-codes the assembly order: business routes + auth routes
    // + authz + unified_auth + cache-control. The returned app is final;
    // do not call .route() / .layer() afterwards -- see the function doc.

    // OIDC is installed only when enabled; no force-disable anymore. The
    // local arg is None here -- when password login is active we install a
    // local validator below via update_local.
    let oidc_cfg = if oidc_on { auth_config.as_ref() } else { None };
    let auth_state = crate::common::unified_auth::UnifiedAuthState::from_configs(
        oidc_cfg,
        None,
        require_auth,
        "center",
    )
    .expect("center auth state build failed");

    // Install the local HS256 validator whenever any password login is
    // active (single admin and/or DB users). It carries the signing
    // secret / expiry / cookie used by issue_login_response and the
    // unified_auth local-validation fallback.
    if any_password_login {
        let session_secret = local_secret
            .clone()
            .or_else(|| db_secret.clone())
            .expect("validate_access guarantees a secret when any password login is active");
        let validator_cfg = if single_admin_on {
            // Build from the real [local_auth] admin so verify_single_admin
            // works, but force the resolved signing secret (the local
            // secret may be empty while db_auth supplies one).
            crate::common::local_auth::LocalAuthConfig {
                jwt_secret: session_secret,
                ..local_auth_config
                    .clone()
                    .expect("single_admin_on implies [local_auth] present")
            }
        } else {
            // DB-users only: the validator is signing-only. Its single-admin
            // credential is never consulted (single_admin_enabled=false), but
            // give it a fresh random password as defense-in-depth so the
            // path is non-verifiable even if that gate were ever flipped
            // (an empty password would otherwise bcrypt-verify against the
            // hash of "").
            crate::common::local_auth::LocalAuthConfig {
                jwt_secret: session_secret,
                jwt_expiry_hours: db_auth_config.jwt_expiry_hours.unwrap_or(24),
                cookie_secure: db_auth_config.cookie_secure.unwrap_or(true),
                password: uuid::Uuid::new_v4().to_string(),
                ..crate::common::local_auth::LocalAuthConfig::default()
            }
        };
        auth_state.update_local(crate::common::local_auth::LocalAuthState::from_config(&validator_cfg));
    }

    // Authorization store: RBAC -> DbAuthz (validated store_present), else AllowAll.
    let authz: std::sync::Arc<dyn crate::common::authz::AuthzStore> = if rbac {
        let store = store_for_db
            .clone()
            .expect("validate_access guarantees a store when rbac is on");
        std::sync::Arc::new(crate::common::authz::db_authz::DbAuthz::new(store))
    } else {
        std::sync::Arc::new(crate::common::authz::allow_all::AllowAllAuthz)
    };

    // First-run admin bootstrap (creates a superuser-by-keys + admin role
    // when the users table is empty and EDGION_ADMIN_* are set). Only
    // meaningful when DB-user login is on.
    if db_auth_on {
        if let Some(store) = store_for_db.as_ref() {
            bootstrap_admin(store).await?;
        }
    }

    tracing::info!(
        component = "center",
        oidc = oidc_on,
        single_admin = single_admin_on,
        db_auth = db_auth_on,
        rbac = rbac,
        "access control assembled"
    );

    let app = if any_password_login {
        // Mount the unified login (+ reuse local logout/me), then compose
        // with local_auth_intent=false so the single-admin login is not
        // ALSO mounted (which would panic axum on a duplicate route).
        let login_state = crate::common::db_auth::UnifiedLoginState {
            store: if db_auth_on { store_for_db.clone() } else { None },
            local: auth_state
                .local
                .load_full()
                .expect("local validator installed when any password login is active"),
            single_admin_enabled: single_admin_on,
        };
        let business =
            crate::common::db_auth::add_unified_auth_routes(base_router, login_state, auth_state.clone());
        crate::common::api::compose_admin_routes(business, auth_state, false, authz)
    } else {
        // OIDC-only or nothing configured: no /login route is mounted.
        crate::common::api::compose_admin_routes(base_router, auth_state, false, authz)
    };
    Ok(app)
}

/// Validate an orthogonal access-control axis combination at startup.
///
/// Pure and testable. Returns `Err` (fail-close, no silent fallback) when:
/// - `rbac` is on but there is no usable store (DbAuthz needs the DB), or
/// - `db_auth` is on but there is no usable store (DB-user login needs the DB), or
/// - any password login (single admin or DB users) is on but no signing secret
///   is configured (tokens could not be signed/validated).
///
/// Having NO authn provider at all is NOT an error here: business routes
/// fail-close with 503 at request time. `oidc_on` is accepted for symmetry /
/// future cross-checks but does not by itself impose a requirement.
fn validate_access(
    oidc_on: bool,
    single_admin_on: bool,
    db_auth_on: bool,
    rbac: bool,
    has_secret: bool,
    store_present: bool,
) -> anyhow::Result<()> {
    let _ = oidc_on;
    let any_password_login = single_admin_on || db_auth_on;
    if rbac && !store_present {
        return Err(anyhow::anyhow!(
            "authz.mode=rbac requires a database: set database.enabled=true with a reachable \
             backend. Refusing to start."
        ));
    }
    if db_auth_on && !store_present {
        return Err(anyhow::anyhow!(
            "db_auth.enabled=true requires a database: set database.enabled=true with a reachable \
             backend. Refusing to start."
        ));
    }
    if any_password_login && !has_secret {
        return Err(anyhow::anyhow!(
            "password login (local_auth single admin or db_auth) requires a signing secret: set \
             local_auth.jwt_secret or db_auth.jwt_secret. Refusing to start."
        ));
    }
    Ok(())
}

/// First-run admin bootstrap for DB-user login.
///
/// When the `users` table is empty, reads `EDGION_ADMIN_USERNAME` /
/// `EDGION_ADMIN_PASSWORD`. If both are set, creates the admin user (bcrypt),
/// a built-in `admin` role holding EVERY catalog permission key, and binds the
/// user to that role — yielding a working superuser-by-keys. If the env vars are
/// unset (and there are no users) it logs a prominent WARN: login is impossible
/// until a user is provisioned. Credentials are never invented.
///
/// No-op when users already exist (idempotent across restarts).
async fn bootstrap_admin(store: &crate::store::Store) -> anyhow::Result<()> {
    if !store.list_users().await?.is_empty() {
        return Ok(());
    }

    let username = std::env::var("EDGION_ADMIN_USERNAME").ok().filter(|v| !v.is_empty());
    let password = std::env::var("EDGION_ADMIN_PASSWORD").ok().filter(|v| !v.is_empty());

    match (username, password) {
        (Some(username), Some(password)) => {
            let hash = bcrypt::hash(&password, bcrypt::DEFAULT_COST)
                .map_err(|e| anyhow::anyhow!("failed to hash bootstrap admin password: {}", e))?;
            let all_keys: Vec<String> = crate::common::authz::catalog::all_keys()
                .iter()
                .map(|k| (*k).to_string())
                .collect();
            // Provision the user, the built-in `admin` role, its permission grants,
            // and the user→role binding in ONE transaction. All-or-nothing: a crash
            // or DB error mid-way rolls back, so we never leave a non-empty users
            // table holding a permissionless admin that the empty-table guard above
            // would skip on the next startup (stranding an unrepairable admin).
            store
                .bootstrap_admin(
                    &username,
                    &hash,
                    "Bootstrap Admin",
                    "admin",
                    "Built-in administrator (all permissions)",
                    &all_keys,
                )
                .await?;
            tracing::info!(
                component = "center",
                username = %username,
                "bootstrap admin user created with the built-in 'admin' role (full access-control tier)"
            );
        }
        _ => {
            tracing::warn!(
                component = "center",
                "db_auth is enabled but the users table is empty and EDGION_ADMIN_USERNAME / \
                 EDGION_ADMIN_PASSWORD are not set: NO DB user can log in until one is provisioned. \
                 Set both env vars to bootstrap a first admin."
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{admin_tls_cookie_warning, build_access_app, decide_transport, validate_access, TransportDecision};
    use crate::common::auth::AdminAuthConfig;
    use crate::config::{AuthzMode, CenterConfig};
    use crate::common::config::{ConfSyncSecurityConfig, ConfSyncTlsConfig};
    use crate::common::unified_auth::UnifiedAuthState;

    fn mtls_sec() -> ConfSyncSecurityConfig {
        ConfSyncSecurityConfig {
            tls: Some(ConfSyncTlsConfig {
                name: None,
                cert: "c".into(),
                key: "k".into(),
                ca: "a".into(),
                skip_tls: false,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn validate_access_rbac_requires_store() {
        // rbac on, no store -> error.
        let err = validate_access(false, false, false, true, true, false).unwrap_err();
        assert!(err.to_string().contains("rbac"), "unexpected: {err}");
    }

    #[test]
    fn validate_access_db_auth_requires_store() {
        // db_auth on (with a secret), no store -> error.
        let err = validate_access(false, false, true, false, true, false).unwrap_err();
        assert!(err.to_string().contains("db_auth"), "unexpected: {err}");
    }

    #[test]
    fn validate_access_password_login_requires_secret() {
        // single admin on, no secret -> error.
        let err = validate_access(false, true, false, false, false, true).unwrap_err();
        assert!(err.to_string().contains("signing secret"), "unexpected: {err}");
    }

    #[test]
    fn validate_access_ok_valid_combos() {
        // OIDC + single admin (with secret) + allow_all, no store needed -> ok.
        assert!(validate_access(true, true, false, false, true, false).is_ok());
        // db_auth + rbac with store + secret -> ok.
        assert!(validate_access(false, false, true, true, true, true).is_ok());
        // Nothing configured at all is not an error here (503 at request time).
        assert!(validate_access(false, false, false, false, false, false).is_ok());
    }

    /// POST `/api/v1/auth/login` against the composed app and return the status.
    /// Used to probe whether the unified login route was mounted (404 when not).
    async fn post_login_status(app: axum::Router) -> u16 {
        use axum::body::Body;
        use axum::http::{Method, Request};
        use tower::ServiceExt;
        let body = serde_json::json!({ "username": "nobody", "password": "nope" }).to_string();
        let req = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/auth/login")
            .header("content-type", "application/json")
            .body(Body::from(body))
            .unwrap();
        app.oneshot(req).await.unwrap().status().as_u16()
    }

    /// rbac requires a store: with `authz.mode = rbac` and no Store, build_access_app
    /// must surface the validate_access error (fail-close) rather than assemble an app.
    #[tokio::test]
    async fn build_access_rbac_without_store_errors() {
        let mut config = CenterConfig::default();
        config.authz.mode = AuthzMode::Rbac;
        let res = build_access_app(&config, axum::Router::new(), None, true).await;
        let err = res.expect_err("rbac without a store must error").to_string();
        assert!(err.contains("rbac"), "unexpected error: {err}");
    }

    /// allow_all + OIDC only (no single admin, no db_auth): the app assembles, but
    /// because no password-login axis is active the unified `/login` route is NOT
    /// mounted, so a POST to it returns 404 (unmatched).
    #[tokio::test]
    async fn build_access_allow_all_oidc_only_mounts_no_login() {
        // reqwest's TLS client construction (OidcProvider::from_config) is happier
        // with a process CryptoProvider installed; idempotent install.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let config = CenterConfig {
            auth: Some(AdminAuthConfig {
                enabled: true,
                discovery: "https://idp.example.com/.well-known/openid-configuration".to_string(),
                ..AdminAuthConfig::default()
            }),
            ..CenterConfig::default()
        };
        // authz.mode defaults to allow_all; local_auth None; db_auth off.
        let app = build_access_app(&config, axum::Router::new(), None, true)
            .await
            .expect("oidc-only allow_all must assemble");
        let status = post_login_status(app).await;
        assert_eq!(status, 404, "no password login => /login must not be mounted (got {status})");
    }

    /// db_auth on (with a store + a signing secret): the unified `/login` route IS
    /// mounted, so a POST to it reaches the handler (a uniform 401 for bad creds),
    /// not a 404. Proves the login wiring is installed.
    #[tokio::test]
    async fn build_access_db_auth_mounts_login() {
        use std::sync::Arc;
        let store = Arc::new(crate::store::Store::open_in_memory().await.unwrap());
        let mut config = CenterConfig::default();
        config.db_auth.enabled = true;
        config.db_auth.jwt_secret = Some("a_long_enough_jwt_secret_value_abcdef".to_string());
        let app = build_access_app(&config, axum::Router::new(), Some(store), true)
            .await
            .expect("db_auth with store + secret must assemble");
        let status = post_login_status(app).await;
        assert_ne!(status, 404, "db_auth on => /login must be mounted (got {status})");
        assert_eq!(status, 401, "bad creds against the mounted login => uniform 401 (got {status})");
    }

    /// `local_auth.enabled = false` is honored symmetrically with `auth.enabled`:
    /// even with a non-empty username + password present, the single-admin provider
    /// does NOT load. With no other authn provider and no db_auth, the app assembles
    /// but mounts no password login (POST /login => 404), and because there is then
    /// NO provider at all, business routes fail-close with 503 (auth still mandatory).
    #[tokio::test]
    async fn build_access_local_auth_disabled_mounts_no_login() {
        use axum::{body::Body, http::Request, routing::get, Router};
        use tower::ServiceExt;

        let config = CenterConfig {
            local_auth: Some(crate::common::local_auth::LocalAuthConfig {
                enabled: false,
                username: "admin".to_string(),
                password: "a-real-password".to_string(),
                jwt_secret: "a_long_enough_jwt_secret_value_abcdef".to_string(),
                ..crate::common::local_auth::LocalAuthConfig::default()
            }),
            ..CenterConfig::default()
        };
        // No OIDC, no db_auth, authz.mode defaults to allow_all.

        let business = Router::new().route("/api/v1/controllers", get(|| async { "controllers" }));
        let app = build_access_app(&config, business, None, true)
            .await
            .expect("local_auth disabled must still assemble (no provider => 503 at request time)");

        // The disabled single-admin login must NOT be mounted.
        let login_status = post_login_status(app.clone()).await;
        assert_eq!(
            login_status, 404,
            "local_auth.enabled=false => /login must not be mounted (got {login_status})"
        );

        // With no provider configured, business routes fail-close with 503.
        let req = Request::builder()
            .uri("/api/v1/controllers")
            .body(Body::empty())
            .unwrap();
        let status = app.oneshot(req).await.unwrap().status().as_u16();
        assert_eq!(
            status, 503,
            "no authn provider (local_auth disabled) => business route must 503 (got {status})"
        );
    }

    #[test]
    fn none_fail_closes() {
        let sec = ConfSyncSecurityConfig::default();
        assert_eq!(decide_transport(&sec, None), TransportDecision::FailClose);
    }

    #[test]
    fn skip_tls_fail_closes() {
        let sec = ConfSyncSecurityConfig {
            tls: Some(ConfSyncTlsConfig {
                name: None,
                cert: "c".into(),
                key: "k".into(),
                ca: "a".into(),
                skip_tls: true,
            }),
            ..Default::default()
        };
        assert_eq!(decide_transport(&sec, Some("edgion.io")), TransportDecision::FailClose);
    }

    #[test]
    fn mtls_with_trust_domain_is_mtls() {
        assert_eq!(
            decide_transport(&mtls_sec(), Some("edgion.io")),
            TransportDecision::Mtls
        );
    }

    #[test]
    fn mtls_without_trust_domain_fail_closes() {
        assert_eq!(decide_transport(&mtls_sec(), None), TransportDecision::FailClose);
        assert_eq!(decide_transport(&mtls_sec(), Some("")), TransportDecision::FailClose);
    }

    /// When both local_auth and auth are None on Center:
    /// 1. UnifiedAuthState must be constructed successfully (must not panic).
    /// 2. The safe-default derivation of require_auth must be true (so the
    ///    middleware takes the 503 branch rather than passthrough,
    ///    preventing "no configuration = admin API exposed").
    #[test]
    fn center_boot_with_no_auth_config_does_not_panic() {
        let auth_config: Option<&crate::common::auth::AdminAuthConfig> = None;
        let local_auth_config: Option<&crate::common::local_auth::LocalAuthConfig> = None;

        // Safe-default formula (matches the one in run())
        let local_intent = local_auth_config.is_some_and(|c| c.enabled);
        let oidc_intent = auth_config.is_some_and(|c| c.enabled);
        let nothing_configured = local_auth_config.is_none() && auth_config.is_none();
        let require_auth = local_intent || oidc_intent || nothing_configured;

        // When both are None, this must be true; otherwise the middleware would passthrough
        assert!(
            require_auth,
            "require_auth must be true when nothing is configured (safe default)"
        );

        // And state must build successfully (no validate path since both are None)
        let state = UnifiedAuthState::from_configs(auth_config, local_auth_config, require_auth, "test")
            .expect("from_configs must succeed with no auth configs and require_auth=true");
        assert!(state.require_auth);
    }

    /// HTTP-level regression: in the nothing_configured case, business routes must return 503 (not 200 and not 401).
    #[tokio::test]
    async fn center_no_auth_config_admin_routes_return_503() {
        use axum::{body::Body, http::Request, routing::get, Router};
        use std::sync::Arc;
        use tower::ServiceExt;

        // Simulate Center's require_auth derivation at startup (nothing_configured -> true)
        let auth_config: Option<&crate::common::auth::AdminAuthConfig> = None;
        let local_auth_config: Option<&crate::common::local_auth::LocalAuthConfig> = None;
        let require_auth = true; // nothing_configured case

        let auth_state: Arc<UnifiedAuthState> =
            UnifiedAuthState::from_configs(auth_config, local_auth_config, require_auth, "test").unwrap();

        let business = Router::new().route("/api/v1/controllers", get(|| async { "controllers" }));
        let authz: Arc<dyn crate::common::authz::AuthzStore> =
            Arc::new(crate::common::authz::allow_all::AllowAllAuthz);
        let app = crate::common::api::compose_admin_routes(business, auth_state, false, authz);

        let req = Request::builder()
            .uri("/api/v1/controllers")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(
            resp.status().as_u16(),
            503,
            "admin business route must return 503 when nothing_configured (got {})",
            resp.status()
        );
    }

    /// CenterConfig::default() must carry no admin credentials (no local_auth and no OIDC).
    #[test]
    fn center_default_config_yields_no_auth_config() {
        let config = CenterConfig::default();
        assert!(
            config.local_auth.is_none(),
            "default CenterConfig should not inject edgion1234 via local_auth"
        );
        assert!(
            config.auth.as_ref().is_none_or(|a| !a.enabled),
            "default CenterConfig should not enable OIDC auth"
        );
    }

    #[test]
    fn test_admin_tls_cookie_warning_combinations() {
        // Contradiction: Center terminates HTTPS but cookie is non-Secure -> warn.
        // Assert a substring UNIQUE to this arm so a swapped match arm would fail the test.
        // ("admin_tls is enabled" appears only here; "cookie_secure=true" appears in BOTH arms.)
        let msg = admin_tls_cookie_warning(true, false).expect("contradiction case should warn");
        assert!(msg.contains("admin_tls is enabled"), "unexpected message: {msg}");
        // Silent-drop risk: Secure cookie but admin listener is plain HTTP -> warn.
        // "admin_tls is not configured" is unique to this arm.
        let msg = admin_tls_cookie_warning(false, true).expect("silent-drop case should warn");
        assert!(msg.contains("admin_tls is not configured"), "unexpected message: {msg}");
        // Safe: HTTPS + Secure cookie.
        assert!(admin_tls_cookie_warning(true, true).is_none());
        // Safe: plain HTTP + non-Secure cookie (consistent dev setup).
        assert!(admin_tls_cookie_warning(false, false).is_none());
    }

    #[tokio::test]
    async fn test_rustls_config_loads_self_signed_pem() {
        use rcgen::{CertificateParams, KeyPair};
        // ServerConfig needs a process CryptoProvider; main installs it, tests must too.
        let _ = rustls::crypto::ring::default_provider().install_default();

        let key_pair = KeyPair::generate().expect("rcgen key generation failed");
        let params = CertificateParams::new(vec!["localhost".to_string()]).expect("rcgen params failed");
        let cert = params.self_signed(&key_pair).expect("rcgen self-sign failed");

        let dir = tempfile::tempdir().unwrap();
        let cert_path = dir.path().join("server.crt");
        let key_path = dir.path().join("server.key");
        std::fs::write(&cert_path, cert.pem()).unwrap();
        std::fs::write(&key_path, key_pair.serialize_pem()).unwrap();

        let res = axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert_path, &key_path).await;
        assert!(res.is_ok(), "valid self-signed PEM should load: {:?}", res.err());
    }

    #[tokio::test]
    async fn test_rustls_config_rejects_malformed_pem() {
        let dir = tempfile::tempdir().unwrap();
        let cert_path = dir.path().join("bad.crt");
        let key_path = dir.path().join("bad.key");
        std::fs::write(&cert_path, b"not a pem at all").unwrap();
        std::fs::write(&key_path, b"also not a key").unwrap();

        let res = axum_server::tls_rustls::RustlsConfig::from_pem_file(&cert_path, &key_path).await;
        assert!(res.is_err(), "malformed PEM must return Err, not panic");
    }

    #[tokio::test]
    async fn test_rustls_config_rejects_missing_path() {
        let res =
            axum_server::tls_rustls::RustlsConfig::from_pem_file("/no/such/dir/cert.crt", "/no/such/dir/key.key").await;
        assert!(res.is_err(), "missing cert/key files must return Err, not panic");
    }
}
