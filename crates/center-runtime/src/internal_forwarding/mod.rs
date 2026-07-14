//! Authenticated, one-hop forwarding between Kubernetes Center replicas.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use async_trait::async_trait;
use edgion_center_core::{ControllerOwnerRoute, OwnershipFence};
use http::StatusCode;
use prost::Message;

use crate::federation::proto::{command_request::Command, CommandResponse, HttpProxyResponse};
use crate::{
    commander::{Commander, FencedCommandError},
    eviction::LocalControllerEvictor,
    proxy::{FencedProxyError, ProxyForwarder},
};

pub mod proto {
    tonic::include_proto!("internal_forwarding");
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardErrorKind {
    StaleOwnership,
    Unavailable,
    Deadline,
    Rejected,
}

#[derive(Debug, Clone)]
pub struct ForwardError {
    pub kind: ForwardErrorKind,
    pub message: String,
}

impl ForwardError {
    pub fn stale(message: impl Into<String>) -> Self {
        Self {
            kind: ForwardErrorKind::StaleOwnership,
            message: message.into(),
        }
    }
}

#[async_trait]
pub trait InternalForwardTransport: Send + Sync {
    async fn forward_command(
        &self,
        route: &ControllerOwnerRoute,
        controller_id: &str,
        command: Command,
        timeout: Duration,
    ) -> Result<CommandResponse, ForwardError>;

    async fn forward_http(
        &self,
        route: &ControllerOwnerRoute,
        controller_id: &str,
        request: ForwardHttpOperation,
        timeout: Duration,
    ) -> Result<HttpProxyResponse, ForwardError>;

    async fn forward_evict(
        &self,
        route: &ControllerOwnerRoute,
        controller_id: &str,
    ) -> Result<(), ForwardError>;
}

#[derive(Debug, Clone)]
pub struct ForwardHttpOperation {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Clone)]
pub struct OwnerForwarding {
    pub locator: Arc<dyn edgion_center_core::ControllerOwnerLocator>,
    pub transport: Arc<dyn InternalForwardTransport>,
    pub local_holder: String,
}

pub fn expected_fence(route: &ControllerOwnerRoute) -> (&str, &OwnershipFence) {
    (&route.holder, &route.ownership_fence)
}

pub fn command_error(error: ForwardError) -> anyhow::Error {
    anyhow::anyhow!(error.message)
}

pub fn proxy_error(error: ForwardError) -> (StatusCode, String) {
    let status = match error.kind {
        ForwardErrorKind::Deadline => StatusCode::GATEWAY_TIMEOUT,
        ForwardErrorKind::StaleOwnership | ForwardErrorKind::Unavailable => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        ForwardErrorKind::Rejected => StatusCode::BAD_GATEWAY,
    };
    (status, error.message)
}

pub fn record_forward(operation: &'static str, route: &'static str, result: &'static str) {
    metrics::counter!(
        "edgion_center_internal_forward_total",
        "operation" => operation,
        "route" => route,
        "result" => result,
    )
    .increment(1);
}

#[derive(Clone)]
pub struct GrpcInternalForwardTransport {
    tls: tonic::transport::ClientTlsConfig,
    max_request_bytes: usize,
    max_response_bytes: usize,
}

impl GrpcInternalForwardTransport {
    pub fn new(
        tls: tonic::transport::ClientTlsConfig,
        max_request_bytes: usize,
        max_response_bytes: usize,
    ) -> Self {
        Self {
            tls,
            max_request_bytes,
            max_response_bytes,
        }
    }

    async fn client(
        &self,
        endpoint: &str,
        timeout: Duration,
    ) -> Result<
        proto::internal_forwarding_client::InternalForwardingClient<tonic::transport::Channel>,
        ForwardError,
    > {
        let endpoint = tonic::transport::Endpoint::from_shared(endpoint.to_string())
            .map_err(|error| unavailable(error.to_string()))?
            .connect_timeout(timeout)
            .timeout(timeout)
            .tls_config(self.tls.clone())
            .map_err(|error| unavailable(error.to_string()))?;
        let channel = endpoint
            .connect()
            .await
            .map_err(|error| unavailable(error.to_string()))?;
        Ok(
            proto::internal_forwarding_client::InternalForwardingClient::new(channel)
                .max_decoding_message_size(self.max_response_bytes)
                .max_encoding_message_size(self.max_request_bytes),
        )
    }
}

fn unavailable(message: String) -> ForwardError {
    ForwardError {
        kind: ForwardErrorKind::Unavailable,
        message,
    }
}

fn from_status(status: tonic::Status) -> ForwardError {
    let kind = match status.code() {
        tonic::Code::FailedPrecondition | tonic::Code::NotFound => ForwardErrorKind::StaleOwnership,
        tonic::Code::DeadlineExceeded => ForwardErrorKind::Deadline,
        tonic::Code::Unavailable => ForwardErrorKind::Unavailable,
        _ => ForwardErrorKind::Rejected,
    };
    ForwardError {
        kind,
        message: status.message().to_string(),
    }
}

fn target(route: &ControllerOwnerRoute) -> proto::OwnershipTarget {
    proto::OwnershipTarget {
        holder: route.holder.clone(),
        fencing_token: route.ownership_fence.token.clone(),
        fencing_epoch: route.ownership_fence.epoch,
    }
}

#[async_trait]
impl InternalForwardTransport for GrpcInternalForwardTransport {
    async fn forward_command(
        &self,
        route: &ControllerOwnerRoute,
        controller_id: &str,
        command: Command,
        timeout: Duration,
    ) -> Result<CommandResponse, ForwardError> {
        let mut client = self.client(&route.endpoint, timeout).await?;
        let operation_id = uuid::Uuid::new_v4().to_string();
        let request = proto::ForwardCommandRequest {
            operation_id: operation_id.clone(),
            controller_id: controller_id.to_string(),
            target: Some(target(route)),
            hop_count: 1,
            command: crate::federation::proto::CommandRequest {
                request_id: operation_id,
                command: Some(command),
            }
            .encode_to_vec(),
        };
        let payload = client
            .forward_command(tonic::Request::new(request))
            .await
            .map_err(from_status)?
            .into_inner()
            .payload;
        CommandResponse::decode(payload.as_slice()).map_err(|error| ForwardError {
            kind: ForwardErrorKind::Rejected,
            message: error.to_string(),
        })
    }

    async fn forward_http(
        &self,
        route: &ControllerOwnerRoute,
        controller_id: &str,
        request: ForwardHttpOperation,
        timeout: Duration,
    ) -> Result<HttpProxyResponse, ForwardError> {
        let mut client = self.client(&route.endpoint, timeout).await?;
        let operation_id = uuid::Uuid::new_v4().to_string();
        let request = proto::ForwardHttpRequest {
            operation_id: operation_id.clone(),
            controller_id: controller_id.to_string(),
            target: Some(target(route)),
            hop_count: 1,
            request: crate::federation::proto::HttpProxyRequest {
                request_id: operation_id,
                method: request.method,
                path: request.path,
                headers: sanitize_headers(request.headers),
                body: request.body,
            }
            .encode_to_vec(),
        };
        let payload = client
            .forward_http(tonic::Request::new(request))
            .await
            .map_err(from_status)?
            .into_inner()
            .payload;
        HttpProxyResponse::decode(payload.as_slice()).map_err(|error| ForwardError {
            kind: ForwardErrorKind::Rejected,
            message: error.to_string(),
        })
    }

    async fn forward_evict(
        &self,
        route: &ControllerOwnerRoute,
        controller_id: &str,
    ) -> Result<(), ForwardError> {
        let timeout = Duration::from_secs(10);
        let mut client = self.client(&route.endpoint, timeout).await?;
        let request = proto::ForwardEvictRequest {
            operation_id: uuid::Uuid::new_v4().to_string(),
            controller_id: controller_id.to_string(),
            target: Some(target(route)),
            hop_count: 1,
        };
        client
            .evict_local(tonic::Request::new(request))
            .await
            .map_err(from_status)?;
        Ok(())
    }
}

pub fn sanitize_headers(headers: HashMap<String, String>) -> HashMap<String, String> {
    const DENIED: &[&str] = &[
        "authorization",
        "cookie",
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "host",
    ];
    let nominated: std::collections::HashSet<String> = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("connection"))
        .flat_map(|(_, value)| value.split(','))
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect();
    headers
        .into_iter()
        .filter(|(name, _)| {
            let name = name.to_ascii_lowercase();
            !DENIED.contains(&name.as_str()) && !nominated.contains(&name)
        })
        .collect()
}

#[derive(Clone)]
pub struct InternalForwardingService {
    commander: Arc<Commander>,
    proxy: Arc<ProxyForwarder>,
    evictor: Arc<LocalControllerEvictor>,
    local_holder: String,
    accepting: Arc<AtomicBool>,
    max_request_bytes: usize,
    max_response_bytes: usize,
    expected_peer_spiffe_id: String,
}

enum ValidationError {
    Draining,
    Hop,
    MissingTarget,
    WrongTarget,
    InvalidFence,
    MissingPeer,
    InvalidPeer,
    DeniedPeer,
    RequestTooLarge,
}

impl ValidationError {
    fn status(self) -> tonic::Status {
        match self {
            Self::Draining => tonic::Status::unavailable("replica is draining"),
            Self::Hop => {
                tonic::Status::failed_precondition("internal forwarding is exactly one hop")
            }
            Self::MissingTarget => tonic::Status::invalid_argument("ownership target is required"),
            Self::WrongTarget => tonic::Status::failed_precondition("target replica changed"),
            Self::InvalidFence => tonic::Status::invalid_argument("invalid ownership fence"),
            Self::MissingPeer => {
                tonic::Status::unauthenticated("internal client certificate is required")
            }
            Self::InvalidPeer => {
                tonic::Status::permission_denied("internal client identity is invalid")
            }
            Self::DeniedPeer => {
                tonic::Status::permission_denied("internal client identity is not authorized")
            }
            Self::RequestTooLarge => {
                tonic::Status::resource_exhausted("internal forwarding request exceeds limit")
            }
        }
    }
}

fn command_dispatch_status(error: FencedCommandError) -> tonic::Status {
    match error {
        FencedCommandError::StaleOwnership => {
            tonic::Status::failed_precondition("controller ownership is stale")
        }
        FencedCommandError::Dispatch(_) => {
            tonic::Status::unavailable("controller dispatch result is uncertain")
        }
    }
}

fn proxy_dispatch_status(error: FencedProxyError) -> tonic::Status {
    match error {
        FencedProxyError::StaleOwnership => {
            tonic::Status::failed_precondition("controller ownership is stale")
        }
        FencedProxyError::Dispatch(_, _) => {
            tonic::Status::unavailable("controller dispatch result is uncertain")
        }
    }
}

impl InternalForwardingService {
    pub fn new(
        commander: Arc<Commander>,
        proxy: Arc<ProxyForwarder>,
        evictor: Arc<LocalControllerEvictor>,
        local_holder: String,
        max_request_bytes: usize,
        max_response_bytes: usize,
        expected_peer_spiffe_id: String,
    ) -> Self {
        Self {
            commander,
            proxy,
            evictor,
            local_holder,
            accepting: Arc::new(AtomicBool::new(true)),
            max_request_bytes,
            max_response_bytes,
            expected_peer_spiffe_id,
        }
    }

    pub fn stop_accepting(&self) {
        self.accepting.store(false, Ordering::Release);
    }

    fn validate_target(
        &self,
        hop_count: u32,
        target: Option<proto::OwnershipTarget>,
    ) -> Result<OwnershipFence, ValidationError> {
        if !self.accepting.load(Ordering::Acquire) {
            return Err(ValidationError::Draining);
        }
        if hop_count != 1 {
            return Err(ValidationError::Hop);
        }
        let target = target.ok_or(ValidationError::MissingTarget)?;
        if target.holder != self.local_holder {
            return Err(ValidationError::WrongTarget);
        }
        if target.fencing_token.is_empty() || target.fencing_epoch == 0 {
            return Err(ValidationError::InvalidFence);
        }
        Ok(OwnershipFence {
            token: target.fencing_token,
            epoch: target.fencing_epoch,
        })
    }

    fn validate_peer<T>(&self, request: &tonic::Request<T>) -> Result<(), ValidationError> {
        let leaf = request
            .peer_certs()
            .and_then(|certs| certs.first().cloned())
            .ok_or(ValidationError::MissingPeer)?;
        let actual = crate::federation::spiffe::extract_single_spiffe_uri(leaf.as_ref())
            .map_err(|_| ValidationError::InvalidPeer)?;
        if actual != self.expected_peer_spiffe_id {
            return Err(ValidationError::DeniedPeer);
        }
        Ok(())
    }

    fn validate_request_size<M: Message>(&self, request: &M) -> Result<(), ValidationError> {
        if request.encoded_len() > self.max_request_bytes {
            return Err(ValidationError::RequestTooLarge);
        }
        Ok(())
    }
}

#[tonic::async_trait]
impl proto::internal_forwarding_server::InternalForwarding for InternalForwardingService {
    async fn forward_command(
        &self,
        request: tonic::Request<proto::ForwardCommandRequest>,
    ) -> Result<tonic::Response<proto::ForwardPayload>, tonic::Status> {
        self.validate_peer(&request)
            .map_err(ValidationError::status)?;
        self.validate_request_size(request.get_ref())
            .map_err(ValidationError::status)?;
        let request = request.into_inner();
        let fence = self
            .validate_target(request.hop_count, request.target)
            .map_err(ValidationError::status)?;
        let command = crate::federation::proto::CommandRequest::decode(request.command.as_slice())
            .map_err(|_| tonic::Status::invalid_argument("command is invalid"))?
            .command
            .ok_or_else(|| tonic::Status::invalid_argument("command is required"))?;
        let response = self
            .commander
            .send_fenced_local_command(&request.controller_id, command, &self.local_holder, &fence)
            .await
            .map_err(command_dispatch_status)?;
        if response.encoded_len() > self.max_response_bytes {
            return Err(tonic::Status::resource_exhausted(
                "command response exceeds limit",
            ));
        }
        Ok(tonic::Response::new(proto::ForwardPayload {
            payload: response.encode_to_vec(),
        }))
    }

    async fn forward_http(
        &self,
        request: tonic::Request<proto::ForwardHttpRequest>,
    ) -> Result<tonic::Response<proto::ForwardPayload>, tonic::Status> {
        self.validate_peer(&request)
            .map_err(ValidationError::status)?;
        self.validate_request_size(request.get_ref())
            .map_err(ValidationError::status)?;
        let request = request.into_inner();
        let fence = self
            .validate_target(request.hop_count, request.target)
            .map_err(ValidationError::status)?;
        let controller_id = request.controller_id;
        let request =
            crate::federation::proto::HttpProxyRequest::decode(request.request.as_slice())
                .map_err(|_| tonic::Status::invalid_argument("proxy request is invalid"))?;
        if request.body.len() > self.max_request_bytes {
            return Err(tonic::Status::resource_exhausted(
                "proxy request exceeds limit",
            ));
        }
        let response = self
            .proxy
            .forward_fenced_local(
                &controller_id,
                ForwardHttpOperation {
                    method: request.method,
                    path: request.path,
                    headers: sanitize_headers(request.headers),
                    body: request.body,
                },
                &self.local_holder,
                &fence,
            )
            .await
            .map_err(proxy_dispatch_status)?;
        if response.body.len() > self.max_response_bytes {
            return Err(tonic::Status::resource_exhausted(
                "proxy response exceeds limit",
            ));
        }
        Ok(tonic::Response::new(proto::ForwardPayload {
            payload: response.encode_to_vec(),
        }))
    }

    async fn evict_local(
        &self,
        request: tonic::Request<proto::ForwardEvictRequest>,
    ) -> Result<tonic::Response<proto::ForwardPayload>, tonic::Status> {
        self.validate_peer(&request)
            .map_err(ValidationError::status)?;
        self.validate_request_size(request.get_ref())
            .map_err(ValidationError::status)?;
        let request = request.into_inner();
        let fence = self
            .validate_target(request.hop_count, request.target)
            .map_err(ValidationError::status)?;
        self.evictor
            .evict_fenced(&request.controller_id, &self.local_holder, &fence)
            .map_err(|error| tonic::Status::failed_precondition(error.to_string()))?;
        Ok(tonic::Response::new(proto::ForwardPayload {
            payload: Vec::new(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    #[test]
    fn credentials_and_hop_headers_are_removed() {
        let headers = HashMap::from([
            ("Authorization".to_string(), "Bearer secret".to_string()),
            ("Cookie".to_string(), "session=secret".to_string()),
            ("Connection".to_string(), "close, X-Hop".to_string()),
            ("X-Hop".to_string(), "remove-me".to_string()),
            ("content-type".to_string(), "application/json".to_string()),
        ]);
        let sanitized = sanitize_headers(headers);
        assert_eq!(sanitized.len(), 1);
        assert_eq!(
            sanitized.get("content-type").map(String::as_str),
            Some("application/json")
        );
    }

    #[test]
    fn only_pre_dispatch_stale_errors_are_retryable() {
        assert_eq!(
            command_dispatch_status(FencedCommandError::StaleOwnership).code(),
            tonic::Code::FailedPrecondition
        );
        assert_eq!(
            command_dispatch_status(FencedCommandError::Dispatch(anyhow::anyhow!("timeout")))
                .code(),
            tonic::Code::Unavailable
        );
        assert_eq!(
            proxy_dispatch_status(FencedProxyError::Dispatch(
                StatusCode::GATEWAY_TIMEOUT,
                "timeout".to_string(),
            ))
            .code(),
            tonic::Code::Unavailable
        );
    }

    #[test]
    fn complete_command_and_proxy_envelopes_obey_request_limit() {
        let registry = crate::federation::registry::ControllerRegistry::new();
        let commander = Arc::new(Commander::new(
            registry.clone(),
            Arc::new(Mutex::new(HashMap::new())),
            1,
        ));
        let proxy = Arc::new(ProxyForwarder::new(
            registry.clone(),
            Arc::new(Mutex::new(HashMap::new())),
            1,
        ));
        let metadata = Arc::new(crate::metadata_store::CenterMetaDataStore::new());
        let sync_client = Arc::new(crate::watch_cache::CenterSyncClient {
            plugin_metadata: crate::watch_cache::CenterWatchCacheRegistry::new(metadata),
        });
        let evictor = Arc::new(crate::eviction::LocalControllerEvictor::new(
            registry,
            Arc::new(crate::aggregator::ResourceAggregator::new()),
            sync_client,
        ));
        let service = InternalForwardingService::new(
            commander,
            proxy,
            evictor,
            "center-0/uid-0".to_string(),
            32,
            64,
            "spiffe://edgion.io/center".to_string(),
        );
        let command = proto::ForwardCommandRequest {
            operation_id: "x".repeat(64),
            ..Default::default()
        };
        let proxy = proto::ForwardHttpRequest {
            request: vec![0; 64],
            ..Default::default()
        };
        assert_eq!(
            service
                .validate_request_size(&command)
                .unwrap_err()
                .status()
                .code(),
            tonic::Code::ResourceExhausted
        );
        assert_eq!(
            service
                .validate_request_size(&proxy)
                .unwrap_err()
                .status()
                .code(),
            tonic::Code::ResourceExhausted
        );
    }
}
