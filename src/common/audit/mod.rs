//! Non-blocking, fail-open audit sink.
//!
//! The Admin API records mutating actions into the `audit_log` table, but must
//! never let persistence latency (or a stalled database) add latency to a
//! request. [`AuditSink`] therefore decouples the request path from the write:
//!
//! - `record()` does a single `try_send` onto a bounded channel and returns
//!   immediately. It never `await`s and never blocks.
//! - A background task drains the channel and performs the actual insert. An
//!   insert failure is logged (WARN) and the record is dropped — the request is
//!   never affected (fail-open).
//! - When the channel is full (a write burst the writer can't keep up with) or
//!   closed, the record is dropped, a process-wide counter is incremented, and a
//!   DEBUG line is emitted. The drop is surfaced on `/metrics` as
//!   [`AUDIT_DROPPED_METRIC`].

pub mod middleware;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::store::audit::AuditRecord;
use crate::store::Store;

/// Prometheus counter name for audit records dropped on the fail-open path.
pub const AUDIT_DROPPED_METRIC: &str = "edgion_center_audit_dropped_total";

/// Bounded capacity of the audit channel. A burst beyond this is dropped rather
/// than allowed to back-pressure (and thus slow) the request path.
const CHANNEL_CAPACITY: usize = 1024;

/// Process-wide count of dropped audit records. Mirrors the Prometheus counter
/// so tests can assert on it deterministically without installing a recorder
/// (the `metrics` crate no-ops `counter!` when no recorder is present).
static AUDIT_DROPPED: AtomicU64 = AtomicU64::new(0);

/// Returns the process-wide number of dropped audit records.
pub fn audit_dropped_total() -> u64 {
    AUDIT_DROPPED.load(Ordering::Relaxed)
}

/// A cheap-to-clone handle that hands audit records to a background writer.
#[derive(Clone)]
pub struct AuditSink {
    tx: mpsc::Sender<AuditRecord>,
}

impl AuditSink {
    /// Create a sink backed by a freshly spawned background writer task.
    ///
    /// The task loops `recv().await` and inserts each record via
    /// `Store::insert_audit`, logging a WARN on insert error (fail-open). It
    /// exits when all `AuditSink` clones are dropped (the channel closes).
    pub fn spawn(store: Arc<Store>) -> AuditSink {
        let (tx, mut rx) = mpsc::channel::<AuditRecord>(CHANNEL_CAPACITY);
        tokio::spawn(async move {
            while let Some(rec) = rx.recv().await {
                if let Err(e) = store.insert_audit(&rec).await {
                    tracing::warn!(
                        component = "audit",
                        error = %e,
                        "failed to persist audit record (dropping)"
                    );
                }
            }
            tracing::debug!(component = "audit", "audit writer task stopped (channel closed)");
        });
        AuditSink { tx }
    }

    /// Enqueue a record for the background writer. Non-blocking: a single
    /// `try_send`. On a full or closed channel the record is dropped, the
    /// dropped counter is incremented, and a DEBUG line is logged. Never awaits.
    pub fn record(&self, rec: AuditRecord) {
        if let Err(e) = self.tx.try_send(rec) {
            AUDIT_DROPPED.fetch_add(1, Ordering::Relaxed);
            metrics::counter!(AUDIT_DROPPED_METRIC).increment(1);
            match e {
                mpsc::error::TrySendError::Full(_) => {
                    tracing::debug!(component = "audit", "audit channel full; record dropped");
                }
                mpsc::error::TrySendError::Closed(_) => {
                    tracing::debug!(component = "audit", "audit channel closed; record dropped");
                }
            }
        }
    }

    /// Test-only constructor over a pre-made sender, so a test can own the
    /// receiver and inspect (or deliberately not drain) what was recorded.
    #[cfg(test)]
    pub fn from_sender(tx: mpsc::Sender<AuditRecord>) -> AuditSink {
        AuditSink { tx }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> AuditRecord {
        AuditRecord {
            ts: 1,
            actor: "alice".to_string(),
            provider: "local".to_string(),
            method: "POST".to_string(),
            path: "/api/v1/center/admin/controllers".to_string(),
            target_controller: None,
            status: 200,
            source_ip: None,
            request_id: None,
            detail: None,
        }
    }

    /// Fail-open drop accounting for both full and closed channels.
    ///
    /// These two cases are asserted in ONE test on purpose: they are the only
    /// code paths that increment the process-wide `AUDIT_DROPPED` static, so
    /// keeping them in a single (non-parallel) test makes the delta assertions
    /// deterministic — a separate concurrent test could otherwise bump the
    /// shared counter between this test's before/after reads.
    #[tokio::test]
    async fn sink_drops_when_full_or_closed_and_counts() {
        // Full channel: cap-1 with no draining consumer. The first record fills
        // the single slot; the second must be dropped and counted exactly once.
        let (tx, _rx) = mpsc::channel::<AuditRecord>(1);
        let sink = AuditSink::from_sender(tx);
        let before = audit_dropped_total();
        sink.record(sample()); // buffered into the single slot — succeeds
        sink.record(sample()); // channel full — dropped + counted
        let after = audit_dropped_total();
        assert_eq!(after - before, 1, "exactly one record must be dropped when full");
        // `_rx` held so the failure is Full, not Closed.

        // Closed channel: receiver dropped. The send must drop + count, not panic.
        let (tx2, rx2) = mpsc::channel::<AuditRecord>(4);
        drop(rx2);
        let sink2 = AuditSink::from_sender(tx2);
        let before2 = audit_dropped_total();
        sink2.record(sample());
        let after2 = audit_dropped_total();
        assert_eq!(after2 - before2, 1, "send on a closed channel must drop + count");
    }
}
