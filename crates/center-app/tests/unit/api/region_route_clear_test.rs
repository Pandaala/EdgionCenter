use super::{region_failover_matches, set_region_failover_to};
use serde_json::json;

#[test]
fn clearing_failover_removes_the_optional_target_and_preserves_the_table() {
    let mut document = json!({"spec":{"data":{"config":{"regions":[
        {"name":"east","failoverTo":"west","hashRange":[0,499],"extra":"keep"},
        {"name":"west","hashRange":[500,999]}
    ]}}}});
    let mut expected = document.clone();
    expected["spec"]["data"]["config"]["regions"][0]
        .as_object_mut()
        .unwrap()
        .remove("failoverTo");
    set_region_failover_to(&mut document, "east", "").unwrap();
    assert_eq!(document, expected);
    assert!(region_failover_matches(&document, "east", ""));
}

#[test]
fn invalid_empty_target_is_repaired_instead_of_skipped_as_converged() {
    let mut document =
        json!({"spec":{"data":{"config":{"regions":[{"name":"east","failoverTo":""}]}}}});
    assert!(!region_failover_matches(&document, "east", ""));
    set_region_failover_to(&mut document, "east", "").unwrap();
    assert!(region_failover_matches(&document, "east", ""));
    assert!(!region_failover_matches(&document, "missing", ""));
    set_region_failover_to(&mut document, "east", "west").unwrap();
    assert!(region_failover_matches(&document, "east", "west"));
    assert!(!region_failover_matches(&document, "east", ""));
}
