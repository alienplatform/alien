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
        value["resources"]["targets"]["config"]["defaultPolicy"],
        serde_json::json!({"allow": ["API.Example.COM.", "*.Example.ORG", "git.example.com:022"]})
    );
    assert_eq!(
        value["resources"]["targets"]["config"]["egress"],
        "internet"
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

#[test]
fn legacy_domain_policy_is_a_default_and_never_a_declaration_bound() {
    let stack: alien_core::Stack =
        serde_json::from_str(include_str!("fixtures/sandbox-egress-core-3.3.34.json")).unwrap();
    let sandbox = stack.resources["targets"]
        .config
        .downcast_ref::<alien_core::Sandbox>()
        .unwrap();
    assert_eq!(sandbox.egress, alien_core::SandboxEgress::Internet);
    let policy = sandbox.default_policy.as_ref().unwrap();
    assert!(policy.allows("api.example.com", 8443).unwrap());
    assert!(policy.allows("example.org", 443).unwrap());
    assert!(policy.allows("git.example.com", 22).unwrap());
    assert!(!policy.allows("git.example.com", 443).unwrap());
    let gateway_default: alien_core::bindings::SandboxEgressBinding = serde_json::from_value(
        serde_json::json!({"mode":"allowDomains","domains":["*.example.org"]}),
    )
    .unwrap();
    assert_eq!(
        gateway_default.declaration(),
        &alien_core::SandboxEgress::Internet
    );
    assert!(gateway_default
        .default_policy()
        .unwrap()
        .allows("example.org", 8443)
        .unwrap());
}

#[test]
fn runtime_defaults_are_validated_independently_of_the_declaration() {
    let stack: alien_core::Stack =
        serde_json::from_str(include_str!("fixtures/sandbox-egress-core-3.3.34.json")).unwrap();
    let mut sandbox = stack.resources["open"]
        .config
        .downcast_ref::<alien_core::Sandbox>()
        .unwrap()
        .clone();
    sandbox.privileged_supervisor = None;
    sandbox.default_policy = Some(alien_core::EgressPolicy::Allow { allow: vec![] });
    sandbox
        .validate_for_platform(alien_core::Platform::Aws)
        .unwrap();
    sandbox.egress = alien_core::SandboxEgress::None;
    assert_eq!(
        sandbox
            .validate_for_platform(alien_core::Platform::Aws)
            .unwrap_err()
            .code,
        "SANDBOX_EGRESS_INVALID"
    );
    sandbox.egress = alien_core::SandboxEgress::Internet;
    sandbox.default_policy = Some(alien_core::EgressPolicy::Allow {
        allow: vec!["169.254.169.254".into()],
    });
    assert_eq!(
        sandbox
            .validate_for_platform(alien_core::Platform::Aws)
            .unwrap_err()
            .code,
        "SANDBOX_EGRESS_INVALID"
    );
}
