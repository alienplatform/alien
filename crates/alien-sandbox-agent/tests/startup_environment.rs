use alien_core::{
    EgressPolicy, Platform, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy,
    SandboxPrivilegedSupervisor,
};

#[test]
fn released_supervisor_policies_keep_their_startup_meaning() {
    for encoded in [
        r#"{"mode":"deny"}"#,
        r#"{"mode":"allow"}"#,
        r#"{"mode":"allowDomains","domains":["api.example.com"]}"#,
    ] {
        let expected: SandboxEgress = serde_json::from_str(encoded).unwrap();
        let mut sandbox = Sandbox::new("legacy".into())
            .code(SandboxCode::Image {
                image: "ubuntu".into(),
            })
            .egress(expected.clone())
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();
        sandbox.privileged_supervisor = Some(SandboxPrivilegedSupervisor { command_uid: 60001 });
        let env = sandbox.supervisor_environment();
        let policy: SandboxEgress = serde_json::from_str(&env["ALIEN_SANDBOX_EGRESS"]).unwrap();
        assert_eq!(policy, expected);
        // The runtime must also recognize this as a released object policy.
        serde_json::from_str::<alien_sandbox_agent::egress::LegacyEgress>(
            &env["ALIEN_SANDBOX_EGRESS"],
        )
        .unwrap();
    }
}

#[test]
fn runtime_defaults_do_not_become_the_startup_network_floor() {
    let mut sandbox = Sandbox::new("box".into())
        .code(SandboxCode::Image {
            image: "ubuntu".into(),
        })
        .egress(SandboxEgress::Internet)
        .lifecycle(SandboxLifecyclePolicy {
            max_lifetime_seconds: None,
            idle_pause_seconds: None,
        })
        .build();
    sandbox.privileged_supervisor = Some(SandboxPrivilegedSupervisor { command_uid: 60001 });
    for target in [
        "*.example.com",
        "8.8.8.0/24",
        "bücher.de",
        "api.example.com:8443",
    ] {
        sandbox.default_policy = Some(EgressPolicy::Allow {
            allow: vec![target.into()],
        });
        sandbox.validate_for_platform(Platform::Aws).unwrap();
        let env = sandbox.supervisor_environment();
        let policy: serde_json::Value = serde_json::from_str(&env["ALIEN_SANDBOX_EGRESS"]).unwrap();
        assert!(
            policy == "internet",
            "a runtime default must not become a permanent startup allowlist"
        );
    }
    sandbox.default_policy = None;
    sandbox.egress = SandboxEgress::None;
    let env = sandbox.supervisor_environment();
    // An explicitly configured legacy supervisor keeps the released UID filter.
    let policy: alien_sandbox_agent::egress::LegacyEgress =
        serde_json::from_str(&env["ALIEN_SANDBOX_EGRESS"]).unwrap();
    assert!(matches!(
        policy,
        alien_sandbox_agent::egress::LegacyEgress::Deny
    ));
}
