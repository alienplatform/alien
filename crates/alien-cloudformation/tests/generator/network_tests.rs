//! Network axis — `Create` (full topology) vs `ByoVpcAws` (parameter-driven).

use super::helpers::{render_built_ins, render_sample, sample_stack};
use alien_cloudformation::RegistrationMode;
use alien_core::{
    HeartbeatsMode, Network, NetworkSettings, ResourceLifecycle, Stack, StackSettings,
    TelemetryMode, UpdatesMode,
};

#[test]
fn create_network_three_az_emits_full_topology() {
    let settings = StackSettings {
        network: Some(NetworkSettings::Create {
            cidr: Some("10.42.0.0/16".to_string()),
            availability_zones: 3,
        }),
        updates: UpdatesMode::ApprovalRequired,
        telemetry: TelemetryMode::Off,
        heartbeats: HeartbeatsMode::Off,
        ..StackSettings::default()
    };

    let stack = Stack::new("network-create".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(settings.network.clone().expect("network settings"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let yaml = render_built_ins(
        &stack,
        settings,
        RegistrationMode::OutputsFallback,
        "create network 3 az",
    );
    insta::assert_snapshot!("network_create_three_az", yaml);
}

#[test]
fn byo_vpc_aws_uses_parameter_driven_subnet_ids() {
    let settings = StackSettings {
        network: Some(NetworkSettings::ByoVpcAws {
            vpc_id: "vpc-0123456789abcdef0".to_string(),
            public_subnet_ids: vec!["subnet-public-a".to_string(), "subnet-public-b".to_string()],
            private_subnet_ids: vec![
                "subnet-private-a".to_string(),
                "subnet-private-b".to_string(),
            ],
            security_group_ids: vec!["sg-0123456789abcdef0".to_string()],
        }),
        ..StackSettings::default()
    };

    let yaml = render_sample(
        &sample_stack(),
        settings,
        RegistrationMode::OutputsFallback,
        "byo vpc aws",
    );
    insta::assert_snapshot!("network_byo_vpc_aws", yaml);
}
