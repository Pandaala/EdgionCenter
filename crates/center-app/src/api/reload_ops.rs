//! Outcome core for a Center-initiated Controller reload.
//!
//! # Why reload needs an outcome at all
//!
//! Reload is the only state-changing operation in the default Center policy
//! that is not an `EdgionConfigData` write, and it is the heaviest one: the
//! Controller rebuilds its ConfigSyncServer, mints a new `server_id`, runs a
//! full `Init -> InitApply -> InitDone` cycle, and every Gateway re-lists.
//!
//! The Controller's own answer is honest but limited: `request_reload()` is a
//! queue push, so its `200` says "initiated", nothing more. Reporting that
//! `200` to the operator as plain success is what made a reload that started
//! and then wedged produce no signal anywhere. This module supplies the
//! missing half — a real terminal state — using the same discipline as the
//! `EdgionConfigData` write path in [`super::config_data_ops`], and the same
//! frozen [`OutcomeState`] vocabulary so both render through one component.
//!
//! # Why the completion signal is the cached `server_id`
//!
//! A reload mints a new `server_id`, and `server_id` rides every
//! `FedWatchListResponse`/`FedWatchEventResponse`. Center already reacts to a
//! change by re-watching from zero, and the re-list stores the new id in the
//! local watch cache. So "the reload completed" already arrives in-process for
//! free; all that was missing was correlating it with the request that caused
//! it. We therefore poll the LOCAL cache only — never the Controller — for
//! the same reason the write path does: the federation watch stream already
//! keeps the cache current, so convergence costs no extra round trip.
//!
//! A wedged reload never advances that id, which is precisely why the wedged
//! case can be reported as `Unknown` rather than success.
//!
//! # Why the budget is not the write path's 10 s
//!
//! A full `Init -> InitApply -> InitDone` on a large cluster is slower than a
//! single-document write, so the write path's 10 s is too tight. It is also
//! bounded from above by the client: the dashboard's axios timeout is 30 s and
//! the proxy tunnel's own per-request deadline is 25 s, so a budget at or near
//! 30 s would surface as a browser-side abort — the operator would lose the
//! outcome entirely, which is the failure this module exists to remove.
//! [`RELOAD_BUDGET`] is therefore 20 s measured from handler entry (dispatch
//! time included), leaving 10 s of headroom so a real outcome always reaches
//! the operator. A reload slower than that reports `Unknown`, which is the
//! honest answer: dispatched, completion not observed within this request.
//!
//! Two caveats on that arithmetic:
//!
//! - The 25 s tunnel deadline is `sync.command_timeout_secs`, which is
//!   **operator-configurable**. Raising it above the axios timeout re-opens the
//!   browser-abort hole for a slow dispatch, because the dispatch is inside the
//!   budget but is not bounded by it. The budget itself is guarded by a test;
//!   the tunnel deadline is not, so treat that config field as part of this
//!   contract.
//! - Roughly 4 s of the budget is fixed overhead before the Controller's
//!   restart can even be observed: its reload detector polls at 1 s intervals
//!   before signalling the watch, and Center's re-watch backoff sleeps 3 s
//!   before re-listing. The effective window for `Init -> InitApply ->
//!   InitDone` is therefore closer to 16 s than to 20 s.
//!
//! # Two Controller answers that must stay distinguishable
//!
//! `503` + `Retry-After` (the Controller is transiently unable to start one —
//! retrying is meaningful) and `409 not leader` (this Controller replica is a
//! follower — retrying is pointless, the operator needs the leader address the
//! body carries) both mean "nothing was applied", but they call for opposite
//! operator actions. They are carried on [`ReloadOutcome::retry_after_secs`]
//! and [`ReloadOutcome::leader`] rather than as new `OutcomeState` variants:
//! the six-state vocabulary is shared with the write path and stays frozen.

use std::collections::HashMap;
use std::future::Future;
use std::time::{Duration, Instant};

use http::StatusCode;

use edgion_center_runtime::federation::proto::HttpProxyResponse;

use super::config_data_ops::OutcomeState;
use super::ApiState;

/// Cadence for local-cache `server_id` polling after a dispatched reload.
pub const RELOAD_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// Total budget, measured from handler entry, for observing that the reload
/// completed. See the module docs for why this is 20 s and not the write
/// path's 10 s (or the Controller's own 30 s `Retry-After` hint).
pub const RELOAD_BUDGET: Duration = Duration::from_secs(20);

/// The Controller Admin API path a Center reload forwards to.
const RELOAD_PATH: &str = "/api/v1/reload";

/// Terminal state of one Center-initiated reload.
///
/// Reuses [`OutcomeState`] so the dashboard renders reload and
/// `EdgionConfigData` writes through the same component. The reload path
/// produces five of the six states; `Superseded` has no meaning here (there
/// is no document to be overwritten) and is never constructed.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReloadOutcome {
    pub controller_id: String,
    pub state: OutcomeState,
    /// Why the terminal state was reached. Absent on a clean `Converged`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The new `server_id` observed after the reload completed. Present only
    /// on `Converged` — it is the evidence for that verdict.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    /// Measured time from handler entry to observation. Telemetry only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub convergence_ms: Option<u64>,
    /// Seconds from the Controller's `Retry-After` when it refused transiently.
    /// Present means "retry is meaningful" — the operator action differs from
    /// every other `Failed`. It does NOT say *why* the Controller refused
    /// (already queued, or config center not ready); `reason` carries that.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<u64>,
    /// The leader's admin address from a `409 not leader` body, when the
    /// Controller knew it (`null` mid-election). Present means "retrying here
    /// is pointless — go to this address instead".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leader: Option<String>,
}

impl ReloadOutcome {
    fn new(controller_id: &str, state: OutcomeState) -> Self {
        Self {
            controller_id: controller_id.to_string(),
            state,
            reason: None,
            server_id: None,
            convergence_ms: None,
            retry_after_secs: None,
            leader: None,
        }
    }

    fn with_reason(controller_id: &str, state: OutcomeState, reason: impl Into<String>) -> Self {
        Self {
            reason: Some(reason.into()),
            ..Self::new(controller_id, state)
        }
    }
}

/// Dispatch a reload to `controller_id` over the federation proxy tunnel and
/// report whether it actually completed.
///
/// `Err` is reserved for failures that never reached the Controller (unknown
/// or offline controller, stale ownership, tunnel timeout). Those carry the
/// proxy's own status so the handler can pass it through unchanged; they are
/// deliberately NOT modelled as a `Failed` outcome, which would flatten a
/// `404 unknown controller` into a generic reload failure.
pub async fn reload_controller(
    state: &ApiState,
    controller_id: &str,
) -> Result<ReloadOutcome, (StatusCode, String)> {
    reload_with_dispatch(
        state,
        controller_id,
        RELOAD_BUDGET,
        |method, path, headers, body| {
            state
                .proxy
                .forward(controller_id, method, path, headers, body)
        },
    )
    .await
}

/// Same sequence as [`reload_controller`], with the dispatch and the budget
/// injectable. `ProxyForwarder::forward` needs a live, registered Controller
/// session and drives an actual gRPC bidirectional stream, so it cannot be
/// faked from `center-app`; this seam lets tests drive the Controller's answer
/// (2xx / 409 / 503 / other) and the cache side effects directly. The budget
/// is a parameter for the same reason — the wedged case must be reachable in a
/// test without spending [`RELOAD_BUDGET`] of real time. Production always
/// goes through [`reload_controller`], which supplies [`RELOAD_BUDGET`].
async fn reload_with_dispatch<D, DFut>(
    state: &ApiState,
    controller_id: &str,
    budget: Duration,
    dispatch: D,
) -> Result<ReloadOutcome, (StatusCode, String)>
where
    D: FnOnce(String, String, HashMap<String, String>, Vec<u8>) -> DFut,
    DFut: Future<Output = Result<HttpProxyResponse, (StatusCode, String)>>,
{
    let started = Instant::now();

    // 1. Baseline BEFORE dispatch, from the local cache — no extra round
    // trip. Captured first so a reload that completes unusually fast cannot
    // land between dispatch and baseline capture and be missed.
    let baseline = state
        .sync_client
        .plugin_metadata
        .cached_server_id(controller_id);

    let response = dispatch(
        "POST".to_string(),
        RELOAD_PATH.to_string(),
        HashMap::new(),
        Vec::new(),
    )
    .await?;

    // 2. The two answers that mean "nothing was applied" but call for
    // opposite operator actions. Both are terminal and never retried here.
    if response.status_code == 409 {
        return Ok(not_leader_outcome(controller_id, &response.body));
    }
    if response.status_code == 503 {
        // `request_reload()` fails for more than one transient reason — a
        // reload is already queued, or the config center is not started/ready
        // yet — and the Controller answers 503 + `Retry-After: 5` for all of
        // them. Retrying is the right action either way, so they share one
        // state, but the reason must NOT assert which one it was; the
        // Controller's own body is what says that, so it is carried verbatim.
        let mut outcome = ReloadOutcome::with_reason(
            controller_id,
            OutcomeState::Failed,
            format!(
                "the Controller refused the reload as temporarily unavailable: {}",
                response_detail(&response.body)
            ),
        );
        // Default to the Controller's documented hint when the header is
        // absent or unparseable: the condition is transient either way, and
        // dropping the field would render as a permanent failure.
        outcome.retry_after_secs = Some(retry_after_secs(&response.headers).unwrap_or(5));
        return Ok(outcome);
    }
    if !(200..300).contains(&response.status_code) {
        return Ok(ReloadOutcome::with_reason(
            controller_id,
            OutcomeState::Failed,
            format!(
                "reload returned status {}: {}",
                response.status_code,
                response_detail(&response.body)
            ),
        ));
    }

    // 3. Dispatched, but this replica cannot see the Controller's watch
    // stream, so completion is unobservable here. `Accepted`, not `Unknown`:
    // waiting out the budget would report the same thing 20 s later.
    if !state.proxy.local_session_is_dispatchable(controller_id) {
        return Ok(ReloadOutcome::with_reason(
            controller_id,
            OutcomeState::Accepted,
            "this replica does not hold the Controller session; reload completion cannot be observed here",
        ));
    }
    let Some(baseline) = baseline else {
        return Ok(ReloadOutcome::with_reason(
            controller_id,
            OutcomeState::Accepted,
            "no cached server_id to compare against; reload completion cannot be observed here",
        ));
    };

    // 4. Poll the local cache until the `server_id` leaves the baseline.
    //
    // Observe BEFORE testing the budget, and before the first sleep. The
    // dispatch itself is inside the budget but is bounded by the *tunnel*
    // deadline, which is longer — so a congested tunnel can return after the
    // budget is already spent. Testing the budget first would report `Unknown`
    // without ever looking at the cache the new `server_id` may already be
    // sitting in. Sleeping first would do the same to a reload that completed
    // during dispatch.
    loop {
        // A cache that goes absent is not convergence: the entry is only
        // repopulated by a re-list, which is what carries the new id.
        if let Some(observed) = state
            .sync_client
            .plugin_metadata
            .cached_server_id(controller_id)
        {
            if observed != baseline {
                let mut outcome = ReloadOutcome::new(controller_id, OutcomeState::Converged);
                outcome.convergence_ms = Some(started.elapsed().as_millis() as u64);
                outcome.server_id = Some(observed);
                return Ok(outcome);
            }
        }

        if started.elapsed() >= budget {
            return Ok(ReloadOutcome::with_reason(
                controller_id,
                OutcomeState::Unknown,
                format!(
                    "reload was dispatched, but no new server_id was observed within {}s; the Controller may still be restarting",
                    budget.as_secs()
                ),
            ));
        }

        tokio::time::sleep(RELOAD_POLL_INTERVAL).await;
    }
}

/// Build the `409 not leader` outcome, lifting the leader address out of the
/// Controller's body (`{"error":"not leader","leader":<addr|null>}`). The
/// address is the whole point of surfacing this state separately, so a body
/// we cannot parse still yields `Conflict` — just without the address.
fn not_leader_outcome(controller_id: &str, body: &[u8]) -> ReloadOutcome {
    let parsed = serde_json::from_slice::<serde_json::Value>(body).ok();
    let leader = parsed
        .as_ref()
        .and_then(|value| value.get("leader"))
        .and_then(serde_json::Value::as_str)
        .filter(|leader| !leader.is_empty())
        .map(str::to_string);
    let mut outcome = ReloadOutcome::with_reason(
        controller_id,
        OutcomeState::Conflict,
        "this Controller replica is a follower and refused the reload",
    );
    outcome.leader = leader;
    outcome
}

/// `Retry-After` in delta-seconds form. The federation tunnel forwards
/// response headers with lowercase names (they come from a `HeaderMap`), but
/// match case-insensitively anyway rather than depend on that. An HTTP-date
/// `Retry-After` is not parsed — the Controller only ever sends delta-seconds
/// on this path, and guessing at a date would be worse than falling back to
/// the documented default.
fn retry_after_secs(headers: &HashMap<String, String>) -> Option<u64> {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("retry-after"))
        .and_then(|(_, value)| value.trim().parse::<u64>().ok())
}

fn response_detail(body: &[u8]) -> String {
    String::from_utf8_lossy(body).chars().take(500).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use crate::{
        aggregator::ResourceAggregator,
        fed_sync::registry::ControllerRegistry,
        metadata_store::CenterMetaDataStore,
        proxy::ProxyForwarder,
        watch_cache::{CenterSyncClient, CenterWatchCacheRegistry},
    };
    use edgion_center_runtime::federation::proto::{CenterMessage, RegisterRequest};

    /// Minimal `ApiState` with a real watch-cache registry and controller
    /// registry, mirroring `config_data_ops::tests::test_state`. The reload
    /// core is exercised against `state.sync_client.plugin_metadata` and
    /// `state.registry`, never through a live gRPC session.
    fn test_state() -> ApiState {
        let registry = ControllerRegistry::new();
        let metadata_store = Arc::new(CenterMetaDataStore::new());
        let sync_client = Arc::new(CenterSyncClient {
            plugin_metadata: CenterWatchCacheRegistry::new(metadata_store.clone()),
        });
        let proxy = Arc::new(ProxyForwarder::new(
            registry.clone(),
            Arc::new(Mutex::new(HashMap::new())),
            5,
        ));
        ApiState {
            aggregator: Arc::new(ResourceAggregator::new()),
            proxy,
            controller_directory: None,
            controller_evictor: Arc::new(edgion_center_runtime::eviction::NoopControllerEvictor),
            user_admin: None,
            role_admin: None,
            audit_reader: None,
            cloudflare_dns_admin: None,
            cloudflare_dns_write_admin: None,
            cloudflare_waf_admin: None,
            route53_dns_admin: None,
            route53_dns_write_admin: None,
            route53_zone_lifecycle_admin: None,
            cloudfront_admin: None,
            aws_waf_admin: None,
            provider_account_store: None,
            capability_snapshot_store: None,
            credential_inspection_service: None,
            metadata_store,
            sync_client,
            registry,
            platform_ready: Arc::new(std::sync::atomic::AtomicBool::new(true)),
            authz_mode: edgion_center_core::AuthzMode::AllowAll,
            platform_mode: edgion_center_core::CenterMode::Standalone,
            capabilities: edgion_center_core::CenterCapabilities::for_mode(
                edgion_center_core::CenterMode::Standalone,
            ),
        }
    }

    /// Registers a live, dispatchable session so
    /// `local_session_is_dispatchable` returns `true` — i.e. this replica is
    /// the one the Controller's watch stream feeds.
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

    /// Applies a list batch, which is what stamps `server_id` onto the cache.
    fn seed_server_id(state: &ApiState, controller_id: &str, server_id: &str, sync_version: u64) {
        state
            .sync_client
            .plugin_metadata
            .get_or_create(controller_id)
            .replace_all(Vec::new(), sync_version, server_id.to_string());
    }

    fn ok_response() -> HttpProxyResponse {
        HttpProxyResponse {
            request_id: "r1".to_string(),
            status_code: 200,
            headers: HashMap::new(),
            body: b"Reload initiated - controllers will restart with new server_id".to_vec(),
        }
    }

    /// What a canned test dispatch resolves to — the same `Result` the real
    /// `ProxyForwarder::forward` yields, already ready.
    type CannedDispatch = std::future::Ready<Result<HttpProxyResponse, (StatusCode, String)>>;

    /// A dispatch stand-in that answers with `response` and asserts the request
    /// shape the reload path must always send.
    fn respond(
        response: HttpProxyResponse,
    ) -> impl FnOnce(String, String, HashMap<String, String>, Vec<u8>) -> CannedDispatch {
        move |method, path, _headers, body| {
            assert_eq!(method, "POST");
            assert_eq!(path, RELOAD_PATH);
            assert!(body.is_empty(), "reload carries no request body");
            std::future::ready(Ok(response))
        }
    }

    #[tokio::test]
    async fn transport_failure_passes_the_proxy_status_through_untouched() {
        let state = test_state();
        // No session registered: production `forward` answers 404 here, and
        // that must NOT be flattened into a `Failed` reload outcome.
        let dispatch =
            |_method: String, _path: String, _headers: HashMap<String, String>, _body: Vec<u8>| {
                std::future::ready(Err((
                    StatusCode::NOT_FOUND,
                    "Controller cluster/c0 not found or offline".to_string(),
                )))
            };

        let result = reload_with_dispatch(&state, "cluster/c0", RELOAD_BUDGET, dispatch).await;

        let (status, message) = result.expect_err("a transport failure must not become an outcome");
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(message.contains("not found or offline"));
    }

    #[tokio::test]
    async fn new_server_id_in_the_cache_reports_converged() {
        let state = test_state();
        register_online_session(&state, "ctrl-1");
        seed_server_id(&state, "ctrl-1", "server-1", 1);

        // The dispatch closure stands in for the reload landing: as a side
        // effect the Controller comes back with a fresh server_id and Center
        // re-lists, exactly as the watch stream would drive it.
        let sync_client = state.sync_client.clone();
        let dispatch = move |_method: String,
                             _path: String,
                             _headers: HashMap<String, String>,
                             _body: Vec<u8>| {
            sync_client
                .plugin_metadata
                .get_or_create("ctrl-1")
                .replace_all(Vec::new(), 2, "server-2".to_string());
            std::future::ready(Ok(ok_response()))
        };

        let outcome = reload_with_dispatch(&state, "ctrl-1", RELOAD_BUDGET, dispatch)
            .await
            .expect("the Controller answered");

        assert_eq!(outcome.state, OutcomeState::Converged);
        assert_eq!(outcome.server_id.as_deref(), Some("server-2"));
        assert!(outcome.convergence_ms.is_some());
        assert!(outcome.reason.is_none());
    }

    /// The wedged reload: the Controller accepted it, but the ConfigSyncServer
    /// never came back, so the cached `server_id` never moves. This is the
    /// case that used to render as a green toast.
    ///
    /// Runs on a 1 s budget rather than [`RELOAD_BUDGET`] — the branch under
    /// test is budget-exhaustion, and spending 20 s of wall clock to reach it
    /// would be the only slow test in the crate. `budget_is_bounded_by_the_
    /// client_timeout` covers the production value.
    #[tokio::test]
    async fn dispatched_reload_that_never_completes_reports_unknown() {
        let state = test_state();
        register_online_session(&state, "ctrl-1");
        seed_server_id(&state, "ctrl-1", "server-1", 1);

        let outcome = reload_with_dispatch(
            &state,
            "ctrl-1",
            Duration::from_secs(1),
            respond(ok_response()),
        )
        .await
        .expect("the Controller answered");

        assert_eq!(outcome.state, OutcomeState::Unknown);
        assert!(
            outcome
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("no new server_id was observed within 1s")),
            "unknown must say what was not observed, got {:?}",
            outcome.reason
        );
        assert!(outcome.server_id.is_none());
        assert!(outcome.convergence_ms.is_none());
    }

    /// The budget is bounded from above by the client, not just by how long a
    /// reload takes: the dashboard's axios timeout is 30 s, so a budget at or
    /// near it turns a real outcome into a browser-side abort — reintroducing
    /// exactly the "no signal anywhere" failure this module removes. Guarding
    /// the constant keeps a future "let's wait the full 30 s" edit honest.
    #[test]
    fn budget_is_bounded_by_the_client_timeout() {
        const DASHBOARD_AXIOS_TIMEOUT: Duration = Duration::from_secs(30);
        assert_eq!(RELOAD_BUDGET, Duration::from_secs(20));
        assert!(
            RELOAD_BUDGET + Duration::from_secs(5) <= DASHBOARD_AXIOS_TIMEOUT,
            "the budget must leave the client room to receive the outcome"
        );
        assert!(
            RELOAD_POLL_INTERVAL < RELOAD_BUDGET,
            "the poll loop must get at least one observation in"
        );
    }

    #[tokio::test]
    async fn non_owning_replica_reports_accepted_without_waiting_out_the_budget() {
        let state = test_state();
        seed_server_id(&state, "ctrl-1", "server-1", 1);
        // Deliberately no session registered: this replica does not hold the
        // Controller session, so it can never observe the new server_id.

        let outcome = reload_with_dispatch(&state, "ctrl-1", RELOAD_BUDGET, respond(ok_response()))
            .await
            .expect("the Controller answered");

        assert_eq!(outcome.state, OutcomeState::Accepted);
        assert_eq!(
            outcome.reason.as_deref(),
            Some(
                "this replica does not hold the Controller session; reload completion cannot be observed here"
            )
        );
    }

    #[tokio::test]
    async fn missing_baseline_reports_accepted_rather_than_guessing() {
        let state = test_state();
        register_online_session(&state, "ctrl-1");
        // Cache present as a session but never listed, so there is no
        // server_id to compare against. Treating the first observed id as
        // convergence would report success for a reload that never ran.

        let outcome = reload_with_dispatch(&state, "ctrl-1", RELOAD_BUDGET, respond(ok_response()))
            .await
            .expect("the Controller answered");

        assert_eq!(outcome.state, OutcomeState::Accepted);
        assert_eq!(
            outcome.reason.as_deref(),
            Some(
                "no cached server_id to compare against; reload completion cannot be observed here"
            )
        );
    }

    /// A 503 is transient whatever caused it, so the retry hint is what the
    /// operator acts on — but the reason must not name a cause. The Controller
    /// answers 503 both for "already queued" and for "config center not ready",
    /// and only its own body distinguishes them, so the body is carried through
    /// verbatim instead of being paraphrased into one of the two.
    #[tokio::test]
    async fn transient_refusal_is_failed_with_a_retry_hint_and_the_upstream_cause() {
        let state = test_state();
        register_online_session(&state, "ctrl-1");
        seed_server_id(&state, "ctrl-1", "server-1", 1);

        for cause in [
            "reload unavailable: reload already in progress",
            "reload unavailable: Center not started or not ready for reload",
        ] {
            let response = HttpProxyResponse {
                request_id: "r1".to_string(),
                status_code: 503,
                headers: HashMap::from([("retry-after".to_string(), "5".to_string())]),
                body: serde_json::to_vec(&json!({"success": false, "error": cause})).unwrap(),
            };

            let outcome = reload_with_dispatch(&state, "ctrl-1", RELOAD_BUDGET, respond(response))
                .await
                .expect("the Controller answered");

            assert_eq!(outcome.state, OutcomeState::Failed);
            assert_eq!(
                outcome.retry_after_secs,
                Some(5),
                "a transient refusal must carry the retry hint that distinguishes it"
            );
            assert!(outcome.leader.is_none());
            let reason = outcome
                .reason
                .expect("a transient refusal must explain itself");
            assert!(
                reason.contains(cause),
                "the Controller's own cause must survive verbatim, got {reason}"
            );
            assert!(
                !reason.contains("already in progress") || cause.contains("already in progress"),
                "the reason must not assert a cause the Controller did not report, got {reason}"
            );
        }
    }

    #[tokio::test]
    async fn retry_hint_falls_back_to_the_documented_default_when_the_header_is_unusable() {
        let state = test_state();
        register_online_session(&state, "ctrl-1");

        for headers in [
            HashMap::new(),
            HashMap::from([(
                "Retry-After".to_string(),
                "Wed, 21 Oct 2026 07:28:00 GMT".to_string(),
            )]),
        ] {
            let response = HttpProxyResponse {
                request_id: "r1".to_string(),
                status_code: 503,
                headers,
                body: Vec::new(),
            };
            let outcome = reload_with_dispatch(&state, "ctrl-1", RELOAD_BUDGET, respond(response))
                .await
                .expect("the Controller answered");
            assert_eq!(outcome.state, OutcomeState::Failed);
            assert_eq!(
                outcome.retry_after_secs,
                Some(5),
                "the condition is transient regardless of the header, so the field must be set"
            );
        }
    }

    #[tokio::test]
    async fn retry_hint_reads_the_header_case_insensitively() {
        let state = test_state();
        register_online_session(&state, "ctrl-1");

        let response = HttpProxyResponse {
            request_id: "r1".to_string(),
            status_code: 503,
            headers: HashMap::from([("Retry-After".to_string(), " 12 ".to_string())]),
            body: Vec::new(),
        };
        let outcome = reload_with_dispatch(&state, "ctrl-1", RELOAD_BUDGET, respond(response))
            .await
            .expect("the Controller answered");

        assert_eq!(outcome.retry_after_secs, Some(12));
    }

    #[tokio::test]
    async fn follower_replica_is_conflict_and_carries_the_leader_address() {
        let state = test_state();
        register_online_session(&state, "ctrl-1");
        seed_server_id(&state, "ctrl-1", "server-1", 1);

        let response = HttpProxyResponse {
            request_id: "r1".to_string(),
            status_code: 409,
            headers: HashMap::new(),
            body: serde_json::to_vec(&json!({"error": "not leader", "leader": "10.0.0.7:12101"}))
                .unwrap(),
        };

        let outcome = reload_with_dispatch(&state, "ctrl-1", RELOAD_BUDGET, respond(response))
            .await
            .expect("the Controller answered");

        assert_eq!(outcome.state, OutcomeState::Conflict);
        assert_eq!(
            outcome.leader.as_deref(),
            Some("10.0.0.7:12101"),
            "retrying a follower is pointless, so the leader address is the actionable part"
        );
        assert!(outcome.retry_after_secs.is_none());
    }

    #[tokio::test]
    async fn follower_replica_with_no_known_leader_is_still_conflict() {
        let state = test_state();
        register_online_session(&state, "ctrl-1");

        // `leader` is null mid-election, and an unparseable body is possible
        // too. Neither may downgrade the state.
        for body in [
            serde_json::to_vec(&json!({"error": "not leader", "leader": null})).unwrap(),
            b"not json".to_vec(),
        ] {
            let response = HttpProxyResponse {
                request_id: "r1".to_string(),
                status_code: 409,
                headers: HashMap::new(),
                body,
            };
            let outcome = reload_with_dispatch(&state, "ctrl-1", RELOAD_BUDGET, respond(response))
                .await
                .expect("the Controller answered");
            assert_eq!(outcome.state, OutcomeState::Conflict);
            assert!(outcome.leader.is_none());
        }
    }

    #[tokio::test]
    async fn other_non_2xx_answers_are_failed_with_the_upstream_detail() {
        let state = test_state();
        register_online_session(&state, "ctrl-1");

        let response = HttpProxyResponse {
            request_id: "r1".to_string(),
            status_code: 403,
            headers: HashMap::new(),
            body: b"forbidden: verb reload not granted".to_vec(),
        };

        let outcome = reload_with_dispatch(&state, "ctrl-1", RELOAD_BUDGET, respond(response))
            .await
            .expect("the Controller answered");

        assert_eq!(outcome.state, OutcomeState::Failed);
        let reason = outcome.reason.expect("failed must explain itself");
        assert!(reason.contains("403"), "got {reason}");
        assert!(reason.contains("verb reload not granted"), "got {reason}");
        assert!(outcome.retry_after_secs.is_none());
    }

    /// A dispatch slow enough to spend the whole budget must still get one
    /// look at the cache. The dispatch is inside the budget but is bounded by
    /// the (longer, operator-configurable) tunnel deadline, so this is
    /// reachable on a congested tunnel — and reporting `Unknown` while the new
    /// `server_id` already sits in the cache would be a false negative.
    #[tokio::test]
    async fn a_dispatch_that_outlasts_the_budget_still_observes_the_cache() {
        let state = test_state();
        register_online_session(&state, "ctrl-1");
        seed_server_id(&state, "ctrl-1", "server-1", 1);

        let sync_client = state.sync_client.clone();
        let dispatch = move |_method: String,
                             _path: String,
                             _headers: HashMap<String, String>,
                             _body: Vec<u8>| {
            sync_client
                .plugin_metadata
                .get_or_create("ctrl-1")
                .replace_all(Vec::new(), 2, "server-2".to_string());
            std::future::ready(Ok(ok_response()))
        };

        // A zero budget is already exhausted by the time dispatch returns.
        let outcome = reload_with_dispatch(&state, "ctrl-1", Duration::ZERO, dispatch)
            .await
            .expect("the Controller answered");

        assert_eq!(
            outcome.state,
            OutcomeState::Converged,
            "an exhausted budget must not pre-empt an observation that is already available"
        );
        assert_eq!(outcome.server_id.as_deref(), Some("server-2"));
    }

    /// The baseline is captured before dispatch so a reload that completes
    /// while the dispatch call is still in flight is still observed as a
    /// change rather than being adopted as the baseline.
    #[tokio::test]
    async fn baseline_is_captured_before_dispatch() {
        let state = test_state();
        register_online_session(&state, "ctrl-1");
        seed_server_id(&state, "ctrl-1", "server-1", 1);

        let sync_client = state.sync_client.clone();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_handle = calls.clone();
        let dispatch = move |_method: String,
                             _path: String,
                             _headers: HashMap<String, String>,
                             _body: Vec<u8>| {
            calls_handle.fetch_add(1, Ordering::SeqCst);
            // Completion lands during dispatch, before the poll loop starts.
            sync_client
                .plugin_metadata
                .get_or_create("ctrl-1")
                .replace_all(Vec::new(), 2, "server-2".to_string());
            std::future::ready(Ok(ok_response()))
        };

        let outcome = reload_with_dispatch(&state, "ctrl-1", RELOAD_BUDGET, dispatch)
            .await
            .expect("the Controller answered");

        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(outcome.state, OutcomeState::Converged);
        assert_eq!(outcome.server_id.as_deref(), Some("server-2"));
    }
}
