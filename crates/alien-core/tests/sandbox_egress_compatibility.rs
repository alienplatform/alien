//! Fixtures emitted by the published @alienplatform/core@3.3.34 builders.
//! Legacy declarations must retain their backend path, setup inputs and supervisor contract.
use alien_core::{EgressPolicy, Sandbox, SandboxEgress, Stack};

#[test]
fn released_stack_preserves_legacy_modes_and_supervisor() {
    let original: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/sandbox-egress-core-3.3.34.json")).unwrap();
    let stack: Stack = serde_json::from_value(original.clone()).unwrap();
    let encoded = serde_json::to_value(&stack).unwrap();
    for id in ["open", "targets"] {
        assert_eq!(
            encoded["resources"][id]["config"]["egress"],
            original["resources"][id]["config"]["egress"]
        );
        let sandbox = stack.resources[id]
            .config
            .downcast_ref::<Sandbox>()
            .unwrap();
        assert!(sandbox.default_policy.is_none());
        assert_eq!(sandbox.cloud_egress(), &SandboxEgress::Allow);
        assert_eq!(
            sandbox.supervisor_environment()["ALIEN_SANDBOX_EGRESS"],
            serde_json::to_string(&sandbox.egress).unwrap()
        );
        let binding: alien_core::SandboxEgressBinding =
            serde_json::from_value(original["resources"][id]["config"]["egress"].clone()).unwrap();
        assert_eq!(binding.declaration(), &sandbox.egress);
        assert!(binding.gateway().is_none());
        assert!(binding.default_policy().is_none());
    }
    let denied = stack.resources["deny"]
        .config
        .downcast_ref::<Sandbox>()
        .unwrap();
    assert_eq!(denied.egress, SandboxEgress::None);
    assert_eq!(
        denied.supervisor_environment()["ALIEN_SANDBOX_EGRESS"],
        r#"{"mode":"deny"}"#
    );
    let saved: Stack = serde_json::from_value(encoded).unwrap();
    assert_eq!(stack.setup_owned_digest(), saved.setup_owned_digest());
}

#[test]
fn released_plans_and_frozen_setup_digest_are_identical() {
    let mut stack: Stack = serde_json::from_str(include_str!(
        "fixtures/sandbox-egress-core-3.3.34-plans.json"
    ))
    .unwrap();
    let live = stack.setup_owned_digest();
    let plans: std::collections::BTreeMap<_, _> = stack.resources.iter().map(|(id, entry)| {
        let sandbox = entry.config.downcast_ref::<Sandbox>().unwrap();
        sandbox.validate_for_platform(alien_core::Platform::Aws).unwrap();
        (id.clone(), serde_json::json!({"cloudEgress":sandbox.cloud_egress(), "environment":sandbox.supervisor_environment(),
            "dependencies":entry.combined_dependencies(),
            "setupInputs":alien_core::sandbox_setup_inputs::aws_sandbox_setup_inputs(&stack, sandbox, entry.lifecycle,
                alien_core::sandbox_setup_inputs::SETUP_INPUTS_COMPARISON_ACCOUNT).unwrap()}))
    }).collect();
    for entry in stack.resources.values_mut() {
        entry.lifecycle = alien_core::ResourceLifecycle::Frozen;
    }
    let actual = serde_json::json!({"liveDigest":live, "frozenDigest":stack.setup_owned_digest(), "plans":plans});
    // Produced by the pre-change core (c542f8f1b) from the actual released SDK fixture.
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/sandbox-egress-core-3.3.34-expected-plan.json"
    ))
    .unwrap();
    assert_eq!(actual, expected);
    let stored: Stack = serde_json::from_slice(&serde_json::to_vec(&stack).unwrap()).unwrap();
    assert_eq!(stack.setup_owned_digest(), stored.setup_owned_digest());
}

#[test]
fn only_new_internet_accepts_runtime_defaults() {
    let stack: Stack = serde_json::from_str(include_str!(
        "fixtures/sandbox-egress-core-3.3.34-plans.json"
    ))
    .unwrap();
    let mut sandbox = stack.resources["direct"]
        .config
        .downcast_ref::<Sandbox>()
        .unwrap()
        .clone();
    sandbox.default_policy = Some(EgressPolicy::Allow { allow: vec![] });
    for declaration in [
        SandboxEgress::None,
        SandboxEgress::Allow,
        SandboxEgress::AllowDomains {
            domains: vec!["example.com".into()],
        },
    ] {
        sandbox.egress = declaration;
        assert_eq!(
            sandbox
                .validate_for_platform(alien_core::Platform::Aws)
                .unwrap_err()
                .code,
            "SANDBOX_EGRESS_INVALID"
        );
    }
    sandbox.egress = SandboxEgress::Internet;
    sandbox
        .validate_for_platform(alien_core::Platform::Aws)
        .unwrap();
    sandbox.default_policy = Some(EgressPolicy::Allow {
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
