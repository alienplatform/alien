//! Compatibility with JSON emitted by the published @alienplatform/core@3.3.34 npm package.
//! The fixture was generated with Stack/Sandbox builders, all three old modes and
//! .privilegedSupervisor({commandUid:60000}); it is not a hand-authored schema approximation.

#[test]
fn released_stack_reads_all_old_policies_and_writes_only_new_policies() {
    let old = include_str!("fixtures/sandbox-egress-core-3.3.34.json");
    let stack: alien_core::Stack =
        serde_json::from_str(old).expect("released stack remains readable");
    let value = serde_json::to_value(&stack).expect("stack serializes");
    assert_eq!(value["resources"]["deny"]["config"]["egress"], "none");
    assert_eq!(value["resources"]["open"]["config"]["egress"], "internet");
    assert_eq!(
        value["resources"]["targets"]["config"]["egress"],
        serde_json::json!({"allow": ["api.example.com"]})
    );
    let reread: alien_core::Stack = serde_json::from_value(value).expect("new stack round trips");
    assert_eq!(stack.setup_owned_digest(), reread.setup_owned_digest());
}
