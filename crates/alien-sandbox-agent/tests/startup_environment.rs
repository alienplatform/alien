use alien_core::{
    EgressPolicy, Platform, Sandbox, SandboxCode, SandboxEgress, SandboxLifecyclePolicy,
    SandboxPrivilegedSupervisor,
};

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
