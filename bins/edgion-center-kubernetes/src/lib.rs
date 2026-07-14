mod config;

use anyhow::Context;
use clap::Parser;
use edgion_center_adapter_kubernetes::{
    KubernetesControllerDirectory, KubernetesControllerOwnerLocator, KubernetesLeaseCoordinator,
    KubernetesSarAuthorizer, StructuredStdoutAudit,
};
use edgion_center_app::{
    aggregator::{FedAggregatorMetrics, ResourceAggregator},
    api::{self, ApiState},
    commander::Commander,
    common::{self, audit::AuditSink, unified_auth::UnifiedAuthState},
    fed_sync::{
        registry::{ControllerRegistry, FedRegistryMetrics},
        server::FederationGrpcServer,
    },
    metadata_store::CenterMetaDataStore,
    proxy::{PendingProxyMap, ProxyForwarder},
    watch_cache::{CenterSyncClient, CenterWatchCacheRegistry},
};
use edgion_center_core::{
    Action, Authorizer, AuthzMode, CenterCapabilities, CenterMode, ControllerDirectory,
    ControllerOwnerLocator, ControllerPhase, ControllerRecord, CoordinationRole, Coordinator,
    CoreError, OwnershipFence, Principal, SessionId,
};
use edgion_center_runtime::internal_forwarding::{
    proto::internal_forwarding_server::InternalForwardingServer, GrpcInternalForwardTransport,
    InternalForwardingService, OwnerForwarding,
};
use parking_lot::Mutex;
use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
use tonic::transport::Server;
use tracing_subscriber::EnvFilter;

use config::KubernetesCenterConfig;

#[derive(Parser)]
#[command(name = "edgion-center-kubernetes", version)]
struct Cli {
    #[arg(short = 'c', long, default_value = "/etc/edgion-center/config.yaml")]
    config_file: String,
}

fn refresh_combined_readiness(
    overall: &std::sync::atomic::AtomicBool,
    platform: &std::sync::atomic::AtomicBool,
    read_model: &std::sync::atomic::AtomicBool,
) {
    overall.store(
        platform.load(std::sync::atomic::Ordering::Acquire)
            && read_model.load(std::sync::atomic::Ordering::Acquire),
        std::sync::atomic::Ordering::Release,
    );
}

async fn reconcile_expired_owner(
    directory: &dyn ControllerDirectory,
    coordinator: &dyn Coordinator,
    record: &ControllerRecord,
) -> Result<(), String> {
    let Some(session) = record.current_session_id.as_ref() else {
        return Ok(());
    };
    // The expired observation is not sufficient authority. Acquiring the
    // Lease rotates its fence through Kubernetes CAS; a concurrent renewal
    // wins as Conflict and prevents a false offline projection.
    let leadership = match coordinator
        .acquire(CoordinationRole::ControllerOwner(
            record.controller_id.to_string(),
        ))
        .await
    {
        Ok(leadership) => leadership,
        Err(CoreError::Conflict(_)) => {
            return Err("Controller owner renewed before reconciliation".to_string())
        }
        Err(error) => return Err(error.to_string()),
    };
    let fence = OwnershipFence {
        token: leadership.fencing_token.clone(),
        epoch: leadership.fencing_epoch,
    };
    let observed_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64;
    let projection = directory
        .mark_offline(&record.controller_id, session, Some(&fence), observed_at)
        .await;
    let release = coordinator.release(&leadership).await;
    projection.map_err(|error| error.to_string())?;
    release.map_err(|error| error.to_string())?;
    Ok(())
}

fn controller_revision(record: &ControllerRecord) -> String {
    let phase = match record.phase {
        ControllerPhase::Online => "online",
        ControllerPhase::Offline => "offline",
        ControllerPhase::Stale => "stale",
    };
    let session = record
        .current_session_id
        .as_ref()
        .map(SessionId::as_str)
        .unwrap_or("");
    let (token, epoch) = record
        .ownership_fence
        .as_ref()
        .map(|fence| (fence.token.as_str(), fence.epoch))
        .unwrap_or(("", 0));
    format!("{phase}\0{session}\0{epoch}\0{token}")
}

fn controller_revisions(records: &[ControllerRecord]) -> HashMap<String, String> {
    records
        .iter()
        .map(|record| {
            (
                record.controller_id.to_string(),
                controller_revision(record),
            )
        })
        .collect()
}

fn owner_route_matches_record(
    record: &ControllerRecord,
    route: &edgion_center_core::ControllerOwnerRoute,
) -> bool {
    record.connected_replica.as_deref() == Some(route.holder.as_str())
        && record.ownership_fence.as_ref() == Some(&route.ownership_fence)
}

pub async fn entrypoint() -> anyhow::Result<()> {
    common::startup::install_panic_hook();
    common::startup::init_crypto();
    init_json_tracing()?;
    common::observe::metrics_api::install_global_recorder(
        common::observe::metrics_api::RecorderConfig {
            service: "edgion-center-kubernetes",
            idle_timeout_secs: 0,
            histogram_buckets: &[],
        },
    )
    .map_err(anyhow::Error::msg)?;

    let cli = Cli::parse();
    let content = tokio::fs::read_to_string(&cli.config_file)
        .await
        .with_context(|| format!("read {}", cli.config_file))?;
    let config: KubernetesCenterConfig =
        serde_yaml::from_str(&content).with_context(|| format!("parse {}", cli.config_file))?;
    run(config).await
}

fn init_json_tracing() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"))
        .add_directive("edgion_center_audit=info".parse()?);
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .try_init()
        .map_err(|error| anyhow::anyhow!("initialize tracing: {error}"))
}

async fn run(config: KubernetesCenterConfig) -> anyhow::Result<()> {
    let identity = config.validate()?;
    ensure_distinct_internal_ca(&config).await?;
    let client = kube::Client::try_default()
        .await
        .context("create Kubernetes client")?;
    let directory = Arc::new(KubernetesControllerDirectory::new(
        client.clone(),
        &identity.namespace,
    ));
    let coordinator = Arc::new(KubernetesLeaseCoordinator::new(
        client.clone(),
        &identity.namespace,
        identity.holder.clone(),
        Duration::from_secs(config.lease_duration_secs),
    )?);
    let authorizer = Arc::new(KubernetesSarAuthorizer::new(
        client.clone(),
        identity.namespace.clone(),
    ));
    let audit_writer = Arc::new(StructuredStdoutAudit);
    platform_health_check(
        directory.as_ref(),
        coordinator.as_ref(),
        authorizer.as_ref(),
        Duration::from_secs(5),
    )
    .await?;
    // A successful platform preflight is necessary but not sufficient: a
    // fresh replica must also rebuild both effective read-model snapshots.
    let platform_ready = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let registry = ControllerRegistry::with_metrics(Arc::new(FedRegistryMetrics));
    let aggregator = Arc::new(ResourceAggregator::with_metrics(Arc::new(
        FedAggregatorMetrics,
    )));
    let pending_proxies: PendingProxyMap = Arc::new(Mutex::new(HashMap::new()));
    let metadata_store = Arc::new(CenterMetaDataStore::new());
    let sync_client = Arc::new(CenterSyncClient {
        plugin_metadata: CenterWatchCacheRegistry::new(metadata_store.clone()),
    });

    let grpc_server = FederationGrpcServer::new(
        registry.clone(),
        aggregator.clone(),
        pending_proxies.clone(),
        config.sync.clone(),
        sync_client.clone(),
        Some(directory.clone()),
        Some(config.trust_domain.clone()),
    )
    .with_audit_writer(audit_writer.clone())
    .with_coordinator(coordinator.clone());
    let internal_tls_config = config
        .internal_forwarding
        .tls
        .as_ref()
        .expect("validated internal forwarding TLS");
    let internal_client_tls = common::grpc_tls::load_client_tls(internal_tls_config)
        .await?
        .domain_name(config.internal_forwarding.server_name.clone());
    let owner_locator = Arc::new(KubernetesControllerOwnerLocator::new(
        client,
        &identity.namespace,
        config.internal_forwarding.port,
    )?);
    let owner_forwarding = OwnerForwarding {
        locator: owner_locator.clone(),
        transport: Arc::new(GrpcInternalForwardTransport::new(
            internal_client_tls,
            config.internal_forwarding.max_request_bytes,
            config.internal_forwarding.max_response_bytes,
        )),
        local_holder: identity.holder.clone(),
    };
    let commander = Arc::new(
        Commander::new(
            registry.clone(),
            grpc_server.pending_commands.clone(),
            config.sync.command_timeout_secs,
        )
        .with_owner_forwarding(owner_forwarding.clone()),
    );
    let ownership_tasks = grpc_server.ownership_tasks();
    let runtime_projection_handle = grpc_server.runtime_projection_handle();
    let proxy = Arc::new(
        ProxyForwarder::new(
            registry.clone(),
            pending_proxies,
            config.sync.command_timeout_secs,
        )
        .with_owner_forwarding(owner_forwarding.clone()),
    );
    let local_evictor = Arc::new(
        edgion_center_runtime::eviction::LocalControllerEvictor::new(
            registry.clone(),
            aggregator.clone(),
            sync_client.clone(),
        ),
    );
    let controller_evictor: Arc<dyn edgion_center_runtime::eviction::ControllerEviction> = Arc::new(
        edgion_center_runtime::eviction::OwnerAwareControllerEvictor::with_owner_forwarding(
            local_evictor.clone(),
            owner_forwarding,
        ),
    );
    let internal_service = InternalForwardingService::new(
        commander.clone(),
        proxy.clone(),
        local_evictor.clone(),
        identity.holder.clone(),
        config.internal_forwarding.max_request_bytes,
        config.internal_forwarding.max_response_bytes,
        config.internal_forwarding.expected_peer_spiffe_id.clone(),
    );
    let api_state = ApiState {
        aggregator: aggregator.clone(),
        commander,
        proxy: proxy.clone(),
        controller_directory: Some(directory.clone()),
        controller_evictor,
        user_admin: None,
        role_admin: None,
        audit_reader: None,
        metadata_store: metadata_store.clone(),
        sync_client,
        registry: registry.clone(),
        platform_ready: platform_ready.clone(),
        authz_mode: AuthzMode::Rbac,
        platform_mode: CenterMode::Kubernetes,
        capabilities: CenterCapabilities::for_mode(CenterMode::Kubernetes),
    };

    let auth_state = UnifiedAuthState::from_configs(Some(&config.auth), None, true, "center")?;
    let audit_state = common::audit::middleware::AuditLayerState {
        sink: audit_writer.clone() as AuditSink,
        log_reads: config.audit_log_reads,
    };
    let business = api::router(api_state.clone()).layer(axum::middleware::from_fn_with_state(
        audit_state,
        common::audit::middleware::audit_middleware,
    ));
    let admin = common::api::compose_admin_routes(business, auth_state, false, authorizer.clone());
    let admin = match api::web::WebSource::resolve(config.web.dir.as_deref()) {
        Some(source) => {
            let source = Arc::new(source);
            admin.fallback(move |uri: axum::http::Uri| {
                let source = source.clone();
                async move { api::web::serve(source, uri).await }
            })
        }
        None => admin,
    };

    let grpc_addr: SocketAddr = config.server.grpc_addr.parse()?;
    let http_addr: SocketAddr = config.server.http_addr.parse()?;
    let probe_addr: SocketAddr = config.server.probe_addr.parse()?;
    let metrics_addr: SocketAddr = config.server.metrics_addr.parse()?;
    let internal_addr: SocketAddr = config.internal_forwarding.bind_addr.parse()?;
    let tls = config
        .grpc_security
        .resolve_mtls_or_refuse()
        .expect("validated mTLS configuration");
    let grpc_tls = common::grpc_tls::load_server_tls(tls).await?;
    let internal_server_tls = common::grpc_tls::load_server_tls(internal_tls_config).await?;
    let shutdown = CancellationToken::new();
    let mut tasks = tokio::task::JoinSet::new();
    let grpc_shutdown = shutdown.child_token();
    let grpc_service = Server::builder()
        .http2_keepalive_interval(Some(Duration::from_secs(10)))
        .http2_keepalive_timeout(Some(Duration::from_secs(5)))
        .tls_config(grpc_tls)?
        .add_service(
            common::fed_sync::proto::federation_sync_server::FederationSyncServer::new(grpc_server),
        );
    tasks.spawn(async move {
        let result = grpc_service
            .serve_with_shutdown(grpc_addr, grpc_shutdown.cancelled_owned())
            .await
            .map_err(anyhow::Error::from);
        ("federation", result)
    });
    let internal_shutdown = shutdown.child_token();
    let max_internal_request = config.internal_forwarding.max_request_bytes;
    let max_internal_response = config.internal_forwarding.max_response_bytes;
    let internal_grpc = Server::builder()
        .http2_keepalive_interval(Some(Duration::from_secs(10)))
        .http2_keepalive_timeout(Some(Duration::from_secs(5)))
        .tls_config(internal_server_tls)?
        .add_service(
            InternalForwardingServer::new(internal_service.clone())
                .max_decoding_message_size(max_internal_request)
                .max_encoding_message_size(max_internal_response),
        );
    tasks.spawn(async move {
        let result = internal_grpc
            .serve_with_shutdown(internal_addr, internal_shutdown.cancelled_owned())
            .await
            .map_err(anyhow::Error::from);
        ("internal-forwarding", result)
    });
    for (name, addr, app) in [
        ("admin", http_addr, admin),
        ("probe", probe_addr, api::create_probe_router(api_state)),
        ("metrics", metrics_addr, api::create_metrics_router()),
    ] {
        let task_shutdown = shutdown.child_token();
        tasks.spawn(async move {
            (
                name,
                serve(addr, app, task_shutdown.cancelled_owned()).await,
            )
        });
    }

    let platform_health_ready = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let read_model_ready = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let health_shutdown = shutdown.child_token();
    let health_directory = directory.clone();
    let health_coordinator = coordinator.clone();
    let health_authorizer = authorizer.clone();
    let health_component = platform_health_ready.clone();
    let health_model_component = read_model_ready.clone();
    let health_overall = platform_ready.clone();
    tasks.spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(10));
        let mut platform_failures = 0_u8;
        loop {
            tokio::select! {
                _ = health_shutdown.cancelled() => break,
                _ = ticker.tick() => {}
            }
            match platform_health_check(
                health_directory.as_ref(),
                health_coordinator.as_ref(),
                health_authorizer.as_ref(),
                Duration::from_secs(2),
            )
            .await
            {
                Ok(_) => {
                    platform_failures = 0;
                    health_component.store(true, std::sync::atomic::Ordering::Release);
                }
                Err(error) => {
                    platform_failures = platform_failures.saturating_add(1);
                    tracing::warn!(%error, platform_failures, "Kubernetes readiness check failed");
                    if platform_failures >= 3 {
                        health_component.store(false, std::sync::atomic::Ordering::Release);
                    }
                }
            }
            refresh_combined_readiness(&health_overall, &health_component, &health_model_component);
        }
        ("platform-health", Ok(()))
    });

    let sync_shutdown = shutdown.child_token();
    let sync_directory = directory.clone();
    let sync_coordinator = coordinator.clone();
    let sync_owner_locator = owner_locator.clone();
    let sync_local_evictor = local_evictor.clone();
    let sync_registry = registry.clone();
    let sync_store = metadata_store.clone();
    let sync_proxy = proxy.clone();
    let sync_component = read_model_ready.clone();
    let sync_health_component = platform_health_ready.clone();
    let sync_overall = platform_ready.clone();
    tasks.spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(10));
        loop {
            tokio::select! {
                _ = sync_shutdown.cancelled() => break,
                _ = ticker.tick() => {}
            }
            let records = match sync_directory.list().await {
                Ok(records) => records,
                Err(error) => {
                    tracing::warn!(%error, "Global Controller directory sync failed");
                    sync_component.store(false, std::sync::atomic::Ordering::Release);
                    refresh_combined_readiness(
                        &sync_overall,
                        &sync_health_component,
                        &sync_component,
                    );
                    continue;
                }
            };
            let visible: std::collections::HashSet<String> = records
                .iter()
                .map(|record| record.controller_id.to_string())
                .collect();
            sync_store.retain_controllers(&visible);
            let initial_revisions = controller_revisions(&records);
            if !sync_store.revisions_match(&initial_revisions) {
                sync_component.store(false, std::sync::atomic::Ordering::Release);
                refresh_combined_readiness(&sync_overall, &sync_health_component, &sync_component);
            }
            sync_store.prepare_revisions(&initial_revisions);
            for controller_id in sync_registry.online_controller_ids() {
                if !visible.contains(&controller_id) {
                    sync_local_evictor.evict_unfenced(&controller_id);
                }
            }
            let online_ids: std::collections::HashSet<String> = records
                .iter()
                .filter(|record| record.phase == ControllerPhase::Online)
                .map(|record| record.controller_id.to_string())
                .collect();
            let mut polls = tokio::task::JoinSet::new();
            let limit = Arc::new(tokio::sync::Semaphore::new(8));
            for record in records
                .into_iter()
                .filter(|record| record.phase == ControllerPhase::Online)
            {
                let permit = limit.clone().acquire_owned().await.expect("semaphore open");
                let locator = sync_owner_locator.clone();
                let coordinator = sync_coordinator.clone();
                let directory = sync_directory.clone();
                let proxy = sync_proxy.clone();
                let store = sync_store.clone();
                let revision = initial_revisions
                    .get(record.controller_id.as_str())
                    .cloned()
                    .expect("listed Controller has revision");
                polls.spawn(async move {
                    let _permit = permit;
                    let id = record.controller_id.clone();
                    match locator.locate(&id).await {
                        Ok(Some(route)) if owner_route_matches_record(&record, &route) => {
                            tokio::time::timeout(
                                Duration::from_secs(5),
                                edgion_center_app::poll::poll_controller_once_owner_fenced(
                                    proxy.as_ref(),
                                    store.as_ref(),
                                    id.as_str(),
                                    &revision,
                                    &route,
                                ),
                            )
                            .await
                            .map_err(|_| "effective snapshot poll timed out".to_string())?
                        }
                        Ok(Some(_)) => Err(
                            "Controller owner route changed before CRD ownership projection"
                                .to_string(),
                        ),
                        Ok(None) => {
                            reconcile_expired_owner(
                                directory.as_ref(),
                                coordinator.as_ref(),
                                &record,
                            )
                            .await?;
                            Err("expired owner was reconciled; awaiting next sweep".to_string())
                        }
                        Err(error) => Err(error.to_string()),
                    }
                });
            }
            while let Some(result) = polls.join_next().await {
                if let Err(error) = result.unwrap_or_else(|error| Err(error.to_string())) {
                    tracing::warn!(%error, "Global effective snapshot sync incomplete");
                }
            }
            let final_revisions = match sync_directory.list().await {
                Ok(records) => controller_revisions(&records),
                Err(error) => {
                    tracing::warn!(%error, "Final Controller directory verification failed");
                    HashMap::new()
                }
            };
            let complete = initial_revisions == final_revisions
                && sync_store.has_fresh_coverage(&online_ids, Duration::from_secs(30));
            sync_component.store(complete, std::sync::atomic::Ordering::Release);
            refresh_combined_readiness(&sync_overall, &sync_health_component, &sync_component);
        }
        ("global-read-model", Ok(()))
    });

    tracing::info!(%grpc_addr, %http_addr, %internal_addr, namespace = %identity.namespace, holder = %identity.holder, "Kubernetes-native Edgion Center started");
    let unexpected_exit = tokio::select! {
        _ = shutdown_signal() => None,
        result = tasks.join_next() => Some(result),
    };

    platform_ready.store(false, std::sync::atomic::Ordering::Release);
    internal_service.stop_accepting();
    runtime_projection_handle.stop();
    let cancelled_sessions = registry.cancel_all();
    tracing::info!(cancelled_sessions, "Draining Kubernetes-native Center");
    shutdown.cancel();
    let drained = tokio::time::timeout(Duration::from_secs(20), async {
        while let Some(result) = tasks.join_next().await {
            match result {
                Ok((name, Ok(()))) => tracing::debug!(task = name, "Center task stopped"),
                Ok((name, Err(error))) => {
                    tracing::warn!(task = name, %error, "Center task stopped with error")
                }
                Err(error) => tracing::warn!(%error, "Center task join failed"),
            }
        }
        // The gRPC listener is now fully stopped, so no new session can spawn
        // an ownership maintainer. Wait for every canceled session's final
        // fenced Lease release before allowing the process runtime to exit.
        ownership_tasks.wait().await;
        runtime_projection_handle.wait().await;
    })
    .await;
    if drained.is_err() {
        tasks.abort_all();
        tracing::warn!("Center graceful shutdown deadline elapsed; aborting remaining tasks");
    }

    match unexpected_exit {
        None => Ok(()),
        Some(Some(Ok((name, result)))) => {
            anyhow::bail!("{name} task exited unexpectedly: {result:?}")
        }
        Some(Some(Err(error))) => anyhow::bail!("Center task join failed: {error}"),
        Some(None) => anyhow::bail!("Center task supervisor became empty unexpectedly"),
    }
}

async fn ensure_distinct_internal_ca(config: &KubernetesCenterConfig) -> anyhow::Result<()> {
    let federation = config
        .grpc_security
        .resolve_mtls_or_refuse()
        .expect("validated federation TLS");
    let internal = config
        .internal_forwarding
        .tls
        .as_ref()
        .expect("validated internal TLS");
    let federation_ca = tokio::fs::read(federation.ca_path())
        .await
        .context("read federation CA for isolation check")?;
    let internal_ca = tokio::fs::read(internal.ca_path())
        .await
        .context("read internal forwarding CA for isolation check")?;
    validate_ca_isolation(&federation_ca, &internal_ca)
}

fn validate_ca_isolation(federation_ca: &[u8], internal_ca: &[u8]) -> anyhow::Result<()> {
    if federation_ca == internal_ca {
        anyhow::bail!("internal forwarding CA material must differ from federation CA material");
    }
    Ok(())
}

async fn platform_health_check(
    directory: &dyn ControllerDirectory,
    coordinator: &dyn Coordinator,
    authorizer: &dyn Authorizer,
    lease_wait: Duration,
) -> anyhow::Result<Vec<edgion_center_core::ControllerRecord>> {
    let records = directory
        .list()
        .await
        .context("Kubernetes CRD/list preflight failed")?;
    let lease_deadline = tokio::time::Instant::now() + lease_wait;
    let leadership = loop {
        match coordinator
            .acquire(CoordinationRole::Maintenance("runtime-health".to_string()))
            .await
        {
            Ok(leadership) => break leadership,
            Err(edgion_center_core::CoreError::Conflict(message))
                if tokio::time::Instant::now() < lease_deadline =>
            {
                tracing::debug!(%message, "Waiting for shared startup preflight Lease");
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            Err(error) => return Err(error).context("Kubernetes Lease preflight failed"),
        }
    };
    coordinator
        .release(&leadership)
        .await
        .context("Kubernetes Lease release preflight failed")?;
    authorizer
        .authorize(
            &Principal {
                subject: "startup-preflight".to_string(),
                provider: "oidc".to_string(),
                issuer: Some("https://startup-preflight.invalid".to_string()),
                groups: Vec::new(),
            },
            &Action {
                permission: "server:read".to_string(),
                controller_id: None,
                operation: None,
                request_path: Some("/api/v1/server-info".to_string()),
                request_verb: Some("get".to_string()),
            },
        )
        .await
        .context("Kubernetes SubjectAccessReview preflight failed")?;
    Ok(records)
}

async fn serve(
    addr: SocketAddr,
    app: axum::Router,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown)
    .await?;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c()
        .await
        .expect("install Ctrl-C handler");
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgion_center_core::{
        ControllerId, ControllerRecord, ControllerRegistration, CoreError, CoreResult,
        EvictionResult, Leadership, OfflineOutcome, OwnershipFence, ReleaseOutcome, RenewalOutcome,
        SessionId,
    };
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct TestDirectory(bool);

    #[async_trait::async_trait]
    impl ControllerDirectory for TestDirectory {
        async fn upsert_registration(&self, _: ControllerRegistration) -> CoreResult<()> {
            unreachable!()
        }
        async fn mark_offline(
            &self,
            _: &ControllerId,
            _: &SessionId,
            _: Option<&OwnershipFence>,
            _: i64,
        ) -> CoreResult<OfflineOutcome> {
            unreachable!()
        }
        async fn list(&self) -> CoreResult<Vec<ControllerRecord>> {
            if self.0 {
                Ok(Vec::new())
            } else {
                Err(CoreError::Adapter("CRD unavailable".to_string()))
            }
        }
        async fn evict(&self, _: &ControllerId) -> CoreResult<EvictionResult> {
            unreachable!()
        }
    }

    struct TestCoordinator(bool);

    #[async_trait::async_trait]
    impl Coordinator for TestCoordinator {
        async fn acquire(&self, role: CoordinationRole) -> CoreResult<Leadership> {
            if !self.0 {
                return Err(CoreError::Adapter("Lease RBAC denied".to_string()));
            }
            Ok(Leadership {
                role,
                holder: "test".to_string(),
                fencing_token: "token".to_string(),
                fencing_epoch: 1,
                valid_for_millis: 1_000,
            })
        }
        async fn renew(&self, _: &Leadership) -> CoreResult<RenewalOutcome> {
            unreachable!()
        }
        async fn release(&self, _: &Leadership) -> CoreResult<ReleaseOutcome> {
            Ok(ReleaseOutcome::Released)
        }
    }

    struct RenewedOwnerCoordinator;

    #[async_trait::async_trait]
    impl Coordinator for RenewedOwnerCoordinator {
        async fn acquire(&self, _: CoordinationRole) -> CoreResult<Leadership> {
            Err(CoreError::Conflict("owner renewed".to_string()))
        }
        async fn renew(&self, _: &Leadership) -> CoreResult<RenewalOutcome> {
            unreachable!()
        }
        async fn release(&self, _: &Leadership) -> CoreResult<ReleaseOutcome> {
            unreachable!()
        }
    }

    struct CountingDirectory(AtomicUsize);

    #[async_trait::async_trait]
    impl ControllerDirectory for CountingDirectory {
        async fn upsert_registration(&self, _: ControllerRegistration) -> CoreResult<()> {
            unreachable!()
        }
        async fn mark_offline(
            &self,
            _: &ControllerId,
            _: &SessionId,
            _: Option<&OwnershipFence>,
            _: i64,
        ) -> CoreResult<OfflineOutcome> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(OfflineOutcome::Marked)
        }
        async fn list(&self) -> CoreResult<Vec<ControllerRecord>> {
            unreachable!()
        }
        async fn evict(&self, _: &ControllerId) -> CoreResult<EvictionResult> {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn concurrent_owner_renewal_prevents_false_offline_projection() {
        let directory = CountingDirectory(AtomicUsize::new(0));
        let record = ControllerRecord {
            controller_id: ControllerId::new("c1").unwrap(),
            current_session_id: Some(SessionId::new("s1").unwrap()),
            cluster: "cluster-a".to_string(),
            environments: Vec::new(),
            tags: Vec::new(),
            connected_replica: Some("center-a/uid-a".to_string()),
            ownership_fence: Some(OwnershipFence {
                token: "old".to_string(),
                epoch: 1,
            }),
            sync_version: None,
            watch_server_id: None,
            resource_count: None,
            stats_updated_unix_ms: None,
            watch_updated_unix_ms: None,
            phase: ControllerPhase::Online,
            last_seen_unix_ms: 1,
        };
        assert!(
            reconcile_expired_owner(&directory, &RenewedOwnerCoordinator, &record)
                .await
                .is_err()
        );
        assert_eq!(directory.0.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn lease_ahead_of_crd_cannot_publish_under_stale_revision() {
        let record = ControllerRecord {
            controller_id: ControllerId::new("c1").unwrap(),
            current_session_id: Some(SessionId::new("session-a").unwrap()),
            cluster: "cluster-a".to_string(),
            environments: Vec::new(),
            tags: Vec::new(),
            connected_replica: Some("center-a/uid-a".to_string()),
            ownership_fence: Some(OwnershipFence {
                token: "token-a".to_string(),
                epoch: 1,
            }),
            sync_version: None,
            watch_server_id: None,
            resource_count: None,
            stats_updated_unix_ms: None,
            watch_updated_unix_ms: None,
            phase: ControllerPhase::Online,
            last_seen_unix_ms: 1,
        };
        let projected_route = edgion_center_core::ControllerOwnerRoute {
            holder: "center-a/uid-a".to_string(),
            endpoint: "https://10.0.0.1:12252".to_string(),
            ownership_fence: OwnershipFence {
                token: "token-a".to_string(),
                epoch: 1,
            },
        };
        assert!(owner_route_matches_record(&record, &projected_route));

        let lease_ahead_route = edgion_center_core::ControllerOwnerRoute {
            holder: "center-b/uid-b".to_string(),
            endpoint: "https://10.0.0.2:12252".to_string(),
            ownership_fence: OwnershipFence {
                token: "token-b".to_string(),
                epoch: 2,
            },
        };
        assert!(!owner_route_matches_record(&record, &lease_ahead_route));

        let rotated_fence_same_holder = edgion_center_core::ControllerOwnerRoute {
            holder: "center-a/uid-a".to_string(),
            endpoint: "https://10.0.0.1:12252".to_string(),
            ownership_fence: OwnershipFence {
                token: "token-b".to_string(),
                epoch: 2,
            },
        };
        assert!(!owner_route_matches_record(
            &record,
            &rotated_fence_same_holder
        ));
    }

    struct TestAuthorizer(bool);

    #[async_trait::async_trait]
    impl Authorizer for TestAuthorizer {
        async fn authorize(
            &self,
            _: &Principal,
            _: &Action,
        ) -> CoreResult<edgion_center_core::Decision> {
            if self.0 {
                Ok(edgion_center_core::Decision::deny(
                    "expected synthetic denial",
                ))
            } else {
                Err(CoreError::Adapter("SAR RBAC denied".to_string()))
            }
        }
    }

    struct ConflictCoordinator;

    #[async_trait::async_trait]
    impl Coordinator for ConflictCoordinator {
        async fn acquire(&self, _: CoordinationRole) -> CoreResult<Leadership> {
            Err(CoreError::Conflict("persistent conflict".to_string()))
        }
        async fn renew(&self, _: &Leadership) -> CoreResult<RenewalOutcome> {
            unreachable!()
        }
        async fn release(&self, _: &Leadership) -> CoreResult<ReleaseOutcome> {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn absent_crd_fails_before_readiness() {
        let result = platform_health_check(
            &TestDirectory(false),
            &TestCoordinator(true),
            &TestAuthorizer(true),
            Duration::ZERO,
        )
        .await;
        assert!(result.unwrap_err().to_string().contains("CRD/list"));
    }

    #[tokio::test]
    async fn invalid_runtime_rbac_fails_before_readiness() {
        let lease_error = platform_health_check(
            &TestDirectory(true),
            &TestCoordinator(false),
            &TestAuthorizer(true),
            Duration::ZERO,
        )
        .await
        .unwrap_err();
        assert!(lease_error.to_string().contains("Lease preflight"));

        let sar_error = platform_health_check(
            &TestDirectory(true),
            &TestCoordinator(true),
            &TestAuthorizer(false),
            Duration::ZERO,
        )
        .await
        .unwrap_err();
        assert!(sar_error.to_string().contains("SubjectAccessReview"));
    }

    #[tokio::test]
    async fn persistent_preflight_lease_conflict_fails_closed() {
        let error = platform_health_check(
            &TestDirectory(true),
            &ConflictCoordinator,
            &TestAuthorizer(true),
            Duration::ZERO,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("Lease preflight"));
    }

    #[test]
    fn deployment_fronts_loopback_admin_with_oidc_browser_proxy() {
        let deployment = include_str!("../../../cicd/deploy/center-kubernetes/deployment.yaml");
        let config = include_str!("../../../cicd/deploy/center-kubernetes/config.yaml");
        let service = include_str!("../../../cicd/deploy/center-kubernetes/service.yaml");

        assert!(deployment.contains("name: oauth2-proxy"));
        assert!(deployment.contains(
            "oauth2-proxy:v7.15.2@sha256:aa0bd8dd5ab0c78e4c91c92755ad573a5f92241f88138b4141b8ec803463b4fd"
        ));
        assert!(!deployment.contains("oauth2-proxy:v7.12."));
        assert!(deployment.contains("OAUTH2_PROXY_PASS_AUTHORIZATION_HEADER"));
        assert!(deployment.contains("OAUTH2_PROXY_CODE_CHALLENGE_METHOD"));
        assert!(deployment.contains("edgion-center-browser-oidc"));
        assert!(config.contains("http_addr: 127.0.0.1:12201"));
        assert!(service.contains("targetPort: auth-proxy"));
    }

    #[test]
    fn identical_ca_material_is_rejected_even_from_different_paths() {
        assert!(validate_ca_isolation(b"same-ca", b"same-ca").is_err());
        assert!(validate_ca_isolation(b"federation-ca", b"internal-ca").is_ok());
    }
}
