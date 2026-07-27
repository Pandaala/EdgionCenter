//! gRPC server implementing FederationSync.
//!
//! On each connection:
//! 1. Wait up to 5s for first ControllerMessage (must be RegisterRequest)
//! 2. Register controller in registry
//! 3. Spawn heartbeat task (Ping every ping_interval)
//! 4. Loop: forward incoming messages to aggregator/proxy; forward outgoing to stream

use parking_lot::Mutex;
use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tonic::{Request, Response, Status, Streaming};
use uuid::Uuid;

use crate::aggregator::{ControllerInfo, ResourceAggregator};
use crate::federation::config::CenterSyncConfig;
use crate::federation::proto::{
    center_message::Payload as CenterPayload, controller_message::Payload as CtrlPayload,
    federation_sync_server::FederationSync, CenterMessage, ControllerMessage,
    FedWatchEventResponse, FedWatchListResponse, FedWatchRequest, Ping, RegisterAck,
    RegisterRequest,
};
use crate::federation::registry::{ControllerRegistry, SessionOwnership};
use crate::observe::fed_metrics;
use crate::proxy::PendingProxyMap;
use crate::watch_cache::{
    ApplyResult, CenterSyncClient, EventType, WatchEventSimple, WatchedConfigData,
    MAX_ENTRIES_PER_CONTROLLER,
};
use edgion_center_core::{
    AuditEvent, AuditWriter, ControllerDirectory, ControllerId, ControllerRegistration,
    ControllerRuntimeObservation, CoordinationRole, Coordinator, Leadership, OfflineOutcome,
    OwnershipFence, RenewalOutcome, SessionId,
};

/// Label value used on fed-sync metrics whose `kind` dimension is
/// currently hardcoded. The federation server only streams EdgionConfigData
/// today; when new kinds are added, each call site should pass its own
/// kind instead of this constant.
const PLUGIN_METADATA_KIND: &str = "EdgionConfigData";
const RUNTIME_PROJECTION_TIMEOUT: Duration = Duration::from_secs(2);

/// Upper bound for decoding one `ControllerMessage` from the federation
/// stream. Must exceed the Controller's 10 MiB proxied-response body cap
/// (plus envelope overhead): tonic's 4 MiB default tears down the entire
/// federation stream — heartbeat and watch included — on a single
/// oversized frame instead of failing only that request.
pub const MAX_FED_DECODE_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

/// Upper bound on the number of kinds carried in a single StatsReport's
/// `per_kind` map before the observation drops it rather than truncating.
const MAX_STATS_KINDS: usize = 64;
/// Upper bound on a single kind name's length in a StatsReport's `per_kind`
/// map before the observation drops it rather than truncating.
const MAX_STATS_KIND_LEN: usize = 63;

/// Validate a StatsReport's `per_kind` map against the ingest bounds and
/// convert it into the deterministic BTreeMap shape the CRD status stores.
///
/// This is fail-closed rather than truncating: an oversized or malformed map
/// would otherwise silently undercount some kinds, which is worse than
/// temporarily losing the per-kind breakdown while keeping the scalar total.
fn bounded_per_kind(
    per_kind: HashMap<String, u32>,
) -> Option<std::collections::BTreeMap<String, u32>> {
    if per_kind.len() > MAX_STATS_KINDS {
        tracing::warn!(
            component = "fed_server",
            bound = "MAX_STATS_KINDS",
            kinds = per_kind.len(),
            "StatsReport per_kind map exceeds the kind-count bound; dropping per-kind counts"
        );
        return None;
    }
    if let Some(offending_key) = per_kind.keys().find(|key| key.len() > MAX_STATS_KIND_LEN) {
        tracing::warn!(
            component = "fed_server",
            bound = "MAX_STATS_KIND_LEN",
            key = %offending_key,
            "StatsReport per_kind map has a kind name exceeding the length bound; dropping per-kind counts"
        );
        return None;
    }
    Some(per_kind.into_iter().collect())
}

#[derive(Clone)]
struct RuntimeProjector {
    directory: Option<Arc<dyn ControllerDirectory>>,
    slots: Arc<Mutex<HashMap<String, RuntimeProjectionSlot>>>,
    handle: RuntimeProjectionHandle,
}

#[derive(Clone, Default)]
pub struct RuntimeProjectionHandle {
    cancellation: tokio_util::sync::CancellationToken,
    tasks: OwnershipTaskTracker,
}

impl RuntimeProjectionHandle {
    pub fn stop(&self) {
        self.cancellation.cancel();
    }

    pub async fn wait(&self) {
        self.tasks.wait().await;
    }
}

struct RuntimeProjectionSlot {
    observation: ControllerRuntimeObservation,
    generation: u64,
}

impl RuntimeProjector {
    fn new(directory: Option<Arc<dyn ControllerDirectory>>) -> Self {
        Self {
            directory,
            slots: Arc::new(Mutex::new(HashMap::new())),
            handle: RuntimeProjectionHandle::default(),
        }
    }

    fn submit(&self, observation: ControllerRuntimeObservation) {
        let Some(directory) = self.directory.clone() else {
            return;
        };
        let key = observation.controller_id.to_string();
        let should_spawn = {
            let mut slots = self.slots.lock();
            match slots.get_mut(&key) {
                Some(slot) => {
                    if slot.observation.session_id == observation.session_id
                        && slot.observation.ownership_fence == observation.ownership_fence
                    {
                        if observation.sync_version.is_some() {
                            slot.observation.sync_version = observation.sync_version;
                        }
                        if observation.watch_server_id.is_some() {
                            slot.observation.watch_server_id = observation.watch_server_id;
                        }
                        if observation.resource_count.is_some() {
                            slot.observation.resource_count = observation.resource_count;
                        }
                        if observation.resource_counts_by_kind.is_some() {
                            slot.observation.resource_counts_by_kind =
                                observation.resource_counts_by_kind;
                        }
                        if observation.stats_updated_unix_ms.is_some() {
                            slot.observation.stats_updated_unix_ms =
                                observation.stats_updated_unix_ms;
                        }
                        if observation.watch_updated_unix_ms.is_some() {
                            slot.observation.watch_updated_unix_ms =
                                observation.watch_updated_unix_ms;
                        }
                        slot.observation.observed_at_unix_ms = slot
                            .observation
                            .observed_at_unix_ms
                            .max(observation.observed_at_unix_ms);
                        slot.generation = slot.generation.wrapping_add(1);
                    } else {
                        slot.observation = observation;
                        slot.generation = slot.generation.wrapping_add(1);
                    }
                    false
                }
                None => {
                    slots.insert(
                        key.clone(),
                        RuntimeProjectionSlot {
                            observation,
                            generation: 0,
                        },
                    );
                    true
                }
            }
        };
        if should_spawn {
            let slots = self.slots.clone();
            let cancellation = self.handle.cancellation.clone();
            self.handle.tasks.spawn(async move {
                let mut failures = 0_u32;
                loop {
                    let Some((observation, generation)) = slots
                        .lock()
                        .get(&key)
                        .map(|slot| (slot.observation.clone(), slot.generation))
                    else {
                        return;
                    };
                    let result = tokio::select! {
                        _ = cancellation.cancelled() => return,
                        result = tokio::time::timeout(
                            RUNTIME_PROJECTION_TIMEOUT,
                            directory.project_runtime(observation),
                        ) => result,
                    };
                    if matches!(result, Ok(Ok(_))) {
                        let mut guard = slots.lock();
                        if guard
                            .get(&key)
                            .is_some_and(|slot| slot.generation == generation)
                        {
                            guard.remove(&key);
                            return;
                        }
                        failures = 0;
                        continue;
                    }
                    failures = failures.saturating_add(1);
                    if failures == 1 || failures.is_power_of_two() {
                        tracing::warn!(
                            component = "fed_server",
                            controller_id = %key,
                            failures,
                            "Controller runtime projection will retry"
                        );
                    }
                    let exponent = failures.saturating_sub(1).min(5);
                    let base_ms = 250_u64.saturating_mul(1_u64 << exponent).min(8_000);
                    let jitter_ms = key.bytes().fold(0_u64, |sum, byte| sum + u64::from(byte)) % 97;
                    tokio::select! {
                        _ = cancellation.cancelled() => return,
                        _ = tokio::time::sleep(Duration::from_millis(base_ms + jitter_ms)) => {}
                    }
                }
            });
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn next_observed_at_ms() -> i64 {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let wall = now_ms();
    let mut previous = LAST.load(Ordering::Relaxed);
    loop {
        let next = wall.max(previous.saturating_add(1));
        match LAST.compare_exchange_weak(previous, next, Ordering::SeqCst, Ordering::Relaxed) {
            Ok(_) => return next.min(i64::MAX as u64) as i64,
            Err(actual) => previous = actual,
        }
    }
}

fn resource_key(value: &WatchedConfigData) -> Option<String> {
    let metadata = value.get("metadata")?.as_object()?;
    let name = metadata.get("name")?.as_str()?.trim();
    if name.is_empty() {
        return None;
    }
    Some(
        metadata
            .get("namespace")
            .and_then(serde_json::Value::as_str)
            .map_or_else(
                || name.to_string(),
                |namespace| format!("{namespace}/{name}"),
            ),
    )
}

// Boundary caps on RegisterRequest fields. The federation gRPC server requires
// mTLS, but input validation is kept independent of the TLS layer: any peer
// whose cert passes the mTLS handshake can still submit a RegisterRequest, so
// each field that becomes a HashMap key, an SQLite row, or an Admin API payload
// must be bounded here. Caps follow K8s label-value conventions (63 bytes per
// token, 253 bytes for full DNS-style identifiers) so legitimate controllers
// built from `cluster + name` are well below the limit.
const MAX_CONTROLLER_ID_LEN: usize = 253;
const MAX_CLUSTER_LEN: usize = 63;
const MAX_TAG_LEN: usize = 63;
const MAX_LIST_ITEMS: usize = 32;

/// Number of consecutive missed PINGs (no Pong received) before declaring the peer offline.
/// Three intervals allow one transient packet loss without spurious offline marking.
/// Per RFC 9113 §6.7 (informative).
const HEARTBEAT_MISSED_PING_BUDGET: u32 = 3;
const CONTROLLER_PROJECTION_TIMEOUT: Duration = Duration::from_secs(3);
const OFFLINE_PROJECTION_MAX_BACKOFF: Duration = Duration::from_secs(30);
const COORDINATION_OPERATION_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Default)]
struct OwnershipTasksInner {
    active: AtomicU64,
    completed: tokio::sync::Notify,
}

/// Tracks detached ownership maintainers so a process composition root can
/// wait until their final fenced Lease release has completed.
#[derive(Clone, Default)]
pub struct OwnershipTaskTracker {
    inner: Arc<OwnershipTasksInner>,
}

struct OwnershipTaskGuard(Arc<OwnershipTasksInner>);

impl Drop for OwnershipTaskGuard {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::AcqRel);
        self.0.completed.notify_waiters();
    }
}

impl OwnershipTaskTracker {
    fn spawn(&self, future: impl Future<Output = ()> + Send + 'static) {
        self.inner.active.fetch_add(1, Ordering::AcqRel);
        let inner = self.inner.clone();
        tokio::spawn(async move {
            let _guard = OwnershipTaskGuard(inner);
            future.await;
        });
    }

    pub async fn wait(&self) {
        loop {
            let completed = self.inner.completed.notified();
            if self.inner.active.load(Ordering::Acquire) == 0 {
                return;
            }
            completed.await;
        }
    }
}

fn unix_now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn unix_now_seconds() -> i64 {
    unix_now_millis() / 1_000
}

fn record_federation_audit(
    writer: Option<&Arc<dyn AuditWriter>>,
    controller_id: &str,
    session_id: &str,
    path: &str,
    status: i32,
    detail: Option<String>,
) {
    if let Some(writer) = writer {
        writer.record(AuditEvent {
            ts: unix_now_seconds(),
            actor: controller_id.to_string(),
            provider: "mtls".to_string(),
            method: "FEDERATION".to_string(),
            path: path.to_string(),
            target_controller: Some(controller_id.to_string()),
            status,
            source_ip: None,
            request_id: Some(session_id.to_string()),
            detail,
        });
    }
}

async fn release_ownership(coordinator: &Arc<dyn Coordinator>, leadership: &Leadership) {
    match tokio::time::timeout(
        COORDINATION_OPERATION_TIMEOUT,
        coordinator.release(leadership),
    )
    .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => tracing::warn!(
            component = "fed_server",
            %error,
            "Failed to release controller ownership Lease"
        ),
        Err(_) => tracing::warn!(
            component = "fed_server",
            "Timed out releasing controller ownership Lease"
        ),
    }
}

async fn maintain_ownership(
    coordinator: Arc<dyn Coordinator>,
    mut leadership: Leadership,
    session_cancel: tokio_util::sync::CancellationToken,
    ownership_lost: tokio_util::sync::CancellationToken,
    ownership_valid: Arc<AtomicBool>,
) {
    let mut valid_for = Duration::from_millis(leadership.valid_for_millis);
    let Some(mut deadline) = std::time::Instant::now().checked_add(valid_for) else {
        ownership_lost.cancel();
        ownership_valid.store(false, Ordering::SeqCst);
        session_cancel.cancel();
        release_ownership(&coordinator, &leadership).await;
        return;
    };
    loop {
        if valid_for.is_zero() {
            ownership_lost.cancel();
            ownership_valid.store(false, Ordering::SeqCst);
            session_cancel.cancel();
            break;
        }
        let renew_after = (valid_for / 3).max(Duration::from_millis(100));
        tokio::select! {
            _ = session_cancel.cancelled() => break,
            _ = tokio::time::sleep(renew_after) => {}
        }
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            ownership_lost.cancel();
            ownership_valid.store(false, Ordering::SeqCst);
            session_cancel.cancel();
            break;
        }
        let renew_timeout = COORDINATION_OPERATION_TIMEOUT.min(remaining);
        match tokio::time::timeout(renew_timeout, coordinator.renew(&leadership)).await {
            Ok(Ok(RenewalOutcome::Renewed(renewed))) => {
                valid_for = Duration::from_millis(renewed.valid_for_millis);
                let Some(renewed_deadline) = std::time::Instant::now().checked_add(valid_for)
                else {
                    ownership_lost.cancel();
                    ownership_valid.store(false, Ordering::SeqCst);
                    session_cancel.cancel();
                    break;
                };
                deadline = renewed_deadline;
                leadership = renewed;
            }
            Ok(Ok(RenewalOutcome::Lost)) => {
                tracing::warn!(
                    component = "fed_server",
                    holder = %leadership.holder,
                    "Controller ownership Lease was fenced; closing session"
                );
                ownership_lost.cancel();
                ownership_valid.store(false, Ordering::SeqCst);
                session_cancel.cancel();
                break;
            }
            Ok(Err(error)) => {
                tracing::warn!(
                    component = "fed_server",
                    %error,
                    "Controller ownership Lease renewal failed"
                );
            }
            Err(_) => tracing::warn!(
                component = "fed_server",
                "Controller ownership Lease renewal timed out"
            ),
        }
        if std::time::Instant::now() >= deadline {
            tracing::warn!(
                component = "fed_server",
                holder = %leadership.holder,
                "Controller ownership Lease expired without successful renewal"
            );
            ownership_lost.cancel();
            ownership_valid.store(false, Ordering::SeqCst);
            session_cancel.cancel();
            break;
        }
    }
    release_ownership(&coordinator, &leadership).await;
}

async fn project_offline_with_retry(
    directory: Arc<dyn ControllerDirectory>,
    controller_id: ControllerId,
    session_id: SessionId,
    ownership_fence: Option<OwnershipFence>,
    observed_at_unix_ms: i64,
    cancellation: tokio_util::sync::CancellationToken,
) {
    let mut delay = Duration::from_millis(100);
    let mut attempt = 1_u64;
    loop {
        let result = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return,
            result = tokio::time::timeout(
                CONTROLLER_PROJECTION_TIMEOUT,
                directory.mark_offline(
                    &controller_id,
                    &session_id,
                    ownership_fence.as_ref(),
                    observed_at_unix_ms,
                ),
            ) => result,
        };
        if let Ok(Ok(OfflineOutcome::Marked | OfflineOutcome::NotCurrent)) = result {
            return;
        }
        tracing::warn!(
            component = "fed_server",
            controller_id = %controller_id,
            session_id = %session_id,
            attempt,
            "Failed to project controller offline state; retrying"
        );
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => return,
            _ = tokio::time::sleep(delay) => {}
        }
        delay = delay.saturating_mul(2).min(OFFLINE_PROJECTION_MAX_BACKOFF);
        attempt = attempt.saturating_add(1);
    }
}

struct OfflineReconcilerInner {
    workers: Mutex<HashMap<String, (u64, tokio_util::sync::CancellationToken)>>,
    next_generation: AtomicU64,
}

impl Drop for OfflineReconcilerInner {
    fn drop(&mut self) {
        for (_, cancellation) in self.workers.get_mut().drain().map(|(_, worker)| worker) {
            cancellation.cancel();
        }
    }
}

/// One latest desired offline projection per controller. Reconnect churn
/// replaces and cancels the previous worker instead of accumulating one
/// infinite retry task per session during an API-server outage.
#[derive(Clone)]
struct OfflineReconciler(Arc<OfflineReconcilerInner>);

impl Default for OfflineReconciler {
    fn default() -> Self {
        Self(Arc::new(OfflineReconcilerInner {
            workers: Mutex::new(HashMap::new()),
            next_generation: AtomicU64::new(1),
        }))
    }
}

impl OfflineReconciler {
    fn cancel(&self, controller_id: &str) {
        if let Some((_, cancellation)) = self.0.workers.lock().remove(controller_id) {
            cancellation.cancel();
        }
    }

    fn schedule(
        &self,
        directory: Arc<dyn ControllerDirectory>,
        controller_id: ControllerId,
        session_id: SessionId,
        ownership_fence: Option<OwnershipFence>,
        observed_at_unix_ms: i64,
    ) {
        let generation = self.0.next_generation.fetch_add(1, Ordering::Relaxed);
        let cancellation = tokio_util::sync::CancellationToken::new();
        if let Some((_, previous)) = self.0.workers.lock().insert(
            controller_id.to_string(),
            (generation, cancellation.clone()),
        ) {
            previous.cancel();
        }
        let inner = Arc::downgrade(&self.0);
        let key = controller_id.to_string();
        tokio::spawn(async move {
            project_offline_with_retry(
                directory,
                controller_id,
                session_id,
                ownership_fence,
                observed_at_unix_ms,
                cancellation,
            )
            .await;
            if let Some(inner) = inner.upgrade() {
                let mut workers = inner.workers.lock();
                if workers
                    .get(&key)
                    .is_some_and(|(current, _)| *current == generation)
                {
                    workers.remove(&key);
                }
            }
        });
    }

    #[cfg(test)]
    fn worker_count(&self) -> usize {
        self.0.workers.lock().len()
    }
}

// Upper bound on total registered controllers (online + offline). Once
// this is reached, registration of a *new* controller_id is refused;
// reconnects from already-known ids still go through. Default is sized
// well above realistic enterprise federation deployments (typically tens
// to a few hundred controllers) but below the point where an attacker can
// inflate registry / aggregator / SQLite to dangerous sizes.
const MAX_REGISTRY_ENTRIES: usize = 10_000;

/// Reject a `RegisterRequest` whose fields are empty, oversized, or
/// contain control characters.
///
/// Returning a static `&'static str` keeps the rejection reason out of
/// the response body — the caller emits the reason via `tracing::warn!`
/// for ops, and surfaces a fixed `Status::invalid_argument` message to
/// the peer so we never echo attacker-controlled bytes back.
fn validate_register_req(req: &RegisterRequest) -> Result<(), &'static str> {
    if req.controller_id.is_empty() {
        return Err("controller_id is empty");
    }
    if req.controller_id.len() > MAX_CONTROLLER_ID_LEN {
        return Err("controller_id exceeds max length");
    }
    if req.controller_id.chars().any(|c| c.is_control()) {
        return Err("controller_id contains control characters");
    }
    if req.cluster.len() > MAX_CLUSTER_LEN {
        return Err("cluster exceeds max length");
    }
    if req.cluster.chars().any(|c| c.is_control()) {
        return Err("cluster contains control characters");
    }
    validate_string_list(&req.env, "env")?;
    validate_string_list(&req.tag, "tag")?;
    Ok(())
}

fn validate_string_list(items: &[String], field: &'static str) -> Result<(), &'static str> {
    if items.len() > MAX_LIST_ITEMS {
        return Err(match field {
            "env" => "env list exceeds max items",
            "tag" => "tag list exceeds max items",
            _ => "list exceeds max items",
        });
    }
    for item in items {
        if item.len() > MAX_TAG_LEN {
            return Err(match field {
                "env" => "env item exceeds max length",
                "tag" => "tag item exceeds max length",
                _ => "list item exceeds max length",
            });
        }
        if item.chars().any(|c| c.is_control()) {
            return Err(match field {
                "env" => "env item contains control characters",
                "tag" => "tag item contains control characters",
                _ => "list item contains control characters",
            });
        }
    }
    Ok(())
}

/// True when the registry is at capacity *and* the incoming id is not
/// already known. Reconnect of a known controller is always allowed —
/// only inflation by new ids is refused, which preserves operator
/// recovery during a flood while still blocking unbounded growth.
fn registry_capacity_exceeded(
    registry: &ControllerRegistry,
    incoming_id: &str,
    cap: usize,
) -> bool {
    registry.len() >= cap && registry.get_session(incoming_id).is_none()
}

/// Typed representation of a single watch event from the controller.
///
/// `data` is borrowed as `&RawValue` so the outer batch parse only slices the
/// array; each item's payload is deserialized lazily into its concrete type,
/// avoiding an intermediate `serde_json::Value` tree per event.
#[derive(serde::Deserialize)]
struct WatchEventRaw<'a> {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(borrow)]
    data: &'a serde_json::value::RawValue,
    #[serde(default)]
    #[allow(dead_code)]
    sync_version: u64,
}

/// Watch state for a controller session's single watched kind.
/// Only EdgionConfigData is watched today, tracked by one `FedWatchState`
/// instance (`pm_watch`) in the stream loop. Supporting multiple kinds would
/// require a `HashMap<kind, FedWatchState>` plus a per-kind match in the loop —
/// not yet implemented.
struct FedWatchState {
    /// Current request_id for correlation (stale responses are skipped).
    request_id: String,
    /// Controller's ConfigSyncServer instance ID (detects restarts).
    server_id: Option<String>,
    /// Consecutive error count (INFO on first, WARN on subsequent).
    consecutive_errors: u32,
    /// Set once a terminal error (e.g. RBAC denial) has stopped the watch.
    /// No further re-watch attempts are made once this is true.
    terminated: bool,
}

impl FedWatchState {
    fn new(request_id: String, server_id: Option<String>) -> Self {
        Self {
            request_id,
            server_id,
            consecutive_errors: 0,
            terminated: false,
        }
    }

    /// Generate a new FedWatchRequest (from_version=0) and update internal request_id.
    ///
    /// Calling this on a terminated state is a programming error: once the
    /// watch is terminated, nothing should attempt to re-watch it.
    fn re_watch(&mut self, kind: &str) -> CenterMessage {
        debug_assert!(!self.terminated);
        let new_id = Uuid::new_v4().to_string();
        self.request_id = new_id.clone();
        self.consecutive_errors = 0;
        CenterMessage {
            payload: Some(CenterPayload::WatchRequest(FedWatchRequest {
                request_id: new_id,
                kind: kind.to_string(),
                from_version: 0,
            })),
        }
    }

    /// Mark this watch as terminated. No further re-watch attempts should be
    /// made after this; the session must be re-registered (or RBAC fixed) to
    /// restore the watch.
    fn terminate(&mut self) {
        self.terminated = true;
    }

    /// Whether this watch has been terminated by a terminal error.
    fn is_terminated(&self) -> bool {
        self.terminated
    }
}

/// Build the initial `FedWatchRequest` (wrapped in a `CenterMessage`) sent
/// right after registration, resuming from the cached sync version.
fn initial_watch_request(request_id: &str, from_version: u64) -> CenterMessage {
    CenterMessage {
        payload: Some(CenterPayload::WatchRequest(FedWatchRequest {
            request_id: request_id.to_string(),
            kind: PLUGIN_METADATA_KIND.to_string(),
            from_version,
        })),
    }
}

/// Outcome returned by the synchronous watch-response handlers.
///
/// The loop inspects this to decide whether to trigger a backoff, re-watch,
/// or simply continue waiting for the next message.
#[derive(Debug, PartialEq, Eq)]
enum WatchOutcome {
    /// Response parsed and applied to the cache.
    Applied,
    /// Response skipped because its request_id is stale.
    Skipped,
    /// Response data could not be deserialized.
    ParseError,
    /// Server ID changed — re-watch from version 0 immediately.
    ReWatch,
    /// Error field set — back off 3 s then re-watch from version 0.
    BackoffReWatch,
    /// Error field set to a terminal condition (e.g. RBAC denial) — the
    /// watch has been stopped and will not be retried.
    Terminal,
    /// The batch would have exceeded `MAX_ENTRIES_PER_CONTROLLER` — nothing
    /// was applied and the watch has been stopped (treated like `Terminal`;
    /// no re-watch storm against a controller that keeps overflowing).
    Overflow,
}

/// Handle a `FedWatchListResponse` from the controller.
///
/// Synchronous — no `.await` inside. Stale-request and parse errors are
/// handled inline; callers do not need to take any further action for
/// `Skipped` or `ParseError`. On `Applied`, `pm_watch` state is updated.
fn apply_watch_list(
    cid: &str,
    pm_cache: &crate::watch_cache::CenterWatchCache<WatchedConfigData>,
    pm_watch: &mut FedWatchState,
    resp: FedWatchListResponse,
) -> WatchOutcome {
    if resp.request_id != pm_watch.request_id {
        tracing::debug!(
            component = "fed_server",
            controller_id = %cid,
            expected = %pm_watch.request_id,
            got = %resp.request_id,
            "Skipping stale WatchListResponse"
        );
        return WatchOutcome::Skipped;
    }

    // Watch already terminated by a prior terminal error — skip silently.
    // The request_id stays fixed after termination (Terminal never
    // re-watches), so a late frame under the same id must not be applied.
    if pm_watch.is_terminated() {
        return WatchOutcome::Skipped;
    }

    match serde_json::from_slice::<Vec<WatchedConfigData>>(&resp.data) {
        Ok(items) => {
            let total = items.len();
            // Validate the WHOLE batch before touching the cache: a single
            // keyless object or a duplicate key rejects everything, rather
            // than silently applying the valid subset and advancing the
            // sync version past data the cache never actually received.
            let mut keyed: Vec<(String, WatchedConfigData)> = Vec::with_capacity(total);
            let mut seen_keys: std::collections::HashSet<String> =
                std::collections::HashSet::with_capacity(total);
            let mut keyless_count = 0usize;
            let mut duplicate_count = 0usize;
            for item in items {
                match resource_key(&item) {
                    None => keyless_count += 1,
                    Some(key) => {
                        if !seen_keys.insert(key.clone()) {
                            duplicate_count += 1;
                        }
                        keyed.push((key, item));
                    }
                }
            }
            if keyless_count > 0 || duplicate_count > 0 {
                fed_metrics::record_watch_list(
                    PLUGIN_METADATA_KIND,
                    fed_metrics::labels::watch_list_result::PARSE_ERROR,
                );
                tracing::warn!(
                    component = "fed_server",
                    controller_id = %cid,
                    total_items = total,
                    keyless_count,
                    duplicate_count,
                    "Rejecting WatchListResponse batch: keyless or duplicate-keyed object present"
                );
                return WatchOutcome::ParseError;
            }
            match pm_cache.replace_all(keyed, resp.sync_version, resp.server_id.clone()) {
                ApplyResult::Overflow => {
                    fed_metrics::record_watch_error(
                        PLUGIN_METADATA_KIND,
                        fed_metrics::labels::watch_error_reason::OVERFLOW,
                    );
                    pm_watch.terminate();
                    tracing::warn!(
                        component = "fed_server",
                        controller_id = %cid,
                        max_entries_per_controller = MAX_ENTRIES_PER_CONTROLLER,
                        "federation watch cache exceeded MAX_ENTRIES_PER_CONTROLLER capacity; terminating watch (no re-watch)"
                    );
                    return WatchOutcome::Overflow;
                }
                ApplyResult::Applied => {}
            }
            pm_watch.server_id = Some(resp.server_id);
            pm_watch.consecutive_errors = 0;
            fed_metrics::record_watch_list(
                PLUGIN_METADATA_KIND,
                fed_metrics::labels::watch_list_result::OK,
            );
            tracing::debug!(
                component = "fed_server",
                controller_id = %cid,
                sync_version = resp.sync_version,
                "EdgionConfigData WatchListResponse applied"
            );
            WatchOutcome::Applied
        }
        Err(e) => {
            fed_metrics::record_watch_list(
                PLUGIN_METADATA_KIND,
                fed_metrics::labels::watch_list_result::PARSE_ERROR,
            );
            tracing::warn!(
                component = "fed_server",
                controller_id = %cid,
                error = %e,
                "Failed to deserialize WatchListResponse data"
            );
            WatchOutcome::ParseError
        }
    }
}

/// Handle a `FedWatchEventResponse` from the controller.
///
/// Synchronous — no `.await` inside. Returns `BackoffReWatch` or `ReWatch`
/// when the caller must re-issue a watch; the caller is responsible for
/// the backoff sleep and sending the new `FedWatchRequest`.
fn apply_watch_event(
    cid: &str,
    pm_cache: &crate::watch_cache::CenterWatchCache<WatchedConfigData>,
    pm_watch: &mut FedWatchState,
    resp: FedWatchEventResponse,
) -> WatchOutcome {
    // (1) Stale request_id — skip silently.
    if resp.request_id != pm_watch.request_id {
        tracing::debug!(
            component = "fed_server",
            controller_id = %cid,
            expected = %pm_watch.request_id,
            got = %resp.request_id,
            "Skipping stale WatchEventResponse"
        );
        return WatchOutcome::Skipped;
    }

    // (1b) Watch already terminated by a prior terminal error — skip
    // silently. Nothing should still be delivering frames for a terminated
    // watch, but if one arrives, do not resurrect the retry loop.
    if pm_watch.is_terminated() {
        return WatchOutcome::Skipped;
    }

    // (2) Record delivery metric (direction = recv from Center's perspective).
    fed_metrics::record_watch_event(PLUGIN_METADATA_KIND, fed_metrics::labels::direction::RECV);

    // (3) Error set — terminal errors stop the watch; everything else backs
    // off then re-watches.
    if !resp.error.is_empty() {
        if resp.error == "Forbidden" {
            fed_metrics::record_watch_error(
                PLUGIN_METADATA_KIND,
                fed_metrics::labels::watch_error_reason::TERMINAL,
            );
            pm_watch.terminate();
            pm_cache.set_stale();
            tracing::warn!(
                component = "fed_server",
                controller_id = %cid,
                "federation watch terminated by Controller RBAC; re-register or fix center.rbac to restore the watch"
            );
            return WatchOutcome::Terminal;
        }
        fed_metrics::record_watch_error(
            PLUGIN_METADATA_KIND,
            fed_metrics::labels::watch_error_reason::RECV_ERROR,
        );
        pm_watch.consecutive_errors += 1;
        if pm_watch.consecutive_errors == 1 {
            // First error is normal during startup/reload.
            tracing::info!(
                component = "fed_server",
                controller_id = %cid,
                error = %resp.error,
                "WatchEventResponse error (likely startup delay), backing off before re-watch"
            );
        } else {
            tracing::warn!(
                component = "fed_server",
                controller_id = %cid,
                error = %resp.error,
                consecutive_errors = pm_watch.consecutive_errors,
                "WatchEventResponse error persists, backing off before re-watch"
            );
        }
        return WatchOutcome::BackoffReWatch;
    }

    // (4) Server-ID mismatch — re-watch from version 0 immediately.
    if let Some(ref expected_sid) = pm_watch.server_id {
        if *expected_sid != resp.server_id {
            fed_metrics::record_watch_list(
                PLUGIN_METADATA_KIND,
                fed_metrics::labels::watch_list_result::VERSION_TOO_OLD,
            );
            tracing::warn!(
                component = "fed_server",
                controller_id = %cid,
                expected_server_id = %expected_sid,
                got_server_id = %resp.server_id,
                "Controller server_id changed, re-watching from 0"
            );
            return WatchOutcome::ReWatch;
        }
    }

    // (5) Parse and classify events. The WHOLE batch is validated up front:
    // any malformed body, unknown event type, or keyless object rejects the
    // entire batch (cache untouched) instead of applying the valid subset
    // and advancing the sync version past data the cache never received.
    let raw_events = match serde_json::from_str::<Vec<WatchEventRaw>>(&resp.data) {
        Ok(raw_events) => raw_events,
        Err(e) => {
            fed_metrics::record_watch_error(
                PLUGIN_METADATA_KIND,
                fed_metrics::labels::watch_error_reason::PARSE_ERROR,
            );
            tracing::warn!(
                component = "fed_server",
                controller_id = %cid,
                error = %e,
                "Failed to parse WatchEventResponse data"
            );
            return WatchOutcome::ParseError;
        }
    };

    let total = raw_events.len();
    let mut events = Vec::with_capacity(total);
    let mut unknown_type_count = 0usize;
    let mut keyless_count = 0usize;
    let mut body_parse_error_count = 0usize;
    for raw in raw_events {
        let event_type = match raw.event_type.as_str() {
            "add" => EventType::Add,
            "update" => EventType::Update,
            "delete" => EventType::Delete,
            _ => {
                unknown_type_count += 1;
                continue;
            }
        };
        match serde_json::from_str::<WatchedConfigData>(raw.data.get()) {
            Ok(item) => match resource_key(&item) {
                Some(key) => events.push(WatchEventSimple {
                    event_type,
                    key,
                    data: item,
                }),
                None => keyless_count += 1,
            },
            Err(_) => body_parse_error_count += 1,
        }
    }
    if unknown_type_count > 0 || keyless_count > 0 || body_parse_error_count > 0 {
        fed_metrics::record_watch_error(
            PLUGIN_METADATA_KIND,
            fed_metrics::labels::watch_error_reason::PARSE_ERROR,
        );
        tracing::warn!(
            component = "fed_server",
            controller_id = %cid,
            total_events = total,
            unknown_type_count,
            keyless_count,
            body_parse_error_count,
            "Rejecting WatchEventResponse batch: malformed or unknown-type event present"
        );
        return WatchOutcome::ParseError;
    }

    // (6) Version-gap check — only meaningful once the cache has actually
    // observed a version. A non-monotonic batch version means the cache has
    // missed or diverged from the controller's event stream; re-watch from 0
    // rather than applying data on top of a state we can no longer trust.
    let cache_version = pm_cache.get_sync_version();
    if cache_version > 0 && resp.sync_version <= cache_version {
        fed_metrics::record_watch_error(
            PLUGIN_METADATA_KIND,
            fed_metrics::labels::watch_error_reason::VERSION_GAP,
        );
        tracing::warn!(
            component = "fed_server",
            controller_id = %cid,
            cache_version,
            batch_version = resp.sync_version,
            "WatchEventResponse batch version is not monotonic, re-watching from 0"
        );
        return WatchOutcome::ReWatch;
    }

    if !events.is_empty() {
        match pm_cache.apply_events(events, resp.sync_version, resp.server_id.clone()) {
            ApplyResult::Overflow => {
                fed_metrics::record_watch_error(
                    PLUGIN_METADATA_KIND,
                    fed_metrics::labels::watch_error_reason::OVERFLOW,
                );
                pm_watch.terminate();
                tracing::warn!(
                    component = "fed_server",
                    controller_id = %cid,
                    max_entries_per_controller = MAX_ENTRIES_PER_CONTROLLER,
                    "federation watch cache exceeded MAX_ENTRIES_PER_CONTROLLER capacity; terminating watch (no re-watch)"
                );
                return WatchOutcome::Overflow;
            }
            ApplyResult::Applied => {}
        }
    }
    pm_watch.server_id = Some(resp.server_id);
    pm_watch.consecutive_errors = 0;
    tracing::debug!(
        component = "fed_server",
        controller_id = %cid,
        sync_version = resp.sync_version,
        "EdgionConfigData WatchEventResponse applied"
    );
    WatchOutcome::Applied
}

/// Sleep for the backoff duration, then send a re-watch request — unless the
/// session closes first, in which case the caller must stop the message loop.
///
/// Shared by both the list and event watch arms in the stream loop: both
/// `WatchOutcome::BackoffReWatch` (recoverable server-reported error) and
/// `WatchOutcome::ParseError` (malformed/non-atomic batch) resync the same
/// way, so they share this one implementation rather than duplicating the
/// `tokio::select!` backoff dance.
///
/// Returns `true` if the caller should `break` out of the message loop
/// (session closed during the backoff sleep), `false` otherwise.
async fn backoff_then_rewatch(
    cid: &str,
    heartbeat_cancel: &tokio_util::sync::CancellationToken,
    inner_tx: &mpsc::Sender<CenterMessage>,
    pm_watch: &mut FedWatchState,
) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(std::time::Duration::from_secs(3)) => {}
        _ = heartbeat_cancel.cancelled() => return true,
        _ = inner_tx.closed() => {
            tracing::debug!(
                component = "fed_server",
                controller_id = %cid,
                "Session closed during backoff, stopping re-watch"
            );
            return true;
        }
    }
    let re_watch_msg = pm_watch.re_watch(PLUGIN_METADATA_KIND);
    let _ = inner_tx.send(re_watch_msg).await;
    false
}

pub struct FederationGrpcServer {
    pub registry: ControllerRegistry,
    pub aggregator: Arc<ResourceAggregator>,
    pub pending_proxies: PendingProxyMap,
    pub sync_config: CenterSyncConfig,
    pub sync_client: Arc<CenterSyncClient>,
    /// Optional durable/queryable projection selected by the composition root.
    pub controller_directory: Option<Arc<dyn ControllerDirectory>>,
    /// SPIFFE trust domain for peer-identity binding (always enforced under mTLS).
    pub trust_domain: Option<String>,
    /// Optional runtime audit sink selected by the platform composition root.
    pub audit_writer: Option<Arc<dyn AuditWriter>>,
    /// Optional cross-replica stream ownership selected by Kubernetes mode.
    pub coordinator: Option<Arc<dyn Coordinator>>,
    offline_reconciler: OfflineReconciler,
    ownership_tasks: OwnershipTaskTracker,
    runtime_projector: RuntimeProjector,
}

impl FederationGrpcServer {
    pub fn new(
        registry: ControllerRegistry,
        aggregator: Arc<ResourceAggregator>,
        pending_proxies: PendingProxyMap,
        sync_config: CenterSyncConfig,
        sync_client: Arc<CenterSyncClient>,
        controller_directory: Option<Arc<dyn ControllerDirectory>>,
        trust_domain: Option<String>,
    ) -> Self {
        Self {
            registry,
            aggregator,
            pending_proxies,
            sync_config,
            sync_client,
            runtime_projector: RuntimeProjector::new(controller_directory.clone()),
            controller_directory,
            trust_domain,
            audit_writer: None,
            coordinator: None,
            offline_reconciler: OfflineReconciler::default(),
            ownership_tasks: OwnershipTaskTracker::default(),
        }
    }

    pub fn with_audit_writer(mut self, audit_writer: Arc<dyn AuditWriter>) -> Self {
        self.audit_writer = Some(audit_writer);
        self
    }

    pub fn with_coordinator(mut self, coordinator: Arc<dyn Coordinator>) -> Self {
        self.coordinator = Some(coordinator);
        self
    }

    pub fn ownership_tasks(&self) -> OwnershipTaskTracker {
        self.ownership_tasks.clone()
    }

    pub fn runtime_projection_handle(&self) -> RuntimeProjectionHandle {
        self.runtime_projector.handle.clone()
    }
}

#[tonic::async_trait]
impl FederationSync for FederationGrpcServer {
    type SyncStream = tokio_stream::wrappers::ReceiverStream<Result<CenterMessage, Status>>;

    async fn sync(
        &self,
        request: Request<Streaming<ControllerMessage>>,
    ) -> Result<Response<Self::SyncStream>, Status> {
        // peer_certs() must be read before into_inner() consumes the request.
        let peer_certs = request.peer_certs();
        let mut inbound = request.into_inner();
        let (out_tx, out_rx) = mpsc::channel::<Result<CenterMessage, Status>>(32);
        let (inner_tx, mut inner_rx) = mpsc::channel::<CenterMessage>(32);

        // 1. Wait for RegisterRequest (5s timeout)
        let first_msg = tokio::time::timeout(Duration::from_secs(5), inbound.message())
            .await
            .map_err(|_| {
                Status::deadline_exceeded("Registration timeout: no RegisterRequest within 5s")
            })?
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::cancelled("Stream closed before RegisterRequest"))?;

        let register_req = match first_msg.payload {
            Some(CtrlPayload::Register(r)) => r,
            _ => {
                return Err(Status::invalid_argument(
                    "First message must be RegisterRequest",
                ))
            }
        };

        // Boundary checks must run before any state mutation (registry,
        // aggregator, watch_cache, SQLite) so that a rejected request
        // leaves zero residue. Reasons are logged at warn level but the
        // peer-facing message is fixed to avoid echoing attacker input.
        if let Err(reason) = validate_register_req(&register_req) {
            tracing::warn!(
                component = "fed_server",
                reason = reason,
                controller_id_len = register_req.controller_id.len(),
                cluster_len = register_req.cluster.len(),
                env_len = register_req.env.len(),
                tag_len = register_req.tag.len(),
                "Rejected RegisterRequest: shape validation failed"
            );
            return Err(Status::invalid_argument(
                "RegisterRequest validation failed",
            ));
        }
        if registry_capacity_exceeded(
            &self.registry,
            &register_req.controller_id,
            MAX_REGISTRY_ENTRIES,
        ) {
            tracing::warn!(
                component = "fed_server",
                registry_len = self.registry.len(),
                cap = MAX_REGISTRY_ENTRIES,
                "Rejected RegisterRequest: registry at capacity"
            );
            return Err(Status::resource_exhausted(
                "Federation registry is at capacity",
            ));
        }

        // Peer-identity binding — always enforced (federation is mTLS-only).
        {
            use crate::observe::fed_metrics::labels::peer_identity_result as pir;
            let leaf = peer_certs.as_ref().and_then(|c| c.first());
            let Some(leaf) = leaf else {
                // Under mTLS the handshake guarantees a client cert; absence is
                // a defensive internal error, never an attacker path.
                return Err(Status::internal("missing client certificate under mTLS"));
            };
            let trust_domain = self.trust_domain.as_deref().unwrap_or_default();
            match crate::federation::spiffe::verify(
                leaf.as_ref(),
                trust_domain,
                &register_req.cluster,
                &register_req.controller_id,
            ) {
                Ok(()) => fed_metrics::record_peer_identity_check(pir::OK),
                Err(e) => {
                    use crate::federation::spiffe::PeerIdentityError as E;
                    let result = match e {
                        E::Mismatch => pir::MISMATCH,
                        E::NoSpiffeSan => pir::NO_SPIFFE_SAN,
                        E::MultiSan => pir::MULTI_SAN,
                        E::ParseError => pir::PARSE_ERROR,
                    };
                    fed_metrics::record_peer_identity_check(result);
                    tracing::warn!(
                        component = "fed_server",
                        result = result,
                        controller_id_len = register_req.controller_id.len(),
                        cluster_len = register_req.cluster.len(),
                        "Rejected RegisterRequest: peer identity check failed"
                    );
                    return Err(Status::permission_denied(
                        "peer identity verification failed",
                    ));
                }
            }
        }

        let controller_id = register_req.controller_id.clone();
        let session_id = Uuid::new_v4().to_string();
        let observed_at_unix_ms = next_observed_at_ms();
        let ownership = if let Some(coordinator) = &self.coordinator {
            // A reconnect can reacquire the same holder's Lease and rotate its
            // fence. Invalidate the old local stream before the API write so
            // there is no post-acquire window where local Admin traffic can
            // still dispatch through the superseded fence.
            self.registry.fence_owned_session(&controller_id);
            let role = CoordinationRole::ControllerOwner(controller_id.clone());
            match tokio::time::timeout(COORDINATION_OPERATION_TIMEOUT, coordinator.acquire(role))
                .await
            {
                Ok(Ok(leadership)) => Some(leadership),
                Ok(Err(error)) => {
                    tracing::warn!(
                        component = "fed_server",
                        controller_id = %controller_id,
                        %error,
                        "Controller ownership acquisition failed"
                    );
                    record_federation_audit(
                        self.audit_writer.as_ref(),
                        &controller_id,
                        &session_id,
                        "/federation/connect",
                        503,
                        Some("controller ownership unavailable".to_string()),
                    );
                    return Err(Status::unavailable("controller ownership unavailable"));
                }
                Err(_) => {
                    tracing::warn!(
                        component = "fed_server",
                        controller_id = %controller_id,
                        "Controller ownership acquisition timed out"
                    );
                    record_federation_audit(
                        self.audit_writer.as_ref(),
                        &controller_id,
                        &session_id,
                        "/federation/connect",
                        503,
                        Some("controller ownership timed out".to_string()),
                    );
                    return Err(Status::unavailable("controller ownership unavailable"));
                }
            }
        } else {
            None
        };
        let ownership_fence = ownership.as_ref().map(|leadership| OwnershipFence {
            token: leadership.fencing_token.clone(),
            epoch: leadership.fencing_epoch,
        });
        let ownership_valid = Arc::new(AtomicBool::new(true));

        tracing::info!(
            component = "fed_server",
            controller_id = %controller_id,
            session_id = %session_id,
            cluster = %register_req.cluster,
            "Controller registered"
        );

        // 2. Register in registry FIRST (atomic takeover), then aggregator + DB.
        let registration_outcome = self.registry.register_owned_cancellable(
            controller_id.clone(),
            register_req.clone(),
            inner_tx.clone(),
            session_id.clone(),
            ownership.as_ref().map(|leadership| SessionOwnership {
                holder: leadership.holder.clone(),
                fence: ownership_fence.clone().expect("ownership fence exists"),
                valid: ownership_valid.clone(),
            }),
        );
        let displaced_session_id = registration_outcome.displaced_session_id.clone();
        let connect_audit_detail = displaced_session_id
            .as_deref()
            .map(|displaced| format!("controller session takeover displaced {displaced}"))
            .unwrap_or_else(|| "controller session established".to_string());
        if displaced_session_id.is_some() {
            fed_metrics::record_session_takeover();
        }
        self.aggregator.set_controller_info(
            &controller_id,
            ControllerInfo {
                controller_id: register_req.controller_id.clone(),
                cluster: register_req.cluster.clone(),
                environments: register_req.env.clone(),
                tags: register_req.tag.clone(),
            },
        );
        // Project registration with a strict deadline before acknowledging it.
        // When a directory is configured, accepting an unprojected session would
        // leave management state claiming that a canceled predecessor is online.
        if let Some(directory) = &self.controller_directory {
            let directory = directory.clone();
            let registration = ControllerRegistration {
                controller_id: ControllerId::new(controller_id.clone())
                    .expect("validated controller id"),
                session_id: SessionId::new(session_id.clone()).expect("generated session id"),
                cluster: register_req.cluster.clone(),
                environments: register_req.env.clone(),
                tags: register_req.tag.clone(),
                connected_replica: ownership.as_ref().map(|lease| lease.holder.clone()),
                ownership_fence: ownership_fence.clone(),
                observed_at_unix_ms,
            };
            let cid = registration.controller_id.clone();
            match tokio::time::timeout(
                CONTROLLER_PROJECTION_TIMEOUT,
                directory.upsert_registration(registration),
            )
            .await
            {
                Ok(Ok(())) => self.offline_reconciler.cancel(&controller_id),
                result => {
                    let error = match result {
                        Ok(Err(error)) => error.to_string(),
                        Err(_) => "controller projection timed out".to_string(),
                        Ok(Ok(())) => unreachable!(),
                    };
                    self.registry.mark_offline(&controller_id, &session_id);
                    self.aggregator.mark_offline(&controller_id);

                    // The write may commit after its future times out. Retry a
                    // conditional offline projection so a late commit is fenced.
                    let current = SessionId::new(session_id.clone()).expect("generated session id");
                    self.offline_reconciler.schedule(
                        directory.clone(),
                        cid.clone(),
                        current,
                        ownership_fence.clone(),
                        observed_at_unix_ms,
                    );
                    tracing::warn!(
                        component = "fed_server",
                        controller_id = %cid,
                        error = %error,
                        "Controller registration rejected because projection failed"
                    );
                    record_federation_audit(
                        self.audit_writer.as_ref(),
                        &controller_id,
                        &session_id,
                        "/federation/connect",
                        503,
                        Some(format!(
                            "{connect_audit_detail}; projection failed: {error}"
                        )),
                    );
                    if let (Some(coordinator), Some(leadership)) =
                        (&self.coordinator, ownership.as_ref())
                    {
                        release_ownership(coordinator, leadership).await;
                    }
                    return Err(Status::unavailable(
                        "controller registration projection unavailable",
                    ));
                }
            }
        }

        // Federation connection metrics: record the connect event and refresh
        // the active-sessions gauge. `online_len` captures the number of
        // controllers with a live `stream_tx`, so it naturally drops on
        // mark_offline without us counting in two places.
        fed_metrics::record_connection_event(
            fed_metrics::labels::role::CENTER,
            fed_metrics::labels::event::CONNECTED,
        );
        fed_metrics::set_connections_active(
            fed_metrics::labels::role::CENTER,
            self.registry.online_len() as u64,
        );
        record_federation_audit(
            self.audit_writer.as_ref(),
            &controller_id,
            &session_id,
            "/federation/connect",
            200,
            Some(connect_audit_detail),
        );
        let session_started_at = std::time::Instant::now();

        // Send RegisterAck
        let _ = inner_tx
            .send(CenterMessage {
                payload: Some(CenterPayload::RegisterAck(RegisterAck {
                    session_id: session_id.clone(),
                })),
            })
            .await;

        // Send FedWatchRequest for EdgionConfigData
        let pm_cache = self
            .sync_client
            .plugin_metadata
            .get_or_create(&controller_id);
        let from_version = pm_cache.get_sync_version();
        let watch_request_id = Uuid::new_v4().to_string();

        let _ = inner_tx
            .send(initial_watch_request(&watch_request_id, from_version))
            .await;

        tracing::debug!(
            component = "fed_server",
            controller_id = %controller_id,
            request_id = %watch_request_id,
            from_version = from_version,
            "Sent FedWatchRequest for EdgionConfigData"
        );

        let registry = self.registry.clone();
        let aggregator = self.aggregator.clone();
        let pending_proxies = self.pending_proxies.clone();
        let sync_client = self.sync_client.clone();
        let directory_for_offline = self.controller_directory.clone();
        let runtime_projector = self.runtime_projector.clone();
        let offline_reconciler = self.offline_reconciler.clone();
        let ping_interval = Duration::from_secs(self.sync_config.ping_interval_secs);
        let heartbeat_timeout = ping_interval * HEARTBEAT_MISSED_PING_BUDGET;
        // Tracks the epoch-ms timestamp of the last received Pong. The heartbeat task reads
        // this to detect idle connections without wrapping message delivery in a timeout,
        // which would falsely fire on large in-flight WatchListResponse payloads.
        let last_pong_at = Arc::new(Mutex::new(Instant::now()));
        let heartbeat_cancel = registration_outcome.session_cancel;
        let ownership_lost = tokio_util::sync::CancellationToken::new();
        let cid = controller_id.clone();

        if let (Some(coordinator), Some(leadership)) = (self.coordinator.clone(), ownership.clone())
        {
            self.ownership_tasks.spawn(maintain_ownership(
                coordinator,
                leadership,
                heartbeat_cancel.clone(),
                ownership_lost.clone(),
                ownership_valid.clone(),
            ));
        }

        // 3. Forward inner_rx → out_tx
        tokio::spawn({
            let out_tx = out_tx.clone();
            async move {
                while let Some(msg) = inner_rx.recv().await {
                    if out_tx.send(Ok(msg)).await.is_err() {
                        break;
                    }
                }
            }
        });

        // 4. Heartbeat task
        tokio::spawn({
            let inner_tx = inner_tx.clone();
            let cid = cid.clone();
            let last_pong_at = last_pong_at.clone();
            let heartbeat_cancel = heartbeat_cancel.clone();
            async move {
                let mut interval = tokio::time::interval(ping_interval);
                interval.tick().await; // skip first
                loop {
                    tokio::select! {
                        _ = heartbeat_cancel.cancelled() => break,
                        _ = interval.tick() => {}
                    }
                    let now_ms = now_ms();
                    // Check Pong freshness before sending next Ping. Tracking last-pong-at
                    // rather than wrapping inbound.message() in a timeout avoids false offline
                    // declarations when a large WatchListResponse is in transit (RFC 9113 §6.7).
                    if last_pong_at.lock().elapsed() > heartbeat_timeout {
                        tracing::warn!(
                            component = "fed_server",
                            controller_id = %cid,
                            "Heartbeat timeout, marking offline"
                        );
                        heartbeat_cancel.cancel();
                        break;
                    }
                    let send_result = tokio::select! {
                        _ = heartbeat_cancel.cancelled() => break,
                        result = inner_tx.send(CenterMessage {
                            payload: Some(CenterPayload::Ping(Ping { timestamp: now_ms })),
                        }) => result,
                    };
                    if send_result.is_err() {
                        tracing::info!(
                            component = "fed_server",
                            controller_id = %cid,
                            "Heartbeat channel closed"
                        );
                        break;
                    }
                }
            }
        });

        // 5. Main message loop
        tokio::spawn({
            let registry = registry.clone();
            let aggregator_for_offline = aggregator.clone();
            let aggregator_for_stats = aggregator.clone();
            let cid = cid.clone();
            let sid = session_id.clone();
            let self_pending_proxies = pending_proxies.clone();
            let pm_cache = pm_cache.clone();
            let inner_tx = inner_tx.clone();
            let directory_for_offline = directory_for_offline.clone();
            let runtime_projector = runtime_projector.clone();
            let offline_reconciler = offline_reconciler.clone();
            let last_pong_at = last_pong_at.clone();
            let heartbeat_cancel = heartbeat_cancel.clone();
            let ownership_lost = ownership_lost.clone();
            let ownership_fence = ownership_fence.clone();
            let ownership_valid = ownership_valid.clone();
            let audit_writer = self.audit_writer.clone();
            async move {
                // Centralize the three disconnect cleanups (timeout / stream error / stream closed).
                // Each branch must propagate offline to all four state holders, which previously
                // diverged easily when new state was added; this closure keeps them in lockstep.
                // The `reason` carries through to `edgion_fed_mark_offline_total{reason}` and
                // gates metric emission so repeated calls (e.g., race between heartbeat timeout
                // and stream close) do not double-count.
                let mark_offline_all = |reason: &'static str| {
                    // Registry guard is the single takeover authority: if this
                    // task is stale (a newer session owns the id), do not touch
                    // aggregator / watch_cache / db / metrics at all.
                    let transitioned = registry.mark_offline(&cid, &sid);
                    if !transitioned {
                        return;
                    }
                    aggregator_for_offline.mark_offline(&cid);
                    sync_client.plugin_metadata.mark_offline(&cid);
                    if let Some(directory) = &directory_for_offline {
                        let directory = directory.clone();
                        let cid = ControllerId::new(cid.clone()).expect("validated controller id");
                        let sid = SessionId::new(sid.clone()).expect("generated session id");
                        offline_reconciler.schedule(
                            directory,
                            cid,
                            sid,
                            ownership_fence.clone(),
                            observed_at_unix_ms,
                        );
                    }
                    fed_metrics::record_mark_offline(reason);
                    fed_metrics::record_connection_event(
                        fed_metrics::labels::role::CENTER,
                        fed_metrics::labels::event::DISCONNECTED,
                    );
                    fed_metrics::record_connection_duration(
                        fed_metrics::labels::role::CENTER,
                        std::time::Instant::now()
                            .saturating_duration_since(session_started_at)
                            .as_secs_f64(),
                    );
                    fed_metrics::set_connections_active(
                        fed_metrics::labels::role::CENTER,
                        registry.online_len() as u64,
                    );
                    record_federation_audit(
                        audit_writer.as_ref(),
                        &cid,
                        &sid,
                        "/federation/disconnect",
                        200,
                        Some(reason.to_string()),
                    );
                };

                // Watch state tracking (local to this session)
                let mut pm_watch = FedWatchState::new(watch_request_id, {
                    let sid = pm_cache.get_server_id();
                    if sid.is_empty() {
                        None
                    } else {
                        Some(sid)
                    }
                });

                loop {
                    tokio::select! {
                        biased;
                        // Heartbeat task detected Pong silence > heartbeat_timeout.
                        _ = heartbeat_cancel.cancelled() => {
                            let reason = if ownership_lost.is_cancelled() {
                                fed_metrics::labels::offline_reason::OWNERSHIP_LOST
                            } else {
                                fed_metrics::labels::offline_reason::HEARTBEAT
                            };
                            mark_offline_all(reason);
                            break;
                        }
                        result = inbound.message() => {
                            match result {
                            Err(e) => {
                                tracing::info!(
                                    component = "fed_server",
                                    controller_id = %cid,
                                    error = %e,
                                    "Stream error"
                                );
                                mark_offline_all(fed_metrics::labels::offline_reason::DISCONNECT);
                                break;
                            }
                            Ok(None) => {
                                tracing::info!(
                                    component = "fed_server",
                                    controller_id = %cid,
                                    "Stream closed"
                                );
                                mark_offline_all(fed_metrics::labels::offline_reason::DISCONNECT);
                                break;
                            }
                            Ok(Some(msg)) => {
                                // Stop a stale (superseded) session before it can
                                // touch shared state — including refreshing the
                                // current session's last_seen.
                                if !ownership_valid.load(Ordering::SeqCst)
                                    || heartbeat_cancel.is_cancelled()
                                    || !registry.is_current_session(&cid, &sid)
                                {
                                    tracing::info!(
                                        component = "fed_server",
                                        controller_id = %cid,
                                        "Session superseded by takeover, stopping stale loop"
                                    );
                                    break;
                                }
                                // Re-check immediately before payload mutation;
                                // the biased select handles the both-ready case.
                                if !ownership_valid.load(Ordering::SeqCst)
                                    || heartbeat_cancel.is_cancelled()
                                {
                                    continue;
                                }
                                registry.update_last_seen(&cid);
                                if !ownership_valid.load(Ordering::SeqCst)
                                    || heartbeat_cancel.is_cancelled()
                                {
                                    continue;
                                }
                                match msg.payload {
                                    Some(CtrlPayload::Pong(_)) => {
                                        if !ownership_valid.load(Ordering::SeqCst) { continue; }
                                        *last_pong_at.lock() = Instant::now();
                                        runtime_projector.submit(ControllerRuntimeObservation {
                                            controller_id: ControllerId::new(cid.clone()).expect("validated controller id"),
                                            session_id: SessionId::new(sid.clone()).expect("generated session id"),
                                            ownership_fence: ownership_fence.clone(),
                                            sync_version: None,
                                            watch_server_id: None,
                                            resource_count: None,
                                            resource_counts_by_kind: None,
                                            stats_updated_unix_ms: None,
                                            watch_updated_unix_ms: None,
                                            observed_at_unix_ms: next_observed_at_ms(),
                                        });
                                    }
                                Some(CtrlPayload::HttpProxyResponse(resp)) => {
                                    if !ownership_valid.load(Ordering::SeqCst) { continue; }
                                    if let Some(tx) = self_pending_proxies.lock().remove(&resp.request_id) {
                                        let _ = tx.send(resp);
                                    }
                                }
                                Some(CtrlPayload::WatchListResponse(resp)) => {
                                    if !ownership_valid.load(Ordering::SeqCst) { continue; }
                                    let sync_version = resp.sync_version;
                                    let server_id = resp.server_id.clone();
                                    match apply_watch_list(&cid, &pm_cache, &mut pm_watch, resp) {
                                        WatchOutcome::Applied => {
                                            let updated_at = next_observed_at_ms();
                                            runtime_projector.submit(ControllerRuntimeObservation {
                                                    controller_id: ControllerId::new(cid.clone()).expect("validated controller id"),
                                                    session_id: SessionId::new(sid.clone()).expect("generated session id"),
                                                    ownership_fence: ownership_fence.clone(),
                                                    sync_version: Some(sync_version),
                                                    watch_server_id: Some(server_id),
                                                    resource_count: None,
                                                    resource_counts_by_kind: None,
                                                    stats_updated_unix_ms: None,
                                                    watch_updated_unix_ms: Some(updated_at),
                                                    observed_at_unix_ms: updated_at,
                                                });
                                        }
                                        WatchOutcome::ParseError
                                            if backoff_then_rewatch(&cid, &heartbeat_cancel, &inner_tx, &mut pm_watch).await =>
                                        {
                                            break;
                                        }
                                        WatchOutcome::Overflow => {
                                            // Watch stopped by capacity overflow (like
                                            // Terminal): no re-watch, no sleep — the
                                            // apply function already warned and
                                            // terminated the watch state.
                                        }
                                        _ => {}
                                    }
                                }
                                Some(CtrlPayload::WatchEventResponse(resp)) => {
                                    if !ownership_valid.load(Ordering::SeqCst) { continue; }
                                    let sync_version = resp.sync_version;
                                    let server_id = resp.server_id.clone();
                                    match apply_watch_event(&cid, &pm_cache, &mut pm_watch, resp) {
                                        WatchOutcome::BackoffReWatch | WatchOutcome::ParseError => {
                                            // Backoff before retrying to avoid tight loop.
                                            if backoff_then_rewatch(&cid, &heartbeat_cancel, &inner_tx, &mut pm_watch).await {
                                                break;
                                            }
                                        }
                                        WatchOutcome::ReWatch => {
                                            let re_watch_msg = pm_watch.re_watch(PLUGIN_METADATA_KIND);
                                            let _ = inner_tx.send(re_watch_msg).await;
                                        }
                                        WatchOutcome::Applied => {
                                            let updated_at = next_observed_at_ms();
                                            runtime_projector.submit(ControllerRuntimeObservation {
                                                    controller_id: ControllerId::new(cid.clone()).expect("validated controller id"),
                                                    session_id: SessionId::new(sid.clone()).expect("generated session id"),
                                                    ownership_fence: ownership_fence.clone(),
                                                    sync_version: Some(sync_version),
                                                    watch_server_id: Some(server_id),
                                                    resource_count: None,
                                                    resource_counts_by_kind: None,
                                                    stats_updated_unix_ms: None,
                                                    watch_updated_unix_ms: Some(updated_at),
                                                    observed_at_unix_ms: updated_at,
                                                });
                                        }
                                        WatchOutcome::Terminal => {
                                            // Watch stopped by a terminal error (e.g. RBAC
                                            // denial). No re-watch, no sleep: the session
                                            // must be re-registered to restore the watch.
                                        }
                                        WatchOutcome::Overflow => {
                                            // Watch stopped by capacity overflow (like
                                            // Terminal): no re-watch, no sleep — the
                                            // apply function already warned and
                                            // terminated the watch state.
                                        }
                                        _ => {}
                                    }
                                }
                                Some(CtrlPayload::StatsReport(report)) => {
                                    if !ownership_valid.load(Ordering::SeqCst) { continue; }
                                    // Push from Controller summarising per-kind resource counts.
                                    // The bound is computed ONCE here and the same value feeds
                                    // both consumers below, so neither can hold an unbounded
                                    // map: the aggregator (the fallback the API always uses in
                                    // standalone-SQL deployments, since the SQL adapter
                                    // hardcodes the directory record's stats fields to None)
                                    // and the durable runtime observation projected for the
                                    // CRD path.
                                    let resource_counts_by_kind = bounded_per_kind(report.per_kind);
                                    aggregator_for_stats.update_stats(
                                        &cid,
                                        resource_counts_by_kind.clone(),
                                        report.total as u64,
                                    );
                                    let updated_at = next_observed_at_ms();
                                    runtime_projector.submit(ControllerRuntimeObservation {
                                            controller_id: ControllerId::new(cid.clone()).expect("validated controller id"),
                                            session_id: SessionId::new(sid.clone()).expect("generated session id"),
                                            ownership_fence: ownership_fence.clone(),
                                            sync_version: None,
                                            watch_server_id: None,
                                            resource_count: Some(report.total as u64),
                                            resource_counts_by_kind,
                                            stats_updated_unix_ms: Some(updated_at),
                                            watch_updated_unix_ms: None,
                                            observed_at_unix_ms: updated_at,
                                        });
                                }
                                _ => {}
                            }
                        }
                        }  // match result
                    }      // result = inbound.message() => { ... }
                    } // tokio::select!
                } // loop
            } // async move
        });

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
            out_rx,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use edgion_center_core::{CoreResult, ReleaseOutcome};
    use prost::Message;
    use std::sync::atomic::AtomicUsize;

    #[derive(Default)]
    struct CapturingAudit(std::sync::Mutex<Vec<AuditEvent>>);

    impl AuditWriter for CapturingAudit {
        fn record(&self, event: AuditEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    struct LostCoordinator {
        releases: AtomicUsize,
    }

    struct BlockingReleaseCoordinator {
        release_started: tokio::sync::Notify,
        release_gate: tokio::sync::Notify,
    }

    #[tonic::async_trait]
    impl Coordinator for BlockingReleaseCoordinator {
        async fn acquire(&self, _: CoordinationRole) -> CoreResult<Leadership> {
            unreachable!()
        }
        async fn renew(&self, _: &Leadership) -> CoreResult<RenewalOutcome> {
            Ok(RenewalOutcome::Lost)
        }
        async fn release(&self, _: &Leadership) -> CoreResult<ReleaseOutcome> {
            self.release_started.notify_waiters();
            self.release_gate.notified().await;
            Ok(ReleaseOutcome::Released)
        }
    }

    #[tonic::async_trait]
    impl Coordinator for LostCoordinator {
        async fn acquire(&self, _role: CoordinationRole) -> CoreResult<Leadership> {
            unreachable!("test supplies an acquired leadership")
        }

        async fn renew(&self, _leadership: &Leadership) -> CoreResult<RenewalOutcome> {
            Ok(RenewalOutcome::Lost)
        }

        async fn release(&self, _leadership: &Leadership) -> CoreResult<ReleaseOutcome> {
            self.releases.fetch_add(1, Ordering::SeqCst);
            Ok(ReleaseOutcome::Lost)
        }
    }

    #[tokio::test]
    async fn lost_ownership_cancels_session_and_releases_once() {
        let coordinator = Arc::new(LostCoordinator {
            releases: AtomicUsize::new(0),
        });
        let coordinator_port: Arc<dyn Coordinator> = coordinator.clone();
        let cancellation = tokio_util::sync::CancellationToken::new();
        let ownership_lost = tokio_util::sync::CancellationToken::new();
        let ownership_valid = Arc::new(AtomicBool::new(true));
        let leadership = Leadership {
            role: CoordinationRole::ControllerOwner("c1".to_string()),
            holder: "center-0".to_string(),
            fencing_token: "token-1".to_string(),
            fencing_epoch: 1,
            valid_for_millis: 3_000,
        };
        tokio::time::timeout(
            Duration::from_secs(2),
            maintain_ownership(
                coordinator_port,
                leadership,
                cancellation.clone(),
                ownership_lost.clone(),
                ownership_valid.clone(),
            ),
        )
        .await
        .unwrap();
        assert!(cancellation.is_cancelled());
        assert!(ownership_lost.is_cancelled());
        assert!(!ownership_valid.load(Ordering::SeqCst));
        assert_eq!(coordinator.releases.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn ownership_tracker_waits_for_fenced_release_to_finish() {
        let coordinator = Arc::new(BlockingReleaseCoordinator {
            release_started: tokio::sync::Notify::new(),
            release_gate: tokio::sync::Notify::new(),
        });
        let release_started = coordinator.release_started.notified();
        let cancellation = tokio_util::sync::CancellationToken::new();
        cancellation.cancel();
        let tracker = OwnershipTaskTracker::default();
        tracker.spawn(maintain_ownership(
            coordinator.clone(),
            Leadership {
                role: CoordinationRole::ControllerOwner("c1".to_string()),
                holder: "center-0".to_string(),
                fencing_token: "token-1".to_string(),
                fencing_epoch: 1,
                valid_for_millis: 3_000,
            },
            cancellation,
            tokio_util::sync::CancellationToken::new(),
            Arc::new(AtomicBool::new(true)),
        ));
        release_started.await;
        assert!(
            tokio::time::timeout(Duration::from_millis(10), tracker.wait())
                .await
                .is_err(),
            "drain must remain pending while Lease release is blocked"
        );
        coordinator.release_gate.notify_one();
        tokio::time::timeout(Duration::from_secs(1), tracker.wait())
            .await
            .expect("drain should finish after Lease release");
    }

    #[tokio::test]
    async fn ownership_cancellation_wins_when_message_is_also_ready() {
        let cancellation = tokio_util::sync::CancellationToken::new();
        cancellation.cancel();
        let mut processed_message = false;
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => {}
            _ = async {} => processed_message = true,
        }
        assert!(!processed_message);
    }

    #[test]
    fn federation_runtime_audit_preserves_controller_session_and_reason() {
        let audit = Arc::new(CapturingAudit::default());
        let writer: Arc<dyn AuditWriter> = audit.clone();
        record_federation_audit(
            Some(&writer),
            "cluster-a/controller-0",
            "session-1",
            "/federation/disconnect",
            200,
            Some("heartbeat".to_string()),
        );
        let event = audit.0.lock().unwrap().pop().unwrap();
        assert_eq!(event.actor, "cluster-a/controller-0");
        assert_eq!(event.provider, "mtls");
        assert_eq!(event.request_id.as_deref(), Some("session-1"));
        assert_eq!(event.detail.as_deref(), Some("heartbeat"));
    }

    struct FlakyDirectory {
        failures_remaining: AtomicUsize,
        not_current_remaining: AtomicUsize,
        attempts: AtomicUsize,
    }

    #[tonic::async_trait]
    impl ControllerDirectory for FlakyDirectory {
        async fn upsert_registration(
            &self,
            _registration: ControllerRegistration,
        ) -> edgion_center_core::CoreResult<()> {
            Ok(())
        }

        async fn mark_offline(
            &self,
            _id: &ControllerId,
            _observed_session: &SessionId,
            _ownership_fence: Option<&OwnershipFence>,
            _observed_at_unix_ms: i64,
        ) -> edgion_center_core::CoreResult<OfflineOutcome> {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            if self
                .failures_remaining
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                Err(edgion_center_core::CoreError::Adapter("transient".into()))
            } else if self
                .not_current_remaining
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                Ok(OfflineOutcome::NotCurrent)
            } else {
                Ok(OfflineOutcome::Marked)
            }
        }

        async fn list(
            &self,
        ) -> edgion_center_core::CoreResult<Vec<edgion_center_core::ControllerRecord>> {
            Ok(Vec::new())
        }

        async fn evict(
            &self,
            _id: &ControllerId,
        ) -> edgion_center_core::CoreResult<edgion_center_core::EvictionResult> {
            Ok(edgion_center_core::EvictionResult {
                outcome: edgion_center_core::EvictionOutcome::AlreadyAbsent,
                target: None,
            })
        }
    }

    #[tokio::test]
    async fn offline_projection_retries_transient_failures() {
        let directory = Arc::new(FlakyDirectory {
            failures_remaining: AtomicUsize::new(2),
            not_current_remaining: AtomicUsize::new(0),
            attempts: AtomicUsize::new(0),
        });
        project_offline_with_retry(
            directory.clone(),
            ControllerId::new("c1").unwrap(),
            SessionId::new("s1").unwrap(),
            None,
            1,
            tokio_util::sync::CancellationToken::new(),
        )
        .await;
        assert_eq!(directory.attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn newer_durable_revision_ends_offline_reconciliation() {
        let directory = Arc::new(FlakyDirectory {
            failures_remaining: AtomicUsize::new(0),
            not_current_remaining: AtomicUsize::new(2),
            attempts: AtomicUsize::new(0),
        });
        project_offline_with_retry(
            directory.clone(),
            ControllerId::new("c1").unwrap(),
            SessionId::new("late-session").unwrap(),
            None,
            7,
            tokio_util::sync::CancellationToken::new(),
        )
        .await;
        assert_eq!(directory.attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn ambiguous_offline_retries_beyond_previous_attempt_limit() {
        let directory = Arc::new(FlakyDirectory {
            failures_remaining: AtomicUsize::new(5),
            not_current_remaining: AtomicUsize::new(0),
            attempts: AtomicUsize::new(0),
        });
        project_offline_with_retry(
            directory.clone(),
            ControllerId::new("c1").unwrap(),
            SessionId::new("late-session").unwrap(),
            None,
            9,
            tokio_util::sync::CancellationToken::new(),
        )
        .await;
        assert_eq!(directory.attempts.load(Ordering::SeqCst), 6);
    }

    struct ProjectionDirectory {
        failures_remaining: AtomicUsize,
        attempts: AtomicUsize,
        applied: std::sync::Mutex<Option<ControllerRuntimeObservation>>,
    }

    #[tonic::async_trait]
    impl ControllerDirectory for ProjectionDirectory {
        async fn upsert_registration(
            &self,
            _: ControllerRegistration,
        ) -> edgion_center_core::CoreResult<()> {
            unreachable!()
        }

        async fn mark_offline(
            &self,
            _: &ControllerId,
            _: &SessionId,
            _: Option<&OwnershipFence>,
            _: i64,
        ) -> edgion_center_core::CoreResult<OfflineOutcome> {
            unreachable!()
        }

        async fn list(
            &self,
        ) -> edgion_center_core::CoreResult<Vec<edgion_center_core::ControllerRecord>> {
            unreachable!()
        }

        async fn project_runtime(
            &self,
            observation: ControllerRuntimeObservation,
        ) -> edgion_center_core::CoreResult<bool> {
            self.attempts.fetch_add(1, Ordering::SeqCst);
            if self
                .failures_remaining
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                    remaining.checked_sub(1)
                })
                .is_ok()
            {
                return Err(edgion_center_core::CoreError::Adapter(
                    "transient".to_string(),
                ));
            }
            *self.applied.lock().unwrap() = Some(observation);
            Ok(true)
        }

        async fn evict(
            &self,
            _: &ControllerId,
        ) -> edgion_center_core::CoreResult<edgion_center_core::EvictionResult> {
            unreachable!()
        }
    }

    #[tokio::test]
    async fn runtime_projector_retries_and_coalesces_latest_observation() {
        let directory = Arc::new(ProjectionDirectory {
            failures_remaining: AtomicUsize::new(2),
            attempts: AtomicUsize::new(0),
            applied: std::sync::Mutex::new(None),
        });
        let projector = RuntimeProjector::new(Some(directory.clone()));
        let base = ControllerRuntimeObservation {
            controller_id: ControllerId::new("c1").unwrap(),
            session_id: SessionId::new("s1").unwrap(),
            ownership_fence: None,
            sync_version: Some(1),
            watch_server_id: Some("server-1".to_string()),
            resource_count: None,
            resource_counts_by_kind: None,
            stats_updated_unix_ms: None,
            watch_updated_unix_ms: Some(1),
            observed_at_unix_ms: 1,
        };
        projector.submit(base.clone());
        projector.submit(ControllerRuntimeObservation {
            sync_version: Some(2),
            watch_server_id: Some("server-2".to_string()),
            resource_count: Some(9),
            stats_updated_unix_ms: Some(2),
            watch_updated_unix_ms: Some(2),
            observed_at_unix_ms: 2,
            ..base
        });

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if directory.applied.lock().unwrap().is_some() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let applied = directory.applied.lock().unwrap().clone().unwrap();
        assert_eq!(applied.sync_version, Some(2));
        assert_eq!(applied.watch_server_id.as_deref(), Some("server-2"));
        assert_eq!(applied.resource_count, Some(9));
        assert_eq!(directory.attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn runtime_projector_shutdown_cancels_retry_workers() {
        let directory = Arc::new(ProjectionDirectory {
            failures_remaining: AtomicUsize::new(usize::MAX),
            attempts: AtomicUsize::new(0),
            applied: std::sync::Mutex::new(None),
        });
        let projector = RuntimeProjector::new(Some(directory.clone()));
        projector.submit(ControllerRuntimeObservation {
            controller_id: ControllerId::new("c1").unwrap(),
            session_id: SessionId::new("s1").unwrap(),
            ownership_fence: None,
            sync_version: Some(1),
            watch_server_id: Some("server-1".to_string()),
            resource_count: None,
            resource_counts_by_kind: None,
            stats_updated_unix_ms: None,
            watch_updated_unix_ms: Some(1),
            observed_at_unix_ms: 1,
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            while directory.attempts.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        projector.handle.stop();
        tokio::time::timeout(Duration::from_secs(1), projector.handle.wait())
            .await
            .unwrap();
        let stopped_at = directory.attempts.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(directory.attempts.load(Ordering::SeqCst), stopped_at);
    }

    #[tokio::test]
    async fn offline_reconciler_keeps_only_latest_worker_per_controller() {
        let directory = Arc::new(FlakyDirectory {
            failures_remaining: AtomicUsize::new(usize::MAX),
            not_current_remaining: AtomicUsize::new(0),
            attempts: AtomicUsize::new(0),
        });
        let reconciler = OfflineReconciler::default();
        for session in ["old", "new"] {
            reconciler.schedule(
                directory.clone(),
                ControllerId::new("c1").unwrap(),
                SessionId::new(session).unwrap(),
                None,
                1,
            );
        }
        assert_eq!(reconciler.worker_count(), 1);
        reconciler.cancel("c1");
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert_eq!(reconciler.worker_count(), 0);
    }

    fn ok_req() -> RegisterRequest {
        RegisterRequest {
            controller_id: "cluster-a/ctrl-01".to_string(),
            cluster: "cluster-a".to_string(),
            env: vec!["prod".to_string()],
            tag: vec!["region:us".to_string()],
        }
    }

    #[test]
    fn validate_accepts_well_formed_request() {
        assert!(validate_register_req(&ok_req()).is_ok());
    }

    #[test]
    fn validate_rejects_empty_controller_id() {
        let mut r = ok_req();
        r.controller_id = String::new();
        assert!(validate_register_req(&r).is_err());
    }

    #[test]
    fn validate_rejects_overlong_controller_id() {
        let mut r = ok_req();
        r.controller_id = "x".repeat(MAX_CONTROLLER_ID_LEN + 1);
        assert!(validate_register_req(&r).is_err());
    }

    #[test]
    fn validate_rejects_controller_id_with_control_char() {
        let mut r = ok_req();
        r.controller_id = "ctrl\n01".to_string();
        assert!(validate_register_req(&r).is_err());
    }

    #[test]
    fn validate_allows_empty_cluster() {
        // Aggregator already normalises empty cluster to "unknown"; do not
        // tighten this without auditing aggregator gauge semantics.
        let mut r = ok_req();
        r.cluster = String::new();
        assert!(validate_register_req(&r).is_ok());
    }

    #[test]
    fn validate_rejects_overlong_cluster() {
        let mut r = ok_req();
        r.cluster = "y".repeat(MAX_CLUSTER_LEN + 1);
        assert!(validate_register_req(&r).is_err());
    }

    #[test]
    fn validate_rejects_cluster_with_control_char() {
        let mut r = ok_req();
        r.cluster = "cluster\u{0}a".to_string();
        assert!(validate_register_req(&r).is_err());
    }

    #[test]
    fn validate_rejects_too_many_env_items() {
        let mut r = ok_req();
        r.env = (0..(MAX_LIST_ITEMS + 1)).map(|i| format!("e{i}")).collect();
        assert!(validate_register_req(&r).is_err());
    }

    #[test]
    fn validate_rejects_overlong_env_item() {
        let mut r = ok_req();
        r.env = vec!["z".repeat(MAX_TAG_LEN + 1)];
        assert!(validate_register_req(&r).is_err());
    }

    #[test]
    fn validate_rejects_env_item_with_control_char() {
        let mut r = ok_req();
        r.env = vec!["prod\tprime".to_string()];
        assert!(validate_register_req(&r).is_err());
    }

    #[test]
    fn validate_rejects_overlong_tag_item() {
        let mut r = ok_req();
        r.tag = vec!["t".repeat(MAX_TAG_LEN + 1)];
        assert!(validate_register_req(&r).is_err());
    }

    #[test]
    fn capacity_gate_allows_under_cap() {
        let reg = ControllerRegistry::new();
        assert!(!registry_capacity_exceeded(&reg, "new-id", 4));
    }

    #[test]
    fn capacity_gate_refuses_new_id_at_cap() {
        let reg = ControllerRegistry::new();
        for i in 0..4 {
            let (tx, _rx) = tokio::sync::mpsc::channel(1);
            reg.register(format!("c{i}"), ok_req(), tx, format!("s{i}"));
        }
        assert!(registry_capacity_exceeded(&reg, "brand-new", 4));
    }

    #[test]
    fn capacity_gate_allows_known_id_at_cap() {
        // Reconnects must continue to work even when the registry is full.
        let reg = ControllerRegistry::new();
        for i in 0..4 {
            let (tx, _rx) = tokio::sync::mpsc::channel(1);
            reg.register(format!("c{i}"), ok_req(), tx, format!("s{i}"));
        }
        assert!(!registry_capacity_exceeded(&reg, "c0", 4));
    }

    #[test]
    fn server_fields_default_trust_domain() {
        let reg = ControllerRegistry::new();
        let agg = std::sync::Arc::new(crate::aggregator::ResourceAggregator::new());
        let pp: crate::proxy::PendingProxyMap =
            std::sync::Arc::new(parking_lot::Mutex::new(std::collections::HashMap::new()));
        let store = std::sync::Arc::new(crate::metadata_store::CenterMetaDataStore::new());
        let sc = std::sync::Arc::new(crate::watch_cache::CenterSyncClient {
            plugin_metadata: crate::watch_cache::CenterWatchCacheRegistry::new(store),
        });
        let s = FederationGrpcServer::new(
            reg,
            agg,
            pp,
            crate::federation::config::CenterSyncConfig::default(),
            sc,
            None,
            None,
        );
        assert!(s.trust_domain.is_none());
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Helpers for apply_watch_list / apply_watch_event unit tests
    // ─────────────────────────────────────────────────────────────────────────

    /// Build a per-controller EdgionConfigData watch cache backed by the real
    /// CenterMetaDataStore handler (same construction used by the server).
    fn make_pm_cache() -> std::sync::Arc<crate::watch_cache::CenterWatchCache<WatchedConfigData>> {
        let store = std::sync::Arc::new(crate::metadata_store::CenterMetaDataStore::new());
        let registry =
            crate::watch_cache::CenterWatchCacheRegistry::<WatchedConfigData>::new(store);
        registry.get_or_create("test-ctrl")
    }

    /// Minimal EdgionConfigData JSON for one resource (KeyList variant).
    /// Wire format changed: kind is now "EdgionConfigData" and spec carries
    /// spec.data (ConfigEntry with type/config tags) instead of spec.metadata.
    fn pm_json(ns: &str, name: &str) -> String {
        serde_json::json!({
            "apiVersion": "edgion.io/v1",
            "kind": "EdgionConfigData",
            "metadata": {"namespace": ns, "name": name},
            "spec": {
                "enable": true,
                "data": {
                    "type": "KeyList",
                    "config": {
                        "items": [{"name": "g1", "items": [{"key": "k1"}]}]
                    }
                }
            }
        })
        .to_string()
    }

    /// JSON array bytes for a WatchListResponse.data field.
    fn list_json(items: &[(&str, &str)]) -> Vec<u8> {
        let pms: Vec<serde_json::Value> = items
            .iter()
            .map(|(ns, n)| serde_json::from_str(&pm_json(ns, n)).unwrap())
            .collect();
        serde_json::to_vec(&pms).unwrap()
    }

    /// JSON array string for a WatchEventResponse.data field.
    /// Each tuple is (event_type, namespace, name).
    fn event_json(events: &[(&str, &str, &str)], sync_version: u64) -> String {
        let evts: Vec<serde_json::Value> = events
            .iter()
            .map(|(t, ns, n)| {
                let data: serde_json::Value = serde_json::from_str(&pm_json(ns, n)).unwrap();
                serde_json::json!({"type": t, "data": data, "sync_version": sync_version})
            })
            .collect();
        serde_json::to_string(&evts).unwrap()
    }

    // ─────────────────────────────────────────────────────────────────────────
    // apply_watch_list tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn apply_watch_list_happy_path() {
        let pm_cache = make_pm_cache();
        let mut pm_watch = FedWatchState::new("req-1".to_string(), None);

        let resp = FedWatchListResponse {
            request_id: "req-1".to_string(),
            data: list_json(&[("default", "pm-a"), ("default", "pm-b")]),
            sync_version: 42,
            server_id: "srv-1".to_string(),
        };

        let outcome = apply_watch_list("test-ctrl", &pm_cache, &mut pm_watch, resp);

        assert_eq!(outcome, WatchOutcome::Applied);
        assert_eq!(pm_cache.get_sync_version(), 42);
        assert_eq!(pm_cache.get_server_id(), "srv-1");
        assert_eq!(pm_watch.server_id, Some("srv-1".to_string()));
        assert_eq!(pm_watch.consecutive_errors, 0);
    }

    #[test]
    fn apply_watch_list_skips_stale_request_id() {
        let pm_cache = make_pm_cache();
        let mut pm_watch = FedWatchState::new("req-current".to_string(), None);

        let resp = FedWatchListResponse {
            request_id: "req-stale".to_string(),
            data: list_json(&[("default", "pm-a")]),
            sync_version: 99,
            server_id: "srv-1".to_string(),
        };

        let outcome = apply_watch_list("test-ctrl", &pm_cache, &mut pm_watch, resp);

        assert_eq!(outcome, WatchOutcome::Skipped);
        assert_eq!(
            pm_cache.get_sync_version(),
            0,
            "cache must be untouched on stale request"
        );
    }

    #[test]
    fn apply_watch_list_skips_on_terminated_state() {
        let pm_cache = make_pm_cache();
        let mut pm_watch = FedWatchState::new("req-1".to_string(), None);
        pm_watch.terminate();

        // A valid, matching-request_id list frame arrives after termination.
        let resp = FedWatchListResponse {
            request_id: "req-1".to_string(),
            data: list_json(&[("default", "pm-a")]),
            sync_version: 42,
            server_id: "srv-1".to_string(),
        };

        let outcome = apply_watch_list("test-ctrl", &pm_cache, &mut pm_watch, resp);

        assert_eq!(outcome, WatchOutcome::Skipped);
        assert_eq!(
            pm_cache.get_sync_version(),
            0,
            "cache must not be updated by a frame on a terminated watch"
        );
    }

    #[test]
    fn apply_watch_list_parse_error() {
        let pm_cache = make_pm_cache();
        let mut pm_watch = FedWatchState::new("req-1".to_string(), None);

        let resp = FedWatchListResponse {
            request_id: "req-1".to_string(),
            data: b"this is not valid json".to_vec(),
            sync_version: 1,
            server_id: "srv-1".to_string(),
        };

        let outcome = apply_watch_list("test-ctrl", &pm_cache, &mut pm_watch, resp);

        assert_eq!(outcome, WatchOutcome::ParseError);
        assert_eq!(
            pm_cache.get_sync_version(),
            0,
            "cache must be untouched on parse error"
        );
    }

    #[test]
    fn apply_watch_list_invalid_utf8_is_parse_error() {
        let pm_cache = make_pm_cache();
        let mut pm_watch = FedWatchState::new("req-1".to_string(), None);

        let resp = FedWatchListResponse {
            request_id: "req-1".to_string(),
            data: vec![0xff, 0xfe, 0xfd],
            sync_version: 1,
            server_id: "srv-1".to_string(),
        };

        let outcome = apply_watch_list("test-ctrl", &pm_cache, &mut pm_watch, resp);

        assert_eq!(outcome, WatchOutcome::ParseError);
        assert_eq!(
            pm_cache.get_sync_version(),
            0,
            "cache must be untouched on invalid UTF-8"
        );
    }

    #[test]
    fn apply_watch_list_rejects_batch_with_keyless_object() {
        let pm_cache = make_pm_cache();
        let mut pm_watch = FedWatchState::new("req-1".to_string(), None);

        let valid: serde_json::Value = serde_json::from_str(&pm_json("default", "pm-a")).unwrap();
        let keyless = serde_json::json!({
            "apiVersion": "edgion.io/v1",
            "kind": "EdgionConfigData",
            "metadata": {"namespace": "default"},
            "spec": {
                "enable": true,
                "data": {
                    "type": "KeyList",
                    "config": {"items": [{"name": "g1", "items": [{"key": "k1"}]}]}
                }
            }
        });
        let data = serde_json::to_vec(&vec![valid, keyless]).unwrap();

        let resp = FedWatchListResponse {
            request_id: "req-1".to_string(),
            data,
            sync_version: 42,
            server_id: "srv-1".to_string(),
        };

        let outcome = apply_watch_list("test-ctrl", &pm_cache, &mut pm_watch, resp);

        assert_eq!(outcome, WatchOutcome::ParseError);
        assert_eq!(
            pm_cache.get_sync_version(),
            0,
            "cache must be untouched when a batch member is keyless"
        );
        assert!(
            pm_cache.snapshot_keys().is_empty(),
            "no entries must be applied from a rejected batch"
        );
    }

    #[test]
    fn apply_watch_list_rejects_duplicate_keys() {
        let pm_cache = make_pm_cache();
        let mut pm_watch = FedWatchState::new("req-1".to_string(), None);

        // Two docs that resolve to the same resource_key ("default/pm-a").
        let data = list_json(&[("default", "pm-a"), ("default", "pm-a")]);

        let resp = FedWatchListResponse {
            request_id: "req-1".to_string(),
            data,
            sync_version: 42,
            server_id: "srv-1".to_string(),
        };

        let outcome = apply_watch_list("test-ctrl", &pm_cache, &mut pm_watch, resp);

        assert_eq!(outcome, WatchOutcome::ParseError);
        assert_eq!(
            pm_cache.get_sync_version(),
            0,
            "cache must be untouched when the batch has a duplicate key"
        );
    }

    #[test]
    fn watch_list_response_prost_round_trip_preserves_data_bytes() {
        let expected = FedWatchListResponse {
            request_id: "req-1".to_string(),
            data: vec![0x00, 0xff, b'{', b'}'],
            sync_version: 42,
            server_id: "srv-1".to_string(),
        };

        let encoded = expected.encode_to_vec();
        let decoded = FedWatchListResponse::decode(encoded.as_slice()).unwrap();

        assert_eq!(decoded, expected);
        assert_eq!(decoded.data, vec![0x00, 0xff, b'{', b'}']);
    }

    #[test]
    fn fed_decode_limit_covers_controller_proxy_response_cap() {
        // Controller-side cap: 10 MiB response body (fed_client MAX body read),
        // plus headroom for headers/envelope. See the spec, section 2 (F2).
        const CONTROLLER_PROXY_RESPONSE_CAP: usize = 10 * 1024 * 1024;
        const { assert!(MAX_FED_DECODE_MESSAGE_BYTES >= CONTROLLER_PROXY_RESPONSE_CAP + 64 * 1024) };
    }

    // ─────────────────────────────────────────────────────────────────────────
    // apply_watch_event tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn apply_watch_event_happy_path_classifies_types() {
        let pm_cache = make_pm_cache();
        // Seed the cache so update and delete have existing targets.
        let keyed = vec![
            (
                "default/seed1".to_string(),
                serde_json::from_str::<WatchedConfigData>(&pm_json("default", "seed1")).unwrap(),
            ),
            (
                "default/seed2".to_string(),
                serde_json::from_str::<WatchedConfigData>(&pm_json("default", "seed2")).unwrap(),
            ),
        ];
        pm_cache.replace_all(keyed, 1, "srv-1".to_string());

        let mut pm_watch = FedWatchState::new("req-1".to_string(), Some("srv-1".to_string()));

        let resp = FedWatchEventResponse {
            request_id: "req-1".to_string(),
            data: event_json(
                &[
                    ("add", "default", "new-pm"),
                    ("update", "default", "seed1"),
                    ("delete", "default", "seed2"),
                ],
                2,
            ),
            sync_version: 2,
            server_id: "srv-1".to_string(),
            error: String::new(),
        };

        let outcome = apply_watch_event("test-ctrl", &pm_cache, &mut pm_watch, resp);

        assert_eq!(outcome, WatchOutcome::Applied);
        assert_eq!(pm_cache.get_sync_version(), 2);
        assert_eq!(pm_watch.server_id, Some("srv-1".to_string()));
        assert_eq!(pm_watch.consecutive_errors, 0);

        // Verify that each event type was classified correctly via observable cache state.
        //
        // Seed had 2 keys (seed1, seed2).  After: +1 add (new-pm), 0 net for update (seed1),
        // -1 delete (seed2) → exactly 2 keys must remain.
        //
        // A misclassification would break this:
        //   • delete treated as update → seed2 stays → 3 keys
        //   • update treated as delete → seed1 removed → 1 key
        //   • add treated as delete (no-op on absent key) → new-pm absent → 1 key (or seed2 absent too)
        let keys = pm_cache.snapshot_keys();
        assert_eq!(
            keys.len(),
            2,
            "expected exactly 2 keys after add+update+delete; got: {:?}",
            keys
        );

        // Add: "default/new-pm" must have been inserted.
        assert!(
            pm_cache.get_entry("default/new-pm").is_some(),
            "add event must insert default/new-pm"
        );
        // Update: "default/seed1" must still be present (not deleted).
        assert!(
            pm_cache.get_entry("default/seed1").is_some(),
            "update event must keep default/seed1 in cache"
        );
        // Delete: "default/seed2" must have been removed.
        assert!(
            pm_cache.get_entry("default/seed2").is_none(),
            "delete event must remove default/seed2 from cache"
        );
    }

    #[test]
    fn apply_watch_event_skips_stale() {
        let pm_cache = make_pm_cache();
        let mut pm_watch = FedWatchState::new("req-current".to_string(), None);

        let resp = FedWatchEventResponse {
            request_id: "req-stale".to_string(),
            data: event_json(&[("add", "default", "pm-a")], 1),
            sync_version: 1,
            server_id: "srv-1".to_string(),
            error: String::new(),
        };

        let outcome = apply_watch_event("test-ctrl", &pm_cache, &mut pm_watch, resp);

        assert_eq!(outcome, WatchOutcome::Skipped);
        assert_eq!(
            pm_cache.get_sync_version(),
            0,
            "cache must be untouched on stale request"
        );
    }

    #[test]
    fn apply_watch_event_server_id_change_rewatches() {
        let pm_cache = make_pm_cache();
        let mut pm_watch = FedWatchState::new("req-1".to_string(), Some("srv-old".to_string()));

        let resp = FedWatchEventResponse {
            request_id: "req-1".to_string(),
            data: event_json(&[("add", "default", "pm-a")], 1),
            sync_version: 1,
            server_id: "srv-new".to_string(), // differs from expected "srv-old"
            error: String::new(),
        };

        let outcome = apply_watch_event("test-ctrl", &pm_cache, &mut pm_watch, resp);

        assert_eq!(outcome, WatchOutcome::ReWatch);
        assert_eq!(
            pm_cache.get_sync_version(),
            0,
            "data must not be applied on server_id mismatch"
        );
    }

    #[test]
    fn apply_watch_event_error_backoff_rewatch() {
        let pm_cache = make_pm_cache();
        let mut pm_watch = FedWatchState::new("req-1".to_string(), None);

        let make_err_resp = || FedWatchEventResponse {
            request_id: "req-1".to_string(),
            data: String::new(),
            sync_version: 0,
            server_id: String::new(),
            error: "WATCH_ERROR".to_string(),
        };

        // First error: consecutive_errors 0 → 1.
        let outcome1 = apply_watch_event("test-ctrl", &pm_cache, &mut pm_watch, make_err_resp());
        assert_eq!(outcome1, WatchOutcome::BackoffReWatch);
        assert_eq!(pm_watch.consecutive_errors, 1);

        // Second error without intervening re_watch reset: 1 → 2.
        let outcome2 = apply_watch_event("test-ctrl", &pm_cache, &mut pm_watch, make_err_resp());
        assert_eq!(outcome2, WatchOutcome::BackoffReWatch);
        assert_eq!(pm_watch.consecutive_errors, 2);
    }

    #[test]
    fn apply_watch_event_forbidden_is_terminal() {
        let pm_cache = make_pm_cache();
        let mut pm_watch = FedWatchState::new("req-1".to_string(), None);

        let make_forbidden_resp = || FedWatchEventResponse {
            request_id: "req-1".to_string(),
            data: String::new(),
            sync_version: 0,
            server_id: String::new(),
            error: "Forbidden".to_string(),
        };

        // Forbidden terminates the watch instead of backing off.
        let outcome1 =
            apply_watch_event("test-ctrl", &pm_cache, &mut pm_watch, make_forbidden_resp());
        assert_eq!(outcome1, WatchOutcome::Terminal);
        assert!(pm_watch.is_terminated());

        // A second frame arriving after termination is skipped, not retried.
        let outcome2 =
            apply_watch_event("test-ctrl", &pm_cache, &mut pm_watch, make_forbidden_resp());
        assert_eq!(outcome2, WatchOutcome::Skipped);
    }

    #[test]
    fn forbidden_terminal_marks_cache_stale() {
        let pm_cache = make_pm_cache();
        let mut pm_watch = FedWatchState::new("req-1".to_string(), None);

        let forbidden_resp = FedWatchEventResponse {
            request_id: "req-1".to_string(),
            data: String::new(),
            sync_version: 0,
            server_id: String::new(),
            error: "Forbidden".to_string(),
        };

        assert!(!pm_cache.status().stale, "cache must start fresh");

        let outcome = apply_watch_event("test-ctrl", &pm_cache, &mut pm_watch, forbidden_resp);
        assert_eq!(outcome, WatchOutcome::Terminal);
        assert!(pm_watch.is_terminated());
        assert!(
            pm_cache.status().stale,
            "a terminated (Forbidden) watch must mark its cache stale so freshness reporting is honest"
        );
    }

    #[test]
    fn terminal_watch_state_never_rewatches() {
        // After FedWatchState::terminate(), is_terminated() is true and the
        // stream loop contract is: no re_watch message is minted.
        let mut state = FedWatchState::new("r1".to_string(), None);
        state.terminate();
        assert!(state.is_terminated());
    }

    #[test]
    fn initial_watch_request_resumes_from_cached_version() {
        let msg = initial_watch_request("req-1", 42);
        let Some(CenterPayload::WatchRequest(req)) = msg.payload else {
            panic!("expected WatchRequest");
        };
        assert_eq!(req.request_id, "req-1");
        assert_eq!(req.kind, PLUGIN_METADATA_KIND);
        assert_eq!(req.from_version, 42);
    }

    #[test]
    fn apply_watch_event_parse_error() {
        let pm_cache = make_pm_cache();
        let mut pm_watch = FedWatchState::new("req-1".to_string(), None);

        let resp = FedWatchEventResponse {
            request_id: "req-1".to_string(),
            data: "not a valid json array".to_string(),
            sync_version: 1,
            server_id: "srv-1".to_string(),
            error: String::new(),
        };

        let outcome = apply_watch_event("test-ctrl", &pm_cache, &mut pm_watch, resp);

        assert_eq!(outcome, WatchOutcome::ParseError);
    }

    #[test]
    fn apply_watch_event_unknown_type_rejects_batch() {
        let pm_cache = make_pm_cache();
        let mut pm_watch = FedWatchState::new("req-1".to_string(), None);

        // One unknown event type plus one otherwise-valid sibling add event.
        let resp = FedWatchEventResponse {
            request_id: "req-1".to_string(),
            data: event_json(
                &[
                    ("bogus_type", "default", "pm-x"),
                    ("add", "default", "pm-valid"),
                ],
                1,
            ),
            sync_version: 1,
            server_id: "srv-1".to_string(),
            error: String::new(),
        };

        let outcome = apply_watch_event("test-ctrl", &pm_cache, &mut pm_watch, resp);

        // The whole batch is rejected: an unknown event type anywhere in the
        // batch means nothing is applied, including the valid sibling event.
        assert_eq!(outcome, WatchOutcome::ParseError);
        assert_eq!(
            pm_cache.get_sync_version(),
            0,
            "cache must be untouched when the batch has an unknown event type"
        );
        assert!(
            pm_cache.get_entry("default/pm-valid").is_none(),
            "valid sibling event must not be applied when the batch is rejected"
        );
        assert!(
            pm_cache.get_entry("default/pm-x").is_none(),
            "unknown-type event must not insert default/pm-x"
        );
    }

    #[test]
    fn apply_watch_event_rejects_partial_batch() {
        let pm_cache = make_pm_cache();
        let mut pm_watch = FedWatchState::new("req-1".to_string(), None);

        // Seed the cache via a valid list at version 10.
        let seed_resp = FedWatchListResponse {
            request_id: "req-1".to_string(),
            data: list_json(&[("default", "seed1")]),
            sync_version: 10,
            server_id: "srv-1".to_string(),
        };
        assert_eq!(
            apply_watch_list("test-ctrl", &pm_cache, &mut pm_watch, seed_resp),
            WatchOutcome::Applied
        );

        // Event batch: one valid add + one keyless event, at version 11.
        let valid_add = serde_json::json!({
            "type": "add",
            "data": serde_json::from_str::<serde_json::Value>(&pm_json("default", "new-pm")).unwrap(),
            "sync_version": 11
        });
        let keyless_doc = serde_json::json!({
            "apiVersion": "edgion.io/v1",
            "kind": "EdgionConfigData",
            "metadata": {"namespace": "default"},
            "spec": {
                "enable": true,
                "data": {"type": "KeyList", "config": {"items": []}}
            }
        });
        let keyless_event = serde_json::json!({
            "type": "add",
            "data": keyless_doc,
            "sync_version": 11
        });
        let data = serde_json::to_string(&vec![valid_add, keyless_event]).unwrap();

        let resp = FedWatchEventResponse {
            request_id: "req-1".to_string(),
            data,
            sync_version: 11,
            server_id: "srv-1".to_string(),
            error: String::new(),
        };

        let outcome = apply_watch_event("test-ctrl", &pm_cache, &mut pm_watch, resp);

        assert_eq!(outcome, WatchOutcome::ParseError);
        assert_eq!(
            pm_cache.get_sync_version(),
            10,
            "cache must stay at the seeded version when the batch is rejected"
        );
        let keys = pm_cache.snapshot_keys();
        assert_eq!(
            keys,
            vec!["default/seed1".to_string()],
            "only the seeded entry must remain"
        );
    }

    #[test]
    fn apply_watch_event_version_gap_rewatches() {
        let pm_cache = make_pm_cache();
        let mut pm_watch = FedWatchState::new("req-1".to_string(), None);

        // Seed the cache via a valid list at version 20.
        let seed_resp = FedWatchListResponse {
            request_id: "req-1".to_string(),
            data: list_json(&[("default", "seed1")]),
            sync_version: 20,
            server_id: "srv-1".to_string(),
        };
        assert_eq!(
            apply_watch_list("test-ctrl", &pm_cache, &mut pm_watch, seed_resp),
            WatchOutcome::Applied
        );

        // Event batch carrying a non-monotonic sync_version (equal to the
        // cache's current version).
        let resp = FedWatchEventResponse {
            request_id: "req-1".to_string(),
            data: event_json(&[("add", "default", "new-pm")], 20),
            sync_version: 20,
            server_id: "srv-1".to_string(),
            error: String::new(),
        };

        let outcome = apply_watch_event("test-ctrl", &pm_cache, &mut pm_watch, resp);

        assert_eq!(outcome, WatchOutcome::ReWatch);
        assert_eq!(
            pm_cache.get_sync_version(),
            20,
            "cache must stay at the seeded version on a version gap"
        );
        assert!(
            pm_cache.get_entry("default/new-pm").is_none(),
            "event must not be applied on a version gap"
        );
    }

    #[test]
    fn takeover_then_stale_offline_keeps_controller_online() {
        use crate::aggregator::ResourceAggregator;
        let registry = ControllerRegistry::new();
        let aggregator = std::sync::Arc::new(ResourceAggregator::new());

        // Use a RegisterRequest whose controller_id matches the registry key so
        // that aggregator.controller_summaries() can find it by controller_id.
        let cid_req = RegisterRequest {
            controller_id: "cid".to_string(),
            cluster: "cluster-a".to_string(),
            env: vec!["prod".to_string()],
            tag: vec!["region:us".to_string()],
        };

        // New session s2 is authoritative.
        let (tx1, _rx1) = tokio::sync::mpsc::channel(8);
        registry.register("cid".to_string(), cid_req.clone(), tx1, "s1".to_string());
        let aggregate_info = ControllerInfo {
            controller_id: cid_req.controller_id.clone(),
            cluster: cid_req.cluster.clone(),
            environments: cid_req.env.clone(),
            tags: cid_req.tag.clone(),
        };
        aggregator.set_controller_info("cid", aggregate_info.clone());
        let (tx2, _rx2) = tokio::sync::mpsc::channel(8);
        registry.register("cid".to_string(), cid_req.clone(), tx2, "s2".to_string());
        aggregator.set_controller_info("cid", aggregate_info);

        // Old task (s1) runs the guarded offline path.
        let transitioned = registry.mark_offline("cid", "s1");
        if transitioned {
            aggregator.mark_offline("cid");
        }
        assert!(!transitioned, "stale s1 must not transition");
        let online = aggregator
            .controller_summaries()
            .iter()
            .find(|s| s.controller_id == "cid")
            .map(|s| s.online)
            .unwrap_or(false);
        assert!(online, "controller must stay online after stale offline");
    }

    #[test]
    fn bounded_per_kind_accepts_a_normal_report() {
        let mut per_kind = HashMap::new();
        per_kind.insert("EdgionConfigData".to_string(), 5u32);
        per_kind.insert("EdgionRoute".to_string(), 3u32);
        per_kind.insert("EdgionUpstream".to_string(), 1u32);

        let bounded = bounded_per_kind(per_kind).expect("normal report must be accepted");

        let mut expected = std::collections::BTreeMap::new();
        expected.insert("EdgionConfigData".to_string(), 5u32);
        expected.insert("EdgionRoute".to_string(), 3u32);
        expected.insert("EdgionUpstream".to_string(), 1u32);
        assert_eq!(bounded, expected);
    }

    #[test]
    fn bounded_per_kind_rejects_oversized_maps_instead_of_truncating() {
        let mut too_many_kinds = HashMap::new();
        for index in 0..(MAX_STATS_KINDS + 1) {
            too_many_kinds.insert(format!("kind-{index}"), 1u32);
        }
        assert_eq!(
            bounded_per_kind(too_many_kinds),
            None,
            "a map with more than MAX_STATS_KINDS entries must be dropped, not truncated"
        );

        let mut key_too_long = HashMap::new();
        key_too_long.insert("k".repeat(MAX_STATS_KIND_LEN + 1), 1u32);
        assert_eq!(
            bounded_per_kind(key_too_long),
            None,
            "a key longer than MAX_STATS_KIND_LEN must be dropped, not truncated"
        );
    }

    /// The bound must be computed ONCE at ingest and fed to both consumers:
    /// the aggregator (the fallback path the API always uses in
    /// standalone-SQL deployments, since the SQL adapter hardcodes the
    /// directory record's stats fields to None) and the durable runtime
    /// observation stored for the CRD path. An oversized map must be DROPPED
    /// on both, never truncated, while the scalar total keeps flowing.
    #[test]
    fn oversized_per_kind_is_dropped_on_both_ingest_paths() {
        let aggregator = ResourceAggregator::new();
        aggregator.set_controller_info(
            "cid",
            ControllerInfo {
                controller_id: "cid".to_string(),
                cluster: "cluster-a".to_string(),
                environments: vec![],
                tags: vec![],
            },
        );

        let mut oversized = HashMap::new();
        for index in 0..(MAX_STATS_KINDS + 1) {
            oversized.insert(format!("kind-{index}"), 1u32);
        }
        let total = oversized.len() as u64;

        // This mirrors exactly what the StatsReport arm in `handle` does:
        // bound once, then feed the SAME value into both the aggregator
        // (fallback path) and what would become the runtime observation
        // (CRD path) — never the raw, unbounded report.per_kind.
        let bounded_for_observation = bounded_per_kind(oversized);
        aggregator.update_stats("cid", bounded_for_observation.clone(), total);

        assert_eq!(
            bounded_for_observation, None,
            "the same bound applied to the observation must also reject the oversized map"
        );

        let summaries = aggregator.controller_summaries();
        let summary = summaries
            .iter()
            .find(|s| s.controller_id == "cid")
            .expect("controller present");
        assert_eq!(
            summary.per_kind, None,
            "an oversized per_kind map must be dropped, never truncated or exposed"
        );
        assert_eq!(
            summary.key_count,
            Some(total),
            "the scalar total must still flow even when the per-kind map is dropped"
        );
    }
}
