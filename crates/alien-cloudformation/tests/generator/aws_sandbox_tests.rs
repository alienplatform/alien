//! AWS Sandbox — what `egress: deny` has to build for the template to mean it.

use super::helpers::{
    custom_resource_registration, render_built_ins_template, try_render_built_ins,
};
use alien_cloudformation::CloudFormationTarget;
use alien_core::{
    Network, NetworkSettings, ResourceLifecycle, Sandbox, SandboxCode, SandboxEgress,
    SandboxSessionPolicy, Stack, StackSettings,
};

fn sandbox_fixture(egress: SandboxEgress) -> Sandbox {
    Sandbox::new("agents".to_string())
        .code(SandboxCode::Image {
            image: "s3://acme-artifacts/agents/bundle.zip".to_string(),
        })
        .egress(egress)
        .session(SandboxSessionPolicy {
            max_lifetime_seconds: None,
            idle_suspend_seconds: None,
        })
        .build()
}

/// A sandbox and the network its egress connector attaches to, which the emitter requires.
fn sandbox_stack(name: &str, egress: SandboxEgress) -> (Stack, StackSettings) {
    let settings = StackSettings {
        network: Some(NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        }),
        ..StackSettings::default()
    };
    let stack = Stack::new(name.to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(settings.network.clone().expect("network"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(sandbox_fixture(egress), ResourceLifecycle::Frozen)
        .build();
    (stack, settings)
}

/// `egress: deny` has to be built, not assumed.
///
/// A MicroVM started with no egress connector reaches the public internet — verified against a
/// live account. The connector is what puts session traffic inside the VPC, and the security
/// group is what stops it there: EC2 adds an allow-all egress rule to any group whose template
/// states none, so the only rule present must be the one that reaches nothing.
#[test]
fn aws_sandbox_deny_builds_a_connector_that_permits_nothing_outbound() {
    let (stack, settings) = sandbox_stack("acme-sandbox-deny", SandboxEgress::Deny);
    let (template, _yaml) = render_built_ins_template(
        &stack,
        settings,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "sandbox deny",
    );

    let security_group = template
        .resources
        .get("AgentsEgressSecurityGroup")
        .expect("the sandbox egress security group must render");
    let egress = serde_json::to_string(
        security_group
            .properties
            .get("SecurityGroupEgress")
            .expect("an egress rule, or EC2's allow-all default survives"),
    )
    .expect("serializes");
    assert!(
        egress.contains("127.0.0.1/32"),
        "the only permitted destination must be the one that reaches nothing: {egress}"
    );
    assert!(
        !egress.contains("0.0.0.0/0"),
        "a wide egress rule turns deny back into outbound access: {egress}"
    );

    let connector = template
        .resources
        .get("AgentsEgressConnector")
        .expect("the egress connector must render");
    assert_eq!(connector.resource_type, "AWS::Lambda::NetworkConnector");
    let configuration = serde_json::to_string(
        connector
            .properties
            .get("Configuration")
            .expect("connector configuration"),
    )
    .expect("serializes");
    assert!(
        configuration.contains("AgentsEgressSecurityGroup"),
        "the connector must carry the group that denies: {configuration}"
    );
    assert!(
        configuration.contains("DefaultNetworkPrivateSubnet1"),
        "the connector must place its interfaces in the network's private subnets: \
         {configuration}"
    );
    assert!(
        configuration.contains("MicroVm"),
        "the connector must be usable by MicroVMs: {configuration}"
    );

    let image = template
        .resources
        .get("Agents")
        .expect("the MicroVM image must render");
    let connectors = serde_json::to_string(
        image
            .properties
            .get("EgressNetworkConnectors")
            .expect("the image's connector list"),
    )
    .expect("serializes");
    // The switch that keeps session output out of the control plane's reach. It has no test of
    // its own anywhere else, so a one-sided edit re-enabling it would ship green.
    let logging = serde_json::to_string(
        image
            .properties
            .get("Logging")
            .expect("the image must state its logging"),
    )
    .expect("serializes");
    assert!(
        logging.contains("\"Disabled\":true"),
        "content-bearing logging must be off: {logging}"
    );

    // The build's route, not the session's. Naming the deny connector here would leave the image
    // build with nowhere to reach a registry, and it would never become ACTIVE.
    assert!(
        connectors.contains("INTERNET_EGRESS"),
        "the image must build through AWS's own connector: {connectors}"
    );
    assert!(
        !connectors.contains("AgentsEgressConnector"),
        "the deny connector belongs to the session, not the build: {connectors}"
    );

    // And the session's own connector still reaches the binding, which is what actually bounds
    // a running sandbox.
    let registration = serde_json::to_string(&template.resources).expect("serializes");
    assert!(
        registration.contains("AgentsEgressConnector"),
        "the deny connector must still be carried to the session"
    );
}

/// Both `GetMicrovmImage` and `RunMicrovm` require the image **ARN** — measured against the live
/// API, where a bare name is refused by both and `RunMicrovm` says "Malformed ARN - doesn't start
/// with 'arn:'". `Ref` on an `AWS::Lambda::MicrovmImage` returns that ARN. The two package formats
/// must also agree, or a controller handed one form by one of them cannot read its own image.
#[test]
fn the_image_identifier_is_the_arn_both_calls_require() {
    let (stack, settings) = sandbox_stack("acme-sandbox-id", SandboxEgress::Deny);
    let (template, _yaml) = render_built_ins_template(
        &stack,
        settings,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "sandbox identifier",
    );

    let rendered = serde_json::to_string(&template.resources).expect("serializes");
    let import = rendered
        .split("imageIdentifier")
        .nth(1)
        .unwrap_or_else(|| panic!("the import data must carry an imageIdentifier:\n{rendered}"));
    let import: String = import.chars().take(200).collect();
    assert!(
        import.contains("Ref"),
        "the identifier must be the ARN Ref returns, not the name: {import}"
    );
}

/// Without a VPC there are no subnets, and a connector needs between one and sixteen.
///
/// Rendering one anyway would produce either a deploy-time failure the reader cannot act on or —
/// worse — a session with no connector, which is the case that reaches the internet.
#[test]
fn aws_sandbox_refuses_to_render_without_a_network_to_attach_to() {
    let stack = Stack::new("acme-sandbox-no-network".to_string())
        .add(
            sandbox_fixture(SandboxEgress::Deny),
            ResourceLifecycle::Frozen,
        )
        .build();

    let error = try_render_built_ins(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "sandbox without a network",
    )
    .expect_err("a sandbox with no network must be refused at emit time");
    assert!(
        error.message.contains("declares no network"),
        "the refusal must name why: {}",
        error.message
    );
}

/// An egress mode the artifact cannot deliver is refused, not dropped.
///
/// The connector this template builds denies outbound traffic. Nothing renders an allowance —
/// `allow` depends on the network's NAT topology and AWS has no domain filter at the connector —
/// so a declared `allow` would be silently ignored while the customer believed otherwise.
#[test]
fn aws_sandbox_refuses_an_egress_mode_it_cannot_deliver() {
    for mode in [
        SandboxEgress::Allow,
        SandboxEgress::AllowDomains {
            domains: vec!["example.com".to_string()],
        },
    ] {
        let (stack, settings) = sandbox_stack("acme-sandbox-egress", mode.clone());
        let error = try_render_built_ins(
            &stack,
            settings,
            custom_resource_registration(),
            CloudFormationTarget::Aws,
            "aws",
            "sandbox egress",
        )
        .expect_err(&format!("egress {mode:?} must be refused at emit time"));
        assert!(
            error.message.contains("VPC egress connector"),
            "the refusal must name why: {}",
            error.message
        );
    }
}
