//! Shared write core for every Center-initiated write to an
//! `EdgionConfigData` resource.
//!
//! # Why the CAS precondition comes from the local watch cache
//!
//! The precondition is read from the LOCAL WATCH CACHE, never from a fresh
//! GET against the Controller. That is deliberate: because the write is a
//! CAS (`If-Match` on `metadata.resourceVersion`), any change that lands
//! between our cached read and our write makes the write 409 and we never
//! reach the convergence wait. A successful CAS therefore pins our change to
//! that exact version, and cached versions only move forward (the watch
//! stream is monotonic) — so "the version left the precondition" can only
//! mean the watch advanced to at or past our write.
//!
//! One exception, and it does not contradict the rule above: in a
//! multi-replica Kubernetes deployment a Controller's federation session is
//! owned by exactly one replica, and the watch cache is process-local, so a
//! NON-OWNING replica has no cache entry for that Controller at all — not
//! because the document doesn't exist, but because this process was never
//! fed it. There the choice isn't "cache vs. a needless GET", it's "GET or
//! fail a write that would have succeeded", so step 1 falls back to a
//! proxied GET (which the proxy forwards to the owning replica) only when
//! the cache is empty AND we do not own the session. That path can only
//! ever end in `Accepted`: this replica still cannot observe convergence.
//!
//! # Why convergence needs two independent signals
//!
//! Convergence needs TWO independent signals: the version moved (the watch
//! caught up with *some* write) AND the caller's predicate holds (the
//! intent we asked for is currently in effect). Version-moved-but-
//! predicate-false is NOT a failure and NOT a timeout — it means our write
//! landed and was then overwritten by something else, and the operator must
//! be told exactly that, with the document as last observed
//! (`OutcomeState::Superseded`).
//!
//! # Why polling only ever reads the local cache
//!
//! The convergence poll reads the LOCAL watch cache on every tick. It never
//! polls the Controller: the whole point of observing convergence "for
//! free" is that the federation watch stream already keeps the cache
//! current, without an extra round trip per poll tick.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use http::StatusCode;

use edgion_center_runtime::federation::proto::HttpProxyResponse;

use super::ApiState;

/// Cadence for local-cache convergence polling after a successful write.
pub const CONVERGENCE_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// Total time budget for observing convergence before reporting `Unknown`.
pub const CONVERGENCE_BUDGET: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OutcomeState {
    Converged,
    Superseded,
    Accepted,
    Conflict,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WriteOutcome {
    pub controller_id: String,
    pub state: OutcomeState,
    /// Why the terminal state was reached — e.g. "not the owning replica",
    /// "overwritten after the write landed", the upstream error. Absent on a
    /// clean `converged`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The document as last observed in the local cache. Present for
    /// `superseded` so the operator sees what is actually in effect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed: Option<serde_json::Value>,
    /// Measured time from write dispatch to observation. Telemetry only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub convergence_ms: Option<u64>,
}

impl WriteOutcome {
    fn failed(controller_id: &str, reason: impl Into<String>) -> Self {
        Self {
            controller_id: controller_id.to_string(),
            state: OutcomeState::Failed,
            reason: Some(reason.into()),
            observed: None,
            convergence_ms: None,
        }
    }

    fn converged(controller_id: &str, convergence_ms: u64) -> Self {
        Self {
            controller_id: controller_id.to_string(),
            state: OutcomeState::Converged,
            reason: None,
            observed: None,
            convergence_ms: Some(convergence_ms),
        }
    }

    fn superseded(controller_id: &str, observed: serde_json::Value, convergence_ms: u64) -> Self {
        Self {
            controller_id: controller_id.to_string(),
            state: OutcomeState::Superseded,
            reason: Some("overwritten after the write landed".to_string()),
            observed: Some(observed),
            convergence_ms: Some(convergence_ms),
        }
    }
}

/// Read the cached document, apply `mutate`, write it back under a CAS
/// precondition, then observe convergence locally.
///
/// `predicate` answers "is the intent currently in effect?" against a
/// document. It is checked twice: before writing (idempotent skip) and
/// during convergence.
pub async fn write_config_data(
    state: &ApiState,
    controller_id: &str,
    namespace: &str,
    name: &str,
    mutate: &(dyn Fn(&mut serde_json::Value) -> Result<(), String> + Send + Sync),
    predicate: &(dyn Fn(&serde_json::Value) -> bool + Send + Sync),
) -> WriteOutcome {
    write_config_data_with_dispatch(
        state,
        controller_id,
        namespace,
        name,
        mutate,
        predicate,
        |method, path, headers, body| {
            state
                .proxy
                .forward(controller_id, method, path, headers, body)
        },
        |method, path, headers, body| {
            state
                .proxy
                .forward(controller_id, method, path, headers, body)
        },
    )
    .await
}

/// Same sequence as [`write_config_data`], with the fallback read and the
/// write dispatch both injectable. `ProxyForwarder::forward` needs a live,
/// registered Controller session and drives an actual gRPC bidirectional
/// stream, so it cannot be faked from `center-app`; this seam lets tests
/// control both outcomes (the fallback GET, and the write's 2xx / 409 /
/// other) directly instead. Production always goes through
/// [`write_config_data`], which supplies `state.proxy.forward` as both
/// `fetch` and `dispatch`.
#[allow(clippy::too_many_arguments)]
async fn write_config_data_with_dispatch<F, FFut, D, DFut>(
    state: &ApiState,
    controller_id: &str,
    namespace: &str,
    name: &str,
    mutate: &(dyn Fn(&mut serde_json::Value) -> Result<(), String> + Send + Sync),
    predicate: &(dyn Fn(&serde_json::Value) -> bool + Send + Sync),
    fetch: F,
    dispatch: D,
) -> WriteOutcome
where
    F: FnOnce(String, String, HashMap<String, String>, Vec<u8>) -> FFut,
    FFut: Future<Output = Result<HttpProxyResponse, (StatusCode, String)>>,
    D: FnOnce(String, String, HashMap<String, String>, Vec<u8>) -> DFut,
    DFut: Future<Output = Result<HttpProxyResponse, (StatusCode, String)>>,
{
    let key = format!("{namespace}/{name}");

    // 1. The CAS precondition normally comes from the local watch cache
    // only. Exception: a non-owning replica's cache legitimately has no
    // entry for this Controller at all (see the module doc comment) — fall
    // back to a proxied GET only in that case, and only when we don't own
    // the session (otherwise the cache is authoritative and a miss means
    // the document genuinely does not exist).
    let (cached, via_fallback_read) = match state
        .sync_client
        .plugin_metadata
        .raw_document(controller_id, &key)
    {
        Some(cached) => (cached, false),
        None if state.proxy.local_session_is_dispatchable(controller_id) => {
            return WriteOutcome::failed(controller_id, "not in the local watch cache");
        }
        None => {
            let read_path = format!("/api/v1/namespaced/edgionconfigdata/{namespace}/{name}");
            let document = match fetch("GET".to_string(), read_path, HashMap::new(), Vec::new())
                .await
            {
                Ok(response) if (200..300).contains(&response.status_code) => {
                    match serde_json::from_slice::<serde_json::Value>(&response.body) {
                        Ok(document) => document,
                        Err(error) => {
                            return WriteOutcome::failed(
                                controller_id,
                                format!(
                                    "not in the local watch cache; fallback read returned an unparseable body: {error}"
                                ),
                            );
                        }
                    }
                }
                Ok(response) => {
                    return WriteOutcome::failed(
                        controller_id,
                        format!(
                            "not in the local watch cache; fallback read returned status {}: {}",
                            response.status_code,
                            response_detail(&response.body)
                        ),
                    );
                }
                Err((status, message)) => {
                    return WriteOutcome::failed(
                        controller_id,
                        format!(
                            "not in the local watch cache; fallback read failed ({status}): {message}"
                        ),
                    );
                }
            };
            (Arc::new(document), true)
        }
    };

    // 2. Never write without a precondition.
    let Some(precondition) = resource_version(&cached) else {
        return WriteOutcome::failed(
            controller_id,
            if via_fallback_read {
                "fallback read document has no resourceVersion"
            } else {
                "document has no resourceVersion"
            },
        );
    };

    // 3. Idempotent skip: the intent already holds, no write needed.
    if predicate(&cached) {
        return WriteOutcome::converged(controller_id, 0);
    }

    // 4. Mutate a clone and PUT it back under the CAS precondition. The
    // mutated document still carries the original `metadata.resourceVersion`
    // — the server, not us, decides the next one.
    let mut document = (*cached).clone();
    if let Err(error) = mutate(&mut document) {
        return WriteOutcome::failed(controller_id, error);
    }
    let body = match serde_json::to_vec(&document) {
        Ok(body) => body,
        Err(error) => {
            return WriteOutcome::failed(
                controller_id,
                format!("failed to serialize document: {error}"),
            );
        }
    };
    let mut headers = HashMap::new();
    headers.insert("content-type".to_string(), "application/json".to_string());
    headers.insert("if-match".to_string(), format!("\"{precondition}\""));
    let path = format!("/api/v1/namespaced/edgionconfigdata/{namespace}/{name}");

    let dispatched_at = Instant::now();
    let response = match dispatch("PUT".to_string(), path, headers, body).await {
        Ok(response) => response,
        Err((status, message)) => {
            return WriteOutcome::failed(
                controller_id,
                format!("write dispatch failed ({status}): {message}"),
            );
        }
    };

    // 5. 409 is terminal and is never retried — the CAS already told us the
    // precondition no longer holds. Any other non-2xx is Failed.
    if response.status_code == 409 {
        return WriteOutcome {
            controller_id: controller_id.to_string(),
            state: OutcomeState::Conflict,
            reason: Some(format!(
                "CAS precondition {precondition} no longer matched: {}",
                response_detail(&response.body)
            )),
            observed: None,
            convergence_ms: None,
        };
    }
    if !(200..300).contains(&response.status_code) {
        return WriteOutcome::failed(
            controller_id,
            format!(
                "write returned status {}: {}",
                response.status_code,
                response_detail(&response.body)
            ),
        );
    }

    // 6. Written, but this replica does not hold the Controller session that
    // feeds its watch cache, so convergence cannot be observed here.
    if !state.proxy.local_session_is_dispatchable(controller_id) {
        return WriteOutcome {
            controller_id: controller_id.to_string(),
            state: OutcomeState::Accepted,
            reason: Some(
                "this replica does not hold the Controller session; convergence cannot be observed here"
                    .to_string(),
            ),
            observed: None,
            convergence_ms: None,
        };
    }

    // 7. Poll the local watch cache — never the Controller — until the
    // version leaves the precondition. Remembers whether the last tick saw
    // the document at all, purely so a timeout can say which of the two it
    // was: "sat unchanged at the precondition" vs. "went missing" —
    // distinct enough situations for an operator that the `Unknown` reason
    // should not read identically for both.
    let mut last_seen_absent: bool;
    loop {
        tokio::time::sleep(CONVERGENCE_POLL_INTERVAL).await;
        let elapsed = dispatched_at.elapsed();

        match state
            .sync_client
            .plugin_metadata
            .raw_document(controller_id, &key)
        {
            Some(observed_doc) => {
                last_seen_absent = false;
                let version_moved =
                    resource_version(&observed_doc).as_deref() != Some(precondition.as_str());
                if version_moved {
                    return if predicate(&observed_doc) {
                        WriteOutcome::converged(controller_id, elapsed.as_millis() as u64)
                    } else {
                        WriteOutcome::superseded(
                            controller_id,
                            observed_doc.as_ref().clone(),
                            elapsed.as_millis() as u64,
                        )
                    };
                }
            }
            None => last_seen_absent = true,
        }

        if elapsed >= CONVERGENCE_BUDGET {
            let reason = if last_seen_absent {
                format!(
                    "no local convergence observed within {}s; the document was last seen absent from the local watch cache",
                    CONVERGENCE_BUDGET.as_secs()
                )
            } else {
                format!(
                    "no local convergence observed within {}s",
                    CONVERGENCE_BUDGET.as_secs()
                )
            };
            return WriteOutcome {
                controller_id: controller_id.to_string(),
                state: OutcomeState::Unknown,
                reason: Some(reason),
                observed: None,
                convergence_ms: None,
            };
        }
    }
}

fn resource_version(document: &serde_json::Value) -> Option<String> {
    document
        .pointer("/metadata/resourceVersion")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn response_detail(body: &[u8]) -> String {
    String::from_utf8_lossy(body).chars().take(500).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use edgion_center_runtime::federation::proto::{CenterMessage, RegisterRequest};

    /// Minimal `ApiState` with a real `CenterWatchCacheRegistry` and a real
    /// `ControllerRegistry`, mirroring the CCI-03 `watch_test_state` builder
    /// pattern (see `api::global_resources::tests`). The write core is
    /// exercised directly against `state.sync_client.plugin_metadata` and
    /// `state.registry`, never through a live gRPC session.
    fn test_state() -> ApiState {
        ApiState::default()
    }

    /// Registers a live, dispatchable session for `controller_id` so
    /// `local_session_is_dispatchable` returns `true` — i.e. this replica is
    /// the one whose watch cache Controller feeds, so it can observe
    /// convergence locally.
    fn register_online_session(state: &ApiState, controller_id: &str) {
        let (tx, _rx) = tokio::sync::mpsc::channel::<CenterMessage>(1);
        state.registry.register(
            controller_id.to_string(),
            RegisterRequest {
                controller_id: controller_id.to_string(),
                ..Default::default()
            },
            tx,
            "session-1".to_string(),
        );
    }

    fn seed_doc(state: &ApiState, controller_id: &str, key: &str, doc: serde_json::Value) {
        state
            .sync_client
            .plugin_metadata
            .get_or_create(controller_id)
            .replace_all(vec![(key.to_string(), doc)], 1, "server-1".to_string());
    }

    fn doc_with_version(version: &str, enabled: bool) -> serde_json::Value {
        json!({
            "apiVersion": "edgion.io/v1",
            "kind": "EdgionConfigData",
            "metadata": {"namespace": "ns", "name": "cfg", "resourceVersion": version},
            "spec": {"enabled": enabled}
        })
    }

    fn enabled_predicate(doc: &serde_json::Value) -> bool {
        doc.pointer("/spec/enabled") == Some(&json!(true))
    }

    fn enable_mutate(doc: &mut serde_json::Value) -> Result<(), String> {
        doc["spec"]["enabled"] = json!(true);
        Ok(())
    }

    /// A `fetch`/`dispatch` stand-in for paths that must not attempt either
    /// call: the cache already had what was needed, or the miss was on an
    /// owning replica where the cache is authoritative.
    fn panics_if_called(
        _method: String,
        _path: String,
        _headers: HashMap<String, String>,
        _body: Vec<u8>,
    ) -> std::future::Ready<Result<HttpProxyResponse, (StatusCode, String)>> {
        panic!("fetch/dispatch must not be called on this path");
    }

    #[tokio::test]
    async fn idempotent_skip_reports_converged_without_writing() {
        let state = test_state();
        seed_doc(&state, "ctrl-1", "ns/cfg", doc_with_version("1", true));

        let outcome = write_config_data_with_dispatch(
            &state,
            "ctrl-1",
            "ns",
            "cfg",
            &enable_mutate,
            &enabled_predicate,
            panics_if_called,
            panics_if_called,
        )
        .await;

        assert_eq!(outcome.state, OutcomeState::Converged);
        assert_eq!(outcome.convergence_ms, Some(0));
        assert!(outcome.reason.is_none());
        assert!(outcome.observed.is_none());
    }

    #[tokio::test]
    async fn missing_document_or_version_fails_before_dispatch() {
        let state = test_state();
        // Both controllers are owning-replica sessions here: the cache is
        // authoritative, so a miss must mean the document genuinely doesn't
        // exist rather than triggering the non-owning-replica fallback read
        // (covered separately by `owning_replica_with_no_cached_document_
        // fails_without_dispatch` and `non_owning_replica_falls_back_to_a_
        // proxy_read_and_reports_accepted`).
        register_online_session(&state, "ctrl-unknown");
        register_online_session(&state, "ctrl-1");

        // No document at all: the controller has no cache entry for this key.
        let outcome = write_config_data_with_dispatch(
            &state,
            "ctrl-unknown",
            "ns",
            "cfg",
            &enable_mutate,
            &enabled_predicate,
            panics_if_called,
            panics_if_called,
        )
        .await;
        assert_eq!(outcome.state, OutcomeState::Failed);
        assert_eq!(
            outcome.reason.as_deref(),
            Some("not in the local watch cache")
        );

        // Document present, but it carries no metadata.resourceVersion.
        seed_doc(
            &state,
            "ctrl-1",
            "ns/cfg",
            json!({
                "apiVersion": "edgion.io/v1",
                "kind": "EdgionConfigData",
                "metadata": {"namespace": "ns", "name": "cfg"},
                "spec": {"enabled": false}
            }),
        );
        let outcome = write_config_data_with_dispatch(
            &state,
            "ctrl-1",
            "ns",
            "cfg",
            &enable_mutate,
            &enabled_predicate,
            panics_if_called,
            panics_if_called,
        )
        .await;
        assert_eq!(outcome.state, OutcomeState::Failed);
        assert_eq!(
            outcome.reason.as_deref(),
            Some("document has no resourceVersion")
        );
    }

    #[tokio::test]
    async fn owning_replica_with_no_cached_document_fails_without_dispatch() {
        let state = test_state();
        register_online_session(&state, "ctrl-1");
        // No document seeded: we own the session, so the cache is
        // authoritative for this Controller — the document genuinely does
        // not exist. Must fail before either fetch or dispatch is called.

        let outcome = write_config_data_with_dispatch(
            &state,
            "ctrl-1",
            "ns",
            "cfg",
            &enable_mutate,
            &enabled_predicate,
            panics_if_called,
            panics_if_called,
        )
        .await;

        assert_eq!(outcome.state, OutcomeState::Failed);
        assert_eq!(
            outcome.reason.as_deref(),
            Some("not in the local watch cache")
        );
    }

    #[tokio::test]
    async fn non_owning_replica_falls_back_to_a_proxy_read_and_reports_accepted() {
        let state = test_state();
        // No document seeded and deliberately no session registered: this
        // replica does not own "ctrl-1"'s Controller session, so its cache
        // legitimately has no entry for it — not because the document
        // doesn't exist. The write must still go through via a fallback GET
        // that the proxy forwards to the owning replica.

        let fetch_calls = Arc::new(AtomicUsize::new(0));
        let fetch_calls_handle = fetch_calls.clone();
        let fetch = move |method: String,
                          path: String,
                          _headers: HashMap<String, String>,
                          _body: Vec<u8>| {
            fetch_calls_handle.fetch_add(1, Ordering::SeqCst);
            assert_eq!(method, "GET");
            assert_eq!(path, "/api/v1/namespaced/edgionconfigdata/ns/cfg");
            std::future::ready(Ok(HttpProxyResponse {
                request_id: "r1".to_string(),
                status_code: 200,
                headers: HashMap::new(),
                body: serde_json::to_vec(&doc_with_version("1", false)).unwrap(),
            }))
        };

        let dispatch_calls = Arc::new(AtomicUsize::new(0));
        let dispatch_calls_handle = dispatch_calls.clone();
        let dispatch = move |method: String,
                             _path: String,
                             _headers: HashMap<String, String>,
                             _body: Vec<u8>| {
            dispatch_calls_handle.fetch_add(1, Ordering::SeqCst);
            assert_eq!(method, "PUT");
            std::future::ready(Ok(HttpProxyResponse {
                request_id: "r2".to_string(),
                status_code: 200,
                headers: HashMap::new(),
                body: Vec::new(),
            }))
        };

        let outcome = write_config_data_with_dispatch(
            &state,
            "ctrl-1",
            "ns",
            "cfg",
            &enable_mutate,
            &enabled_predicate,
            fetch,
            dispatch,
        )
        .await;

        assert_eq!(outcome.state, OutcomeState::Accepted);
        assert_eq!(
            fetch_calls.load(Ordering::SeqCst),
            1,
            "the fallback GET must be attempted on a non-owning replica"
        );
        assert_eq!(
            dispatch_calls.load(Ordering::SeqCst),
            1,
            "the write must still be dispatched even though this replica cannot observe convergence"
        );
    }

    #[tokio::test]
    async fn conflict_is_terminal_and_never_retried() {
        let state = test_state();
        seed_doc(&state, "ctrl-1", "ns/cfg", doc_with_version("1", false));
        register_online_session(&state, "ctrl-1");

        let dispatch_calls = Arc::new(AtomicUsize::new(0));
        let calls = dispatch_calls.clone();
        let dispatch = move |_method: String,
                             _path: String,
                             _headers: HashMap<String, String>,
                             _body: Vec<u8>| {
            calls.fetch_add(1, Ordering::SeqCst);
            std::future::ready(Ok(HttpProxyResponse {
                request_id: "r1".to_string(),
                status_code: 409,
                headers: HashMap::new(),
                body: b"resourceVersion mismatch".to_vec(),
            }))
        };

        let outcome = write_config_data_with_dispatch(
            &state,
            "ctrl-1",
            "ns",
            "cfg",
            &enable_mutate,
            &enabled_predicate,
            panics_if_called,
            dispatch,
        )
        .await;

        assert_eq!(outcome.state, OutcomeState::Conflict);
        assert_eq!(
            dispatch_calls.load(Ordering::SeqCst),
            1,
            "a 409 must never be auto-retried"
        );
    }

    #[tokio::test]
    async fn non_owning_replica_returns_accepted_immediately() {
        let state = test_state();
        seed_doc(&state, "ctrl-1", "ns/cfg", doc_with_version("1", false));
        // Deliberately no session registered: this replica does not hold the
        // Controller session, so it cannot observe convergence. The document
        // is already cached, though, so no fallback read is needed.

        let dispatch =
            |_method: String, _path: String, _headers: HashMap<String, String>, _body: Vec<u8>| {
                std::future::ready(Ok(HttpProxyResponse {
                    request_id: "r1".to_string(),
                    status_code: 200,
                    headers: HashMap::new(),
                    body: Vec::new(),
                }))
            };

        let outcome = write_config_data_with_dispatch(
            &state,
            "ctrl-1",
            "ns",
            "cfg",
            &enable_mutate,
            &enabled_predicate,
            panics_if_called,
            dispatch,
        )
        .await;

        assert_eq!(outcome.state, OutcomeState::Accepted);
        assert_eq!(
            outcome.reason.as_deref(),
            Some(
                "this replica does not hold the Controller session; convergence cannot be observed here"
            )
        );
    }

    #[tokio::test]
    async fn version_moved_with_predicate_false_reports_superseded_with_observed() {
        let state = test_state();
        seed_doc(&state, "ctrl-1", "ns/cfg", doc_with_version("1", false));
        register_online_session(&state, "ctrl-1");

        // The dispatch closure stands in for the write landing at the
        // Controller: as a side effect it advances the local watch cache to
        // a new version, as if the watch stream had caught up — but with
        // the intent overwritten by something else (`enabled: false` again),
        // so the predicate does not hold at the new version.
        let sync_client = state.sync_client.clone();
        let dispatch = move |_method: String,
                             _path: String,
                             _headers: HashMap<String, String>,
                             _body: Vec<u8>| {
            sync_client
                .plugin_metadata
                .get_or_create("ctrl-1")
                .replace_all(
                    vec![("ns/cfg".to_string(), doc_with_version("2", false))],
                    2,
                    "server-1".to_string(),
                );
            std::future::ready(Ok(HttpProxyResponse {
                request_id: "r1".to_string(),
                status_code: 200,
                headers: HashMap::new(),
                body: Vec::new(),
            }))
        };

        let outcome = write_config_data_with_dispatch(
            &state,
            "ctrl-1",
            "ns",
            "cfg",
            &enable_mutate,
            &enabled_predicate,
            panics_if_called,
            dispatch,
        )
        .await;

        assert_eq!(outcome.state, OutcomeState::Superseded);
        assert_eq!(
            outcome.reason.as_deref(),
            Some("overwritten after the write landed")
        );
        assert_eq!(
            outcome.observed,
            Some(doc_with_version("2", false)),
            "observed must carry the document actually in effect"
        );
        assert!(outcome.convergence_ms.is_some());
    }
}
