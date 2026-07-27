//! In-memory controller-info aggregator.
//!
//! Tracks per-controller registration and online/offline state.
//! Snapshots are updated on registration and marked-offline on disconnect.
//! Offline entries are retained in memory and must be removed explicitly via
//! the Admin DELETE API (see ticket #20).

use parking_lot::RwLock;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

#[derive(Debug)]
struct ControllerSnapshot {
    info: ControllerInfo,
    offline_since: Option<std::time::Instant>,
    /// Most recent stats push from the controller (None until the first
    /// StatsReport arrives over fed_sync).
    stats: Option<StatsEntry>,
}

#[derive(Debug, Clone)]
struct StatsEntry {
    /// Sum of `per_kind` values from the latest StatsReport.
    total: u64,
    /// Per-kind counts from the latest report, exposed via
    /// `ControllerSummary::per_kind`. `None` when the report's map violated
    /// an ingest bound and was dropped by the caller (see
    /// `bounded_per_kind` in `federation::server`) — the scalar `total`
    /// still flows in that case, only the breakdown is withheld.
    per_kind: Option<BTreeMap<String, u32>>,
    /// Wall-clock instant the latest report was received.
    updated_at: std::time::Instant,
}

impl ControllerSnapshot {
    fn new(info: ControllerInfo) -> Self {
        Self {
            info,
            offline_since: None,
            stats: None,
        }
    }
}

#[derive(Clone)]
pub struct ResourceAggregator {
    inner: Arc<RwLock<HashMap<String, ControllerSnapshot>>>,
    metrics: Arc<dyn AggregatorMetrics>,
}

/// Registration fields needed by aggregation, independent of the gRPC schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerInfo {
    pub controller_id: String,
    pub cluster: String,
    pub environments: Vec<String>,
    pub tags: Vec<String>,
}

/// Observability hook supplied by the process composition root.
pub trait AggregatorMetrics: Send + Sync {
    fn set_controller_count(&self, cluster: &str, count: u64);
    fn record_eviction(&self);
}

struct NoopAggregatorMetrics;

impl AggregatorMetrics for NoopAggregatorMetrics {
    fn set_controller_count(&self, _cluster: &str, _count: u64) {}

    fn record_eviction(&self) {}
}

impl ResourceAggregator {
    pub fn new() -> Self {
        Self::with_metrics(Arc::new(NoopAggregatorMetrics))
    }

    pub fn with_metrics(metrics: Arc<dyn AggregatorMetrics>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            metrics,
        }
    }

    /// Called when controller registers (or reconnects).
    ///
    /// Mirrors the Kubernetes directory's `upsert_registration`, which
    /// deliberately resets the record's counts to `None` on re-registration:
    /// a fresh session has reported nothing yet, so the previous session's
    /// `stats` must not be served as this session's `Fresh` counts. If the
    /// reconnected Controller is not yet ready, its stats task is gated off,
    /// so this window is not necessarily brief.
    pub fn set_controller_info(&self, controller_id: &str, info: ControllerInfo) {
        let snapshot = {
            let mut map = self.inner.write();
            let snap = map
                .entry(controller_id.to_string())
                .or_insert_with(|| ControllerSnapshot::new(info.clone()));
            snap.info = info;
            snap.offline_since = None;
            snap.stats = None;
            Self::compute_gauge_snapshot(&map)
        };
        self.emit_gauges(&snapshot);
    }

    pub fn mark_offline(&self, controller_id: &str) {
        let snapshot = {
            let mut map = self.inner.write();
            if let Some(snap) = map.get_mut(controller_id) {
                if snap.offline_since.is_none() {
                    snap.offline_since = Some(std::time::Instant::now());
                }
            }
            Self::compute_gauge_snapshot(&map)
        };
        self.emit_gauges(&snapshot);
    }

    /// Remove a controller's snapshot entirely. Used by Admin DELETE cascade.
    pub fn remove(&self, controller_id: &str) -> bool {
        let outcome = {
            let mut map = self.inner.write();
            // Capture the cluster before removal so we can zero the gauge if it disappears.
            let removed_cluster = map.get(controller_id).map(|s| {
                if s.info.cluster.is_empty() {
                    "unknown".to_string()
                } else {
                    s.info.cluster.clone()
                }
            });
            if map.remove(controller_id).is_none() {
                None
            } else {
                let snapshot = Self::compute_gauge_snapshot(&map);
                // Pre-seeding cannot see a fully-removed entry; emit zero explicitly when
                // the last controller for this cluster is gone.
                let zero_cluster = removed_cluster.and_then(|cluster| {
                    let still_present = map.values().any(|s| {
                        let c = if s.info.cluster.is_empty() {
                            "unknown"
                        } else {
                            s.info.cluster.as_str()
                        };
                        c == cluster
                    });
                    if still_present {
                        None
                    } else {
                        Some(cluster)
                    }
                });
                Some((snapshot, zero_cluster))
            }
        };
        let Some((snapshot, zero_cluster)) = outcome else {
            return false;
        };
        self.metrics.record_eviction();
        self.emit_gauges(&snapshot);
        if let Some(cluster) = zero_cluster {
            self.metrics.set_controller_count(&cluster, 0);
        }
        true
    }

    /// Compute the per-cluster online-controller counts. Pure function — must
    /// be called inside the lock so the snapshot is consistent with the map
    /// state at that instant. The returned map is then emitted by
    /// [`Self::emit_gauges`] outside the lock.
    fn compute_gauge_snapshot(map: &HashMap<String, ControllerSnapshot>) -> HashMap<String, u64> {
        let mut by_cluster: HashMap<String, u64> = HashMap::new();
        // Pre-seed all known clusters with 0 so disappeared clusters are zeroed out.
        for snap in map.values() {
            let cluster = if snap.info.cluster.is_empty() {
                "unknown"
            } else {
                &snap.info.cluster
            };
            by_cluster.entry(cluster.to_string()).or_insert(0);
        }
        for snap in map.values().filter(|s| s.offline_since.is_none()) {
            let cluster = if snap.info.cluster.is_empty() {
                "unknown".to_string()
            } else {
                snap.info.cluster.clone()
            };
            *by_cluster.entry(cluster).or_default() += 1;
        }
        by_cluster
    }

    /// Emit the gauge snapshot to the `metrics` backend. Must be called
    /// OUTSIDE the write lock so a future metrics backend with non-trivial
    /// emit cost cannot stall registration / disconnect handling.
    fn emit_gauges(&self, snapshot: &HashMap<String, u64>) {
        for (cluster, count) in snapshot {
            self.metrics.set_controller_count(cluster, *count);
        }
    }

    /// Store/refresh the latest controller-reported stats snapshot.
    ///
    /// Called from the fed_sync server task on each `StatsReport`. Silently
    /// drops if the controller is unknown to the aggregator (this would only
    /// happen if a stats message races a removal).
    ///
    /// `per_kind` must already be bounds-checked by the caller (via
    /// `bounded_per_kind` in `federation::server`) — `None` means the
    /// report's map violated an ingest bound and was dropped, not merely
    /// that the report carried no kinds. The scalar `total` is always
    /// stored, even when `per_kind` is `None`.
    pub fn update_stats(
        &self,
        controller_id: &str,
        per_kind: Option<BTreeMap<String, u32>>,
        total: u64,
    ) {
        let mut map = self.inner.write();
        if let Some(snap) = map.get_mut(controller_id) {
            snap.stats = Some(StatsEntry {
                total,
                per_kind,
                updated_at: std::time::Instant::now(),
            });
        }
    }

    /// Summary of all known controllers (for Admin API).
    ///
    /// `last_seen_secs_ago` is filled in by the API layer (the aggregator does
    /// not own the `ControllerRegistry` session table). The aggregator only
    /// reports the stats-derived `key_count` and `stats_updated_secs_ago`.
    pub fn controller_summaries(&self) -> Vec<ControllerSummary> {
        self.inner
            .read()
            .values()
            .map(|s| {
                let online = s.offline_since.is_none();
                let key_count = s.stats.as_ref().map(|e| e.total);
                ControllerSummary {
                    controller_id: s.info.controller_id.clone(),
                    cluster: s.info.cluster.clone(),
                    env: s.info.environments.clone(),
                    tag: s.info.tags.clone(),
                    online,
                    key_count,
                    per_kind: s.stats.as_ref().and_then(|e| e.per_kind.clone()),
                    stats_updated_secs_ago: s
                        .stats
                        .as_ref()
                        .map(|e| e.updated_at.elapsed().as_secs()),
                    stats_state: StatsState::derive(key_count.is_some(), online),
                    last_seen_secs_ago: None,
                }
            })
            .collect()
    }
}

impl Default for ResourceAggregator {
    fn default() -> Self {
        Self::new()
    }
}

/// Three-state freshness discriminator for a controller's reported resource
/// counts.
///
/// Derived from LIVENESS, never from a time threshold: a Controller only
/// pushes `StatsReport` when its counts change, so a long silence means
/// "unchanged", not "stale". Do not add elapsed-time comparisons here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StatsState {
    /// Counts are present and the controller is currently online.
    Fresh,
    /// Counts are present but frozen at their last value because the
    /// controller is offline. The K8s adapter deliberately preserves counts
    /// on `mark_offline` so this state is renderable.
    Stale,
    /// No counts have ever been observed for this controller.
    Missing,
}

impl StatsState {
    /// Single derivation rule, shared by every call site so the aggregator-only
    /// and directory-composed summary paths can never disagree:
    ///
    /// ```text
    /// counts present && online  -> Fresh
    /// counts present && !online -> Stale
    /// no counts                 -> Missing
    /// ```
    pub fn derive(counts_present: bool, online: bool) -> Self {
        match (counts_present, online) {
            (true, true) => StatsState::Fresh,
            (true, false) => StatsState::Stale,
            (false, _) => StatsState::Missing,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ControllerSummary {
    pub controller_id: String,
    pub cluster: String,
    pub env: Vec<String>,
    pub tag: Vec<String>,
    pub online: bool,
    /// Total resource count pushed by the controller (sum of per-kind counts).
    /// `None` until the first StatsReport arrives.
    pub key_count: Option<u64>,
    /// Per-kind resource counts from the latest StatsReport.
    /// `None` until the first StatsReport arrives.
    pub per_kind: Option<std::collections::BTreeMap<String, u32>>,
    /// Seconds since the last StatsReport from this controller.
    /// `None` until the first StatsReport arrives.
    pub stats_updated_secs_ago: Option<u64>,
    /// Freshness of `key_count` / `per_kind`, derived from liveness (see
    /// [`StatsState`]).
    pub stats_state: StatsState,
    /// Seconds since the last inbound fed_sync message from this controller.
    /// Filled in by the API layer using the registry session table — the
    /// aggregator leaves this as `None`.
    pub last_seen_secs_ago: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mock_register_info(cid: &str, cluster: &str) -> ControllerInfo {
        ControllerInfo {
            controller_id: cid.to_string(),
            cluster: cluster.to_string(),
            environments: vec![],
            tags: vec![],
        }
    }

    #[test]
    fn test_set_and_mark_offline() {
        let agg = ResourceAggregator::new();
        agg.set_controller_info("ctrl-1", mock_register_info("ctrl-1", "cluster-a"));
        let summaries = agg.controller_summaries();
        assert_eq!(summaries.len(), 1);
        assert!(summaries[0].online);
        agg.mark_offline("ctrl-1");
        let summaries = agg.controller_summaries();
        assert!(!summaries[0].online);
    }

    #[test]
    fn test_remove_drops_snapshot() {
        let agg = ResourceAggregator::new();
        agg.set_controller_info("ctrl-1", mock_register_info("ctrl-1", "cluster-a"));
        assert!(agg.remove("ctrl-1"));
        assert_eq!(agg.controller_summaries().len(), 0);
        assert!(!agg.remove("ctrl-1"));
    }

    #[test]
    fn test_reconnect_clears_offline() {
        let agg = ResourceAggregator::new();
        agg.set_controller_info("ctrl-1", mock_register_info("ctrl-1", "cluster-a"));
        agg.mark_offline("ctrl-1");
        assert!(!agg.controller_summaries()[0].online);
        agg.set_controller_info("ctrl-1", mock_register_info("ctrl-1", "cluster-a"));
        assert!(agg.controller_summaries()[0].online);
    }

    /// Mirrors the Kubernetes directory's `upsert_registration`, which
    /// deliberately resets the record's counts to `None` on re-registration.
    /// Without this, a reconnected controller would have the PREVIOUS
    /// session's stale counts served as `Fresh` via the aggregator fallback
    /// path (the only path standalone-SQL deployments ever take) until its
    /// stats task — gated behind readiness — pushes a new report.
    #[test]
    fn reconnect_clears_stale_stats() {
        let agg = ResourceAggregator::new();
        agg.set_controller_info("ctrl-1", mock_register_info("ctrl-1", "cluster-a"));
        let mut per_kind = BTreeMap::new();
        per_kind.insert("Pod".to_string(), 3u32);
        agg.update_stats("ctrl-1", Some(per_kind), 3);

        let summaries = agg.controller_summaries();
        assert_eq!(summaries[0].stats_state, StatsState::Fresh);
        assert_eq!(summaries[0].key_count, Some(3));
        assert!(summaries[0].per_kind.is_some());

        // Controller drops off and reconnects: registration must wipe the
        // stale session's stats, not merely clear offline_since.
        agg.mark_offline("ctrl-1");
        agg.set_controller_info("ctrl-1", mock_register_info("ctrl-1", "cluster-a"));

        let summaries = agg.controller_summaries();
        assert!(summaries[0].online);
        assert_eq!(
            summaries[0].stats_state,
            StatsState::Missing,
            "stale stats from the previous session must not be reported as Fresh"
        );
        assert_eq!(summaries[0].key_count, None);
        assert_eq!(summaries[0].per_kind, None);
    }

    /// Freshness is derived from liveness, never from a time threshold: a
    /// Controller only pushes `StatsReport` when its counts change, so
    /// `stats_state` must track `online` / presence-of-counts, not elapsed
    /// time since the last report.
    #[test]
    fn summaries_expose_per_kind_and_freshness() {
        let agg = ResourceAggregator::new();

        // A controller with no stats at all: Missing, key_count None, per_kind None.
        agg.set_controller_info(
            "ctrl-missing",
            mock_register_info("ctrl-missing", "cluster-a"),
        );

        // A controller that has reported stats and is online: Fresh.
        agg.set_controller_info("ctrl-1", mock_register_info("ctrl-1", "cluster-a"));
        let mut per_kind = BTreeMap::new();
        per_kind.insert("Pod".to_string(), 3u32);
        per_kind.insert("Service".to_string(), 2u32);
        agg.update_stats("ctrl-1", Some(per_kind), 5);

        let summaries = agg.controller_summaries();
        let by_id = |id: &str| {
            summaries
                .iter()
                .find(|s| s.controller_id == id)
                .unwrap_or_else(|| panic!("missing summary for {id}"))
                .clone()
        };

        let fresh = by_id("ctrl-1");
        assert_eq!(fresh.stats_state, StatsState::Fresh);
        assert!(fresh.online);
        assert_eq!(fresh.key_count, Some(5));
        let per_kind = fresh
            .per_kind
            .expect("per_kind present when stats reported");
        assert_eq!(per_kind.get("Pod"), Some(&3));
        assert_eq!(per_kind.get("Service"), Some(&2));

        let missing = by_id("ctrl-missing");
        assert_eq!(missing.stats_state, StatsState::Missing);
        assert_eq!(missing.key_count, None);
        assert_eq!(missing.per_kind, None);

        // Going offline must NOT clear the counts: the K8s adapter deliberately
        // preserves counts on mark_offline so Stale is renderable, frozen at
        // their last reported value.
        agg.mark_offline("ctrl-1");
        let summaries = agg.controller_summaries();
        let stale = summaries
            .iter()
            .find(|s| s.controller_id == "ctrl-1")
            .unwrap();
        assert_eq!(stale.stats_state, StatsState::Stale);
        assert!(!stale.online);
        assert_eq!(stale.key_count, Some(5));
        let per_kind = stale
            .per_kind
            .as_ref()
            .expect("per_kind retained after going offline");
        assert_eq!(per_kind.get("Pod"), Some(&3));
    }
}
