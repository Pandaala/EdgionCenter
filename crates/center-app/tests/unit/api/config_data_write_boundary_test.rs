use super::*;
use serde_json::json;

#[tokio::test]
async fn cached_status_is_not_written_but_cas_and_operator_fields_survive() {
    let state = ApiState::default();
    let original = json!({
        "metadata":{"name":"cfg","namespace":"ns","resourceVersion":"7","labels":{"owner":"keep"}},
        "status":{"conditions":[]},
        "spec":{"enable":false,"currentStatus":{"conditions":[]},
            "data":{"type":"Custom","config":{"currentStatus":"operator-owned","status":"keep"}}}
    });
    state
        .sync_client
        .plugin_metadata
        .get_or_create("ctrl")
        .replace_all(
            vec![("ns/cfg".to_string(), original.clone())],
            1,
            "server".to_string(),
        );
    let expected = original.clone();
    let dispatch =
        move |method: String, _path: String, headers: HashMap<String, String>, body: Vec<u8>| {
            assert_eq!(method, "PUT");
            assert_eq!(headers.get("if-match").unwrap(), "\"7\"");
            let written: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert!(written.get("status").is_none());
            assert!(written["spec"].get("currentStatus").is_none());
            assert_eq!(written["metadata"], expected["metadata"]);
            assert_eq!(written["spec"]["data"], expected["spec"]["data"]);
            assert_eq!(written["spec"]["enable"], true);
            std::future::ready(Ok(HttpProxyResponse {
                request_id: "test".into(),
                status_code: 409,
                headers: HashMap::new(),
                body: vec![],
            }))
        };
    let outcome = write_config_data_with_dispatch(
        &state,
        "ctrl",
        "ns",
        "cfg",
        &|doc| {
            doc["spec"]["enable"] = json!(true);
            Ok(())
        },
        &|doc| doc.pointer("/spec/enable") == Some(&json!(true)),
        |_, _, _, _| async { panic!("cached resource must not trigger a fresh GET") },
        dispatch,
    )
    .await;
    assert_eq!(outcome.state, OutcomeState::Conflict);
}
