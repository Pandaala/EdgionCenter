//! ControllerRegistry: manages per-controller sessions and stream handles.

use crate::federation::proto::{CenterMessage, RegisterRequest};
use parking_lot::RwLock;
use std::collections::HashMap;
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
}

impl ControllerSession {
    pub fn is_online(&self) -> bool {
        self.stream_tx.is_some()
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
        let session_cancel = CancellationToken::new();
        let session = ControllerSession {
            controller_id: controller_id.clone(),
            session_id,
            info,
            stream_tx: Some(stream_tx),
            last_seen: Instant::now(),
            offline_since: None,
            session_cancel: session_cancel.clone(),
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
        map.get(controller_id).map(|s| SessionView {
            controller_id: s.controller_id.clone(),
            session_id: s.session_id.clone(),
            info: s.info.clone(),
            stream_tx: s.stream_tx.clone(),
            last_seen: s.last_seen,
            offline_since: s.offline_since,
        })
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
        let removed = self.inner.write().remove(controller_id).is_some();
        if removed {
            self.metrics.record_eviction();
        }
        removed
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
        registry.register("cid".to_string(), mock_info("cid"), tx, "s1".to_string());
        assert!(registry.remove("cid"));
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
