//! ControllerRegistry: manages per-controller sessions and stream handles.

use crate::federation::proto::{CenterMessage, RegisterRequest};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub struct ControllerSession {
    pub controller_id: String,
    pub session_id: String,
    pub info: RegisterRequest,
    pub stream_tx: Option<mpsc::Sender<CenterMessage>>,
    pub last_seen: Instant,
    pub offline_since: Option<Instant>,
    session_cancel: CancellationToken,
    ownership: Option<SessionOwnership>,
}

#[derive(Debug, Clone)]
pub struct SessionOwnership {
    pub holder: String,
    pub fence: edgion_center_core::OwnershipFence,
    pub valid: Arc<AtomicBool>,
}

impl SessionOwnership {
    pub fn is_valid(&self) -> bool {
        self.valid.load(Ordering::Acquire)
    }
}

impl ControllerSession {
    pub fn is_online(&self) -> bool {
        self.stream_tx.is_some() && !self.session_cancel.is_cancelled()
    }
}

#[derive(Clone)]
pub struct ControllerRegistry {
    inner: Arc<RwLock<HashMap<String, ControllerSession>>>,
    metrics: Arc<dyn RegistryMetrics>,
}

pub struct RegistrationOutcome {
    pub displaced_session_id: Option<String>,
    pub session_cancel: CancellationToken,
}

/// Session lifecycle metrics supplied by the process composition root.
pub trait RegistryMetrics: Send + Sync {
    fn record_session_reentry(&self);
    fn record_eviction(&self);
}

struct NoopRegistryMetrics;

impl RegistryMetrics for NoopRegistryMetrics {
    fn record_session_reentry(&self) {}
    fn record_eviction(&self) {}
}

impl ControllerRegistry {
    pub fn new() -> Self {
        Self::with_metrics(Arc::new(NoopRegistryMetrics))
    }

    pub fn with_metrics(metrics: Arc<dyn RegistryMetrics>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            metrics,
        }
    }

    pub fn register(
        &self,
        controller_id: String,
        info: RegisterRequest,
        stream_tx: mpsc::Sender<CenterMessage>,
        session_id: String,
    ) -> Option<String> {
        self.register_cancellable(controller_id, info, stream_tx, session_id)
            .displaced_session_id
    }

    /// Atomically install a session and cancel any live session it replaces.
    pub fn register_cancellable(
        &self,
        controller_id: String,
        info: RegisterRequest,
        stream_tx: mpsc::Sender<CenterMessage>,
        session_id: String,
    ) -> RegistrationOutcome {
        self.register_owned_cancellable(controller_id, info, stream_tx, session_id, None)
    }

    /// Atomically install a session together with its platform ownership fence.
    pub fn register_owned_cancellable(
        &self,
        controller_id: String,
        info: RegisterRequest,
        stream_tx: mpsc::Sender<CenterMessage>,
        session_id: String,
        ownership: Option<SessionOwnership>,
    ) -> RegistrationOutcome {
        let session_cancel = CancellationToken::new();
        let session = ControllerSession {
            controller_id: controller_id.clone(),
            session_id,
            info,
            stream_tx: Some(stream_tx),
            last_seen: Instant::now(),
            offline_since: None,
            session_cancel: session_cancel.clone(),
            ownership,
        };
        let mut guard = self.inner.write();
        // Capture a displaced *live* session id (takeover). An entry that is
        // already offline (stream_tx == None) is not a live takeover.
        let displaced = guard.get(&controller_id).and_then(|s| {
            if s.stream_tx.is_some() {
                s.session_cancel.cancel();
                Some(s.session_id.clone())
            } else {
                None
            }
        });
        // Detect same controller_id re-registering while a session entry still
        // exists — a signal for reconnect storms / session races. Covers both
        // the online->online case and the offline->online (recovered) case.
        if guard.contains_key(&controller_id) {
            self.metrics.record_session_reentry();
        }
        guard.insert(controller_id, session);
        RegistrationOutcome {
            displaced_session_id: displaced,
            session_cancel,
        }
    }

    /// True iff `controller_id` currently maps to a session with this
    /// `session_id`. Stale tasks use this to stop touching shared state after
    /// a takeover.
    pub fn is_current_session(&self, controller_id: &str, session_id: &str) -> bool {
        self.inner
            .read()
            .get(controller_id)
            .is_some_and(|s| s.session_id == session_id)
    }

    pub fn get_session(&self, controller_id: &str) -> Option<SessionView> {
        let map = self.inner.read();
        let session = map.get(controller_id)?;
        Some(SessionView {
            controller_id: session.controller_id.clone(),
            session_id: session.session_id.clone(),
            info: session.info.clone(),
            stream_tx: if session.is_online() {
                session.stream_tx.clone()
            } else {
                None
            },
            last_seen: session.last_seen,
            offline_since: session.offline_since,
            session_cancel: session.session_cancel.clone(),
            ownership: session.ownership.clone(),
        })
    }

    /// Fence a locally owned session before attempting a same-replica Lease
    /// reacquire. This closes the interval in which Kubernetes has committed a
    /// newer fence but the displaced stream has not reached its next renewal.
    pub fn fence_owned_session(&self, controller_id: &str) -> bool {
        let guard = self.inner.read();
        let Some(session) = guard.get(controller_id) else {
            return false;
        };
        let Some(ownership) = session.ownership.as_ref() else {
            return false;
        };
        ownership.valid.store(false, Ordering::Release);
        session.session_cancel.cancel();
        true
    }

    /// Seconds elapsed since the controller's last_seen, computed inside the
    /// read lock so callers needing only liveness don't clone the whole
    /// SessionView (info vectors, cluster string, stream_tx). None if unknown.
    pub fn last_seen_secs_ago(&self, controller_id: &str) -> Option<u64> {
        self.inner
            .read()
            .get(controller_id)
            .map(|s| s.last_seen.elapsed().as_secs())
    }

    pub fn update_last_seen(&self, controller_id: &str) {
        if let Some(session) = self.inner.write().get_mut(controller_id) {
            session.last_seen = Instant::now();
        }
    }

    /// Cancel every live transport session during process shutdown.
    ///
    /// Session handlers retain responsibility for the normal offline
    /// projection and coordinator release path; this only delivers the shared
    /// cancellation signal and immediately hides their senders from callers.
    pub fn cancel_all(&self) -> usize {
        let mut cancelled = 0;
        for session in self.inner.write().values_mut() {
            if session.is_online() {
                session.session_cancel.cancel();
                session.stream_tx.take();
                cancelled += 1;
            }
        }
        cancelled
    }

    /// Mark the session offline and release its stream sender.
    ///
    /// Returns true if the transition actually happened. Returns false when
    /// the session is already offline, the controller is not known, or the
    /// `expected_session_id` does not match the currently registered session
    /// (a stale task raced with a reconnect — it must not touch the new
    /// session).
    pub fn mark_offline(&self, controller_id: &str, expected_session_id: &str) -> bool {
        let mut guard = self.inner.write();
        let Some(session) = guard.get_mut(controller_id) else {
            return false;
        };
        if session.session_id != expected_session_id {
            tracing::debug!(
                component = "registry",
                controller_id = %controller_id,
                expected_session_id = %expected_session_id,
                actual_session_id = %session.session_id,
                "mark_offline called by stale task; ignoring"
            );
            return false;
        }
        if session.offline_since.is_some() {
            return false;
        }
        session.session_cancel.cancel();
        session.offline_since = Some(Instant::now());
        session.stream_tx.take();
        tracing::info!(
            component = "registry",
            controller_id = %controller_id,
            session_id = %expected_session_id,
            "Controller marked offline, stream sender released"
        );
        true
    }

    #[allow(dead_code)]
    pub fn online_controller_ids(&self) -> Vec<String> {
        self.inner
            .read()
            .values()
            .filter(|s| s.is_online())
            .map(|s| s.controller_id.clone())
            .collect()
    }

    #[allow(dead_code)]
    pub fn online_senders(&self) -> Vec<(String, mpsc::Sender<CenterMessage>)> {
        self.inner
            .read()
            .values()
            .filter_map(|s| {
                if !s.is_online() {
                    return None;
                }
                s.stream_tx
                    .as_ref()
                    .map(|tx| (s.controller_id.clone(), tx.clone()))
            })
            .collect()
    }

    /// Remove a controller entry from the registry entirely.
    /// Returns true if an entry was removed. Unlike `mark_offline`, this
    /// drops the session record itself (used by Admin DELETE cascade).
    pub fn remove(&self, controller_id: &str) -> bool {
        let removed = self.inner.write().remove(controller_id);
        if let Some(mut session) = removed {
            if let Some(ownership) = &session.ownership {
                ownership.valid.store(false, Ordering::Release);
            }
            session.session_cancel.cancel();
            session.stream_tx.take();
            self.metrics.record_eviction();
            true
        } else {
            false
        }
    }

    /// Current number of registered controller sessions (online + offline).
    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    /// True when no controller sessions are registered.
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.inner.read().is_empty()
    }

    /// Number of currently online controller sessions.
    pub fn online_len(&self) -> usize {
        self.inner.read().values().filter(|s| s.is_online()).count()
    }
}

impl Default for ControllerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct SessionView {
    #[allow(dead_code)]
    pub controller_id: String,
    #[allow(dead_code)]
    pub session_id: String,
    #[allow(dead_code)]
    pub info: RegisterRequest,
    pub stream_tx: Option<mpsc::Sender<CenterMessage>>,
    #[allow(dead_code)]
    pub last_seen: Instant,
    #[allow(dead_code)]
    pub offline_since: Option<Instant>,
    pub(crate) session_cancel: CancellationToken,
    pub ownership: Option<SessionOwnership>,
}

impl SessionView {
    pub fn matches_ownership(
        &self,
        holder: &str,
        fence: &edgion_center_core::OwnershipFence,
    ) -> bool {
        self.ownership.as_ref().is_some_and(|ownership| {
            ownership.is_valid() && ownership.holder == holder && ownership.fence == *fence
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct CountingMetrics {
        reentries: AtomicUsize,
        evictions: AtomicUsize,
    }

    impl RegistryMetrics for CountingMetrics {
        fn record_session_reentry(&self) {
            self.reentries.fetch_add(1, Ordering::SeqCst);
        }

        fn record_eviction(&self) {
            self.evictions.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn mock_info(cid: &str) -> RegisterRequest {
        RegisterRequest {
            controller_id: cid.to_string(),
            cluster: "cluster".to_string(),
            env: vec![],
            tag: vec![],
            supported_kinds: vec![],
        }
    }

    #[test]
    fn test_register_and_lookup() {
        let registry = ControllerRegistry::new();
        let (tx, _rx) = mpsc::channel(8);
        registry.register(
            "cluster/ctrl-01".to_string(),
            mock_info("cluster/ctrl-01"),
            tx,
            "sess-1".to_string(),
        );
        assert!(registry.get_session("cluster/ctrl-01").is_some());
        assert_eq!(registry.online_controller_ids().len(), 1);
    }

    #[tokio::test]
    async fn test_mark_offline_releases_sender_and_preserves_entry() {
        let registry = ControllerRegistry::new();
        let (tx, mut rx) = mpsc::channel(8);
        registry.register("cid".to_string(), mock_info("cid"), tx, "s1".to_string());

        assert_eq!(registry.online_senders().len(), 1);

        let marked = registry.mark_offline("cid", "s1");
        assert!(marked, "first mark_offline should transition");

        let view = registry
            .get_session("cid")
            .expect("session should still exist");
        assert!(view.offline_since.is_some());
        assert!(view.stream_tx.is_none());
        assert_eq!(registry.online_senders().len(), 0);

        // Only the registry held a Sender clone; after take() the receiver drains.
        assert!(
            rx.recv().await.is_none(),
            "receiver should see channel close"
        );
    }

    #[test]
    fn test_mark_offline_wrong_session_id_is_noop() {
        let registry = ControllerRegistry::new();
        let (tx, _rx) = mpsc::channel(8);
        registry.register("cid".to_string(), mock_info("cid"), tx, "s1".to_string());

        let marked = registry.mark_offline("cid", "s2");
        assert!(!marked, "mismatched session_id must not transition");

        let view = registry
            .get_session("cid")
            .expect("session should still exist");
        assert!(view.offline_since.is_none());
        assert!(view.stream_tx.is_some());
    }

    #[test]
    fn test_mark_offline_idempotent() {
        let registry = ControllerRegistry::new();
        let (tx, _rx) = mpsc::channel(8);
        registry.register("cid".to_string(), mock_info("cid"), tx, "s1".to_string());

        assert!(registry.mark_offline("cid", "s1"));
        assert!(
            !registry.mark_offline("cid", "s1"),
            "second call on already-offline session must be noop"
        );

        let view = registry.get_session("cid").expect("still exists");
        assert!(view.offline_since.is_some());
        assert!(view.stream_tx.is_none());
    }

    #[test]
    fn test_online_senders_excludes_offline() {
        let registry = ControllerRegistry::new();
        let (tx1, _rx1) = mpsc::channel(8);
        let (tx2, _rx2) = mpsc::channel(8);
        registry.register("c1".to_string(), mock_info("c1"), tx1, "s1".to_string());
        registry.register("c2".to_string(), mock_info("c2"), tx2, "s2".to_string());
        assert!(registry.mark_offline("c2", "s2"));
        let senders = registry.online_senders();
        assert_eq!(senders.len(), 1);
        assert_eq!(senders[0].0, "c1");
    }

    #[test]
    fn test_remove_drops_entry_and_is_idempotent() {
        let registry = ControllerRegistry::new();
        let (tx, _rx) = mpsc::channel(8);
        let registration = registry.register_cancellable(
            "cid".to_string(),
            mock_info("cid"),
            tx,
            "s1".to_string(),
        );
        assert!(registry.remove("cid"));
        assert!(registration.session_cancel.is_cancelled());
        assert!(registry.get_session("cid").is_none());
        assert!(!registry.remove("cid"));
    }

    #[test]
    fn register_returns_displaced_live_session_id() {
        let reg = ControllerRegistry::new();
        let (tx1, _rx1) = mpsc::channel(8);
        let prev = reg.register("cid".to_string(), mock_info("cid"), tx1, "s1".to_string());
        assert_eq!(prev, None, "first registration displaces nothing");

        let (tx2, _rx2) = mpsc::channel(8);
        let prev = reg.register("cid".to_string(), mock_info("cid"), tx2, "s2".to_string());
        assert_eq!(
            prev,
            Some("s1".to_string()),
            "second registration displaces s1"
        );
    }

    #[test]
    fn takeover_immediately_cancels_displaced_session() {
        let registry = ControllerRegistry::new();
        let (tx1, _rx1) = mpsc::channel(8);
        let first = registry.register_cancellable(
            "cid".to_string(),
            mock_info("cid"),
            tx1,
            "s1".to_string(),
        );
        assert!(!first.session_cancel.is_cancelled());

        let (tx2, _rx2) = mpsc::channel(8);
        let second = registry.register_cancellable(
            "cid".to_string(),
            mock_info("cid"),
            tx2,
            "s2".to_string(),
        );
        assert!(first.session_cancel.is_cancelled());
        assert!(!second.session_cancel.is_cancelled());
        assert_eq!(second.displaced_session_id.as_deref(), Some("s1"));
    }

    #[test]
    fn same_holder_reacquire_fences_owned_session_before_lease_rotation() {
        let registry = ControllerRegistry::new();
        let (tx, _rx) = mpsc::channel(8);
        let valid = Arc::new(AtomicBool::new(true));
        let registration = registry.register_owned_cancellable(
            "cid".to_string(),
            mock_info("cid"),
            tx,
            "s1".to_string(),
            Some(SessionOwnership {
                holder: "center-0/uid-0".to_string(),
                fence: edgion_center_core::OwnershipFence {
                    token: "old-token".to_string(),
                    epoch: 1,
                },
                valid: valid.clone(),
            }),
        );

        assert!(registry.fence_owned_session("cid"));
        assert!(!valid.load(Ordering::Acquire));
        assert!(registration.session_cancel.is_cancelled());
        assert!(!registry
            .get_session("cid")
            .unwrap()
            .ownership
            .unwrap()
            .is_valid());
    }

    #[test]
    fn shutdown_cancels_all_live_sessions_and_hides_senders() {
        let registry = ControllerRegistry::new();
        let (tx1, _rx1) = mpsc::channel(8);
        let first =
            registry.register_cancellable("c1".to_string(), mock_info("c1"), tx1, "s1".to_string());
        let (tx2, _rx2) = mpsc::channel(8);
        registry.register("c2".to_string(), mock_info("c2"), tx2, "s2".to_string());
        assert!(registry.mark_offline("c2", "s2"));

        assert_eq!(registry.cancel_all(), 1);
        assert!(first.session_cancel.is_cancelled());
        assert!(registry.online_senders().is_empty());
        assert_eq!(registry.cancel_all(), 0);
    }

    #[test]
    fn register_does_not_report_offline_session_as_displaced() {
        let reg = ControllerRegistry::new();
        let (tx1, _rx1) = mpsc::channel(8);
        reg.register("cid".to_string(), mock_info("cid"), tx1, "s1".to_string());
        assert!(reg.mark_offline("cid", "s1"));
        let (tx2, _rx2) = mpsc::channel(8);
        let prev = reg.register("cid".to_string(), mock_info("cid"), tx2, "s2".to_string());
        assert_eq!(
            prev, None,
            "an already-offline session is not a live takeover"
        );
    }

    #[test]
    fn last_seen_secs_ago_returns_some_for_known_none_for_unknown() {
        let registry = ControllerRegistry::new();
        let (tx, _rx) = mpsc::channel(8);
        registry.register("cid".to_string(), mock_info("cid"), tx, "s1".to_string());
        assert!(registry.last_seen_secs_ago("cid").is_some());
        assert_eq!(registry.last_seen_secs_ago("nope"), None);
    }

    #[test]
    fn is_current_session_tracks_latest() {
        let reg = ControllerRegistry::new();
        let (tx1, _rx1) = mpsc::channel(8);
        reg.register("cid".to_string(), mock_info("cid"), tx1, "s1".to_string());
        assert!(reg.is_current_session("cid", "s1"));
        let (tx2, _rx2) = mpsc::channel(8);
        reg.register("cid".to_string(), mock_info("cid"), tx2, "s2".to_string());
        assert!(!reg.is_current_session("cid", "s1"));
        assert!(reg.is_current_session("cid", "s2"));
        assert!(!reg.is_current_session("unknown", "s2"));
    }

    #[test]
    fn lifecycle_metrics_are_emitted_through_the_runtime_hook() {
        let metrics = Arc::new(CountingMetrics::default());
        let registry = ControllerRegistry::with_metrics(metrics.clone());
        let (tx1, _rx1) = mpsc::channel(8);
        registry.register("cid".to_string(), mock_info("cid"), tx1, "s1".to_string());
        let (tx2, _rx2) = mpsc::channel(8);
        registry.register("cid".to_string(), mock_info("cid"), tx2, "s2".to_string());
        assert!(registry.remove("cid"));
        assert_eq!(metrics.reentries.load(Ordering::SeqCst), 1);
        assert_eq!(metrics.evictions.load(Ordering::SeqCst), 1);
    }
}
