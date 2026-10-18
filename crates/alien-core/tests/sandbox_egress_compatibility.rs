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
        serde_json::json!({"allow": ["API.Example.COM.", "*.Example.ORG", "git.example.com:022"]})
    );
    let reread: alien_core::Stack = serde_json::from_value(value).expect("new stack round trips");
    assert_eq!(stack.setup_owned_digest(), reread.setup_owned_digest());
}

#[test]
fn released_frozen_stack_keeps_its_setup_approval_after_storage_roundtrip() {
    let mut stack: alien_core::Stack =
        serde_json::from_str(include_str!("fixtures/sandbox-egress-core-3.3.34.json"))
            .expect("released stack remains readable");
    for entry in stack.resources.values_mut() {
        entry.lifecycle = alien_core::ResourceLifecycle::Frozen;
    }
    let digest = stack.setup_owned_digest();
    let saved = serde_json::to_vec(&stack).expect("stack saves");
    let loaded: alien_core::Stack = serde_json::from_slice(&saved).expect("stack reloads");
    assert_eq!(digest, loaded.setup_owned_digest());
}
