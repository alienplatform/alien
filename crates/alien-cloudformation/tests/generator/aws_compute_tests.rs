//! AWS compute & artifacts — function / build / artifact-registry /
//! compute-cluster.
//!
//! Mirrors the per-resource coverage in
//! `alien-terraform/tests/generator/aws_compute_tests.rs`. Each
//! scenario renders the full template via the built-in CFN registry,
//! runs `cfn-lint`, and snapshots the YAML for a complete audit
//! review.

use super::helpers::render_built_ins;
use alien_cloudformation::{generate_cloudformation_template, CfRegistry, RegistrationMode};
use alien_core::{
    ArtifactRegistry, Build, CapacityGroup, ComputeCluster, ErrorData, ManagementPermissions,
    Network, NetworkSettings, PermissionProfile, Platform, RemoteStackManagement,
    ResourceLifecycle, Stack, StackSettings, Worker, WorkerCode,
};

#[test]
fn aws_artifact_registry_renders_ecr_repository() {
    let stack = Stack::new("acme-ecr".to_string())
        .add(
            ArtifactRegistry::new("registry".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws_artifact_registry",
    );
    insta::assert_snapshot!("aws_artifact_registry", yaml);
}

#[test]
fn aws_build_renders_codebuild_project() {
    let stack = Stack::new("acme-build".to_string())
        .add(
            Build::new("builder".to_string())
                .permissions("execution".to_string())
                .environment([("PROFILE".to_string(), "release".to_string())].into())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws_build",
    );
    insta::assert_snapshot!("aws_build", yaml);
}

#[test]
fn aws_function_basic_lambda() {
    let stack = Stack::new("acme-fn".to_string())
        .add(
            Worker::new("api".to_string())
                .code(WorkerCode::Image {
                    image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app:1".to_string(),
                })
                .permissions("execution".to_string())
                .timeout_seconds(30)
                .expect("literal Worker timeout is within supported range")
                .memory_mb(256)
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws_function_basic",
    );
    insta::assert_snapshot!("aws_function_basic", yaml);
}

#[test]
fn aws_remote_stack_management_skips_live_provision_sets() {
    let stack = Stack::new("acme-mgmt".to_string())
        .management(ManagementPermissions::extend(
            PermissionProfile::new()
                .resource("job", ["worker/provision", "worker/dispatch-command"]),
        ))
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Worker::new("job".to_string())
                .code(WorkerCode::Image {
                    image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app/job:1.2.3".to_string(),
                })
                .permissions("execution".to_string())
                .build(),
            ResourceLifecycle::Live,
        )
        .build();

    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws_remote_stack_management_skips_live_provision_sets",
    );

    assert!(yaml.contains("lambda:InvokeFunction"));
    assert!(!yaml.contains("lambda:CreateFunction"));
}

#[test]
fn aws_function_public_ingress_emits_apigw_v2() {
    let stack = Stack::new("acme-public".to_string())
        .add(
            Worker::new("public-api".to_string())
                .code(WorkerCode::Image {
                    image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app:1".to_string(),
                })
                .permissions("execution".to_string())
                .public_endpoint(alien_core::WorkerPublicEndpoint {
                    name: "api".to_string(),
                    host_label: None,
                    wildcard_subdomains: false,
                })
                .timeout_seconds(60)
                .expect("literal Worker timeout is within supported range")
                .memory_mb(512)
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "aws_function_public",
    );
    insta::assert_snapshot!("aws_function_public", yaml);
}

#[test]
fn aws_container_cluster_without_platform_extension_errors_cleanly() {
    // OSS no longer registers `ComputeCluster` — Phase 6c moved the
    // emitter to `alien-cloudformationx`. Plugins (or the platform
    // setup package) wire it back in via `register_platform_extensions`.
    // Confirm the OSS error path is the typed `ImportRegistrationMissing`
    // a third-party plugin author would also see when forgetting to
    // register their emitter.
    let settings = StackSettings {
        network: Some(NetworkSettings::Create {
            cidr: Some("10.42.0.0/16".to_string()),
            availability_zones: 2,
        }),
        ..StackSettings::default()
    };
    let stack = Stack::new("acme-cluster".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(settings.network.clone().expect("network"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ComputeCluster::new("compute".to_string())
                .capacity_group(CapacityGroup {
                    group_id: "general".to_string(),
                    instance_type: Some("m7g.large".to_string()),
                    profile: None,
                    min_size: 1,
                    max_size: 3,
                    scale_policy: None,
                    nested_virtualization: None,
                })
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let registry = CfRegistry::built_in();
    let err = generate_cloudformation_template(
        &stack,
        alien_cloudformation::CloudFormationOptions {
            registry: &registry,
            target: alien_cloudformation::CloudFormationTarget::Aws,
            stack_settings: settings,
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: None,
        },
    )
    .expect_err("OSS registry should not register container_cluster");

    match err.error.as_ref().expect("typed error") {
        ErrorData::ImportRegistrationMissing {
            resource_type,
            platform,
            ..
        } => {
            assert_eq!(resource_type.as_ref(), "compute-cluster");
            assert_eq!(*platform, Platform::Aws);
        }
        other => panic!("expected ImportRegistrationMissing, got {other:?}"),
    }
}

#[test]
fn aws_custom_domain_registers_the_public_container_resource_id() {
    let container: alien_core::Container = serde_json::from_value(serde_json::json!({
        "id": "gateway", "links": [], "ports": [], "code": {"type": "image", "image": "example.com/app:1"},
        "cpu": {"min": "1", "desired": "1"}, "memory": {"min": "1Gi", "desired": "1Gi"},
        "permissions": "execution",
        "publicEndpoints": [{"name": "api", "port": 8080, "protocol": "http"}]
    }))
    .expect("public container");
    let stack = Stack::new("custom-domain".to_string())
        .add(container, ResourceLifecycle::Live)
        .build();
    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "container custom domain",
    );
    let template: serde_json::Value = serde_yaml::from_str(&yaml).expect("template");
    assert!(
        template["Parameters"].get("DomainResource").is_none(),
        "one endpoint needs no selector"
    );
    let domains: alien_cloudformation::CfExpression = serde_json::from_value(
        template["Outputs"]["DeploymentStackSettings"]["Value"]["Fn::ToJsonString"]["domains"]
            .clone(),
    )
    .expect("registration domains");
    let resolved = super::helpers::resolve(
        &domains,
        &std::collections::HashMap::from([("HasDomainName", true)]),
        super::helpers::Declined::Null,
    )
    .expect("enabled domain");
    assert_eq!(
        serde_json::to_value(resolved).unwrap(),
        serde_json::json!({
            "customDomains": {"gateway": {"domain": {"Ref": "DomainName"},
                "certificate": {"aws": {"certificateArn": {"Ref": "CertificateArn"}}}}}
        })
    );
}

#[test]
fn aws_custom_domain_selection_registers_exactly_one_public_resource() {
    let mut builder = Stack::new("two-public-resources".to_string());
    for id in ["api", "web"] {
        builder = builder.add(
            Worker::new(id.to_string())
                .code(WorkerCode::Image {
                    image: "example.com/app:1".to_string(),
                })
                .permissions("execution".to_string())
                .public_endpoint(alien_core::WorkerPublicEndpoint {
                    name: "public".to_string(),
                    host_label: None,
                    wildcard_subdomains: false,
                })
                .build(),
            ResourceLifecycle::Live,
        );
    }
    let stack = builder.build();
    let yaml = render_built_ins(
        &stack,
        StackSettings::default(),
        RegistrationMode::OutputsFallback,
        "custom domain resource selection",
    );
    let template: serde_json::Value = serde_yaml::from_str(&yaml).expect("template");
    assert_eq!(
        template["Parameters"]["DomainResource"]["AllowedValues"],
        serde_json::json!(["api", "web"])
    );
    let domains: alien_cloudformation::CfExpression = serde_json::from_value(
        template["Outputs"]["DeploymentStackSettings"]["Value"]["Fn::ToJsonString"]["domains"]
            .clone(),
    )
    .expect("registration domains");
    for (selected_api, resource_id) in [(true, "api"), (false, "web")] {
        let resolved = super::helpers::resolve(
            &domains,
            &std::collections::HashMap::from([
                ("HasDomainName", true),
                ("CustomDomainResource0", selected_api),
            ]),
            super::helpers::Declined::Null,
        )
        .expect("enabled domain");
        let mut value = serde_json::to_value(resolved).unwrap();
        value["customDomains"][resource_id]["domain"] = serde_json::json!("api.example.com");
        value["customDomains"][resource_id]["certificate"]["aws"]["certificateArn"] =
            serde_json::json!("arn:aws:acm:us-east-1:123456789012:certificate/test");
        let settings: alien_core::DomainSettings = serde_json::from_value(value)
            .expect("registered domain settings must deserialize without null entries");
        let domains = settings.custom_domains.expect("custom domains");
        assert_eq!(domains.len(), 1);
        assert!(domains.contains_key(resource_id));
    }
    let settings: StackSettings = serde_json::from_value(serde_json::json!({"domains": {"customDomains": {
        "api": {"domain": "api.example.com", "certificate": {"aws": {"certificateArn": "certificate-api"}}},
        "web": {"domain": "web.example.com", "certificate": {"aws": {"certificateArn": "certificate-web"}}}
    }}})).expect("multiple domain settings");
    let error = super::helpers::try_render_built_ins(
        &stack,
        settings,
        RegistrationMode::OutputsFallback,
        alien_cloudformation::CloudFormationTarget::Aws,
        "aws",
        "multiple configured domains",
    )
    .expect_err("a single DomainName parameter must not silently drop another configured domain");
    assert!(matches!(
        error.error,
        Some(ErrorData::OperationNotSupported { .. })
    ));
}
