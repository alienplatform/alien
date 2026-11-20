//! Validate complete setup artifacts with every built-in emitter.
use alien_cloudformation::{
    generate_cloudformation_template, to_yaml, CfRegistry, CloudFormationOptions,
    CloudFormationTarget, RegistrationMode,
};
use alien_core::{
    ArtifactRegistry, Build, ComputeCluster, Container, ContainerCode, DeploymentConfig,
    EnvironmentVariablesSnapshot, ExternalBindings, Key, Kv, ManagementPermissions, Network,
    NetworkSettings, PermissionProfile, PersistentStorage, Platform, Queue, RemoteStackManagement,
    ResourceLifecycle, ResourceRef, ResourceSpec, ServiceAccount, SetupManagement, Stack,
    StackSettings, StackState, Storage, Vault, VolumeBackups, Worker, WorkerCode,
};
use alien_permissions::{generators::aws_stack_permissions_boundary, PermissionContext};
use alien_preflights::{mutations::ManagementPermissionProfileMutation, StackMutation};
use alien_terraform::{generate_terraform_module, TerraformOptions, TerraformTarget, TfRegistry};
use indexmap::IndexMap;
use std::{fs, path::PathBuf};

fn fixture(include_keys: bool) -> (Stack, StackSettings) {
    let network = NetworkSettings::Create {
        cidr: Some("10.44.0.0/16".into()),
        availability_zones: 2,
    };
    let mut profile = PermissionProfile::new().global([
        "storage/data-read",
        "queue/data-write",
        "kv/data-read",
        "vault/data-read",
    ]);
    if include_keys {
        profile = profile
            .resource("key-0", ["key/remote-cryptography"])
            .resource("key-1", ["key/remote-cryptography"]);
    }
    let mut stack = Stack::new("s".repeat(40))
        .permission("execution", profile)
        .management(ManagementPermissions::extend(
            PermissionProfile::new().global([
                "service-account/provision",
                "worker/provision",
                "container/provision",
                "compute-cluster/management",
                "storage/heartbeat",
                "queue/heartbeat",
                "kv/heartbeat",
            ]),
        ))
        .add(
            Network::new("default-network".into())
                .settings(network.clone())
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ServiceAccount::new("execution-sa".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ComputeCluster::new("compute".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Storage::new("assets".into()).versioning(true).build(),
            ResourceLifecycle::Frozen,
        )
        .add(Queue::new("jobs".into()).build(), ResourceLifecycle::Frozen)
        .add(
            Kv::new("metadata".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Vault::new("secrets".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            ArtifactRegistry::new("registry".into()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Build::new("builder".into())
                .permissions("execution".into())
                .build(),
            ResourceLifecycle::Live,
        );
    for index in 0..2 {
        let key = format!("key-{index}");
        if include_keys {
            stack = stack.add(Key::new(key.clone()).build(), ResourceLifecycle::Frozen);
        }
        for replica in 0..3 {
            let mut container = Container::new(format!("db-{index}-{replica}"))
                .cluster("compute".into())
                .permissions("execution".into())
                .code(ContainerCode::Image {
                    image: "postgres:17".into(),
                })
                .cpu(ResourceSpec {
                    min: "1".into(),
                    desired: "1".into(),
                })
                .memory(ResourceSpec {
                    min: "1Gi".into(),
                    desired: "1Gi".into(),
                })
                .build();
            container.stateful = true;
            container.persistent_storage = Some(PersistentStorage {
                size: "10Gi".into(),
                mount_path: "/data".into(),
                encryption_key: include_keys
                    .then(|| ResourceRef::new(Key::RESOURCE_TYPE, key.clone())),
                backups: VolumeBackups::default(),
            });
            stack = stack.add(container, ResourceLifecycle::Live).add(
                Worker::new(format!("worker-{index}-{replica}"))
                    .permissions("execution".into())
                    .code(WorkerCode::Image {
                        image: "123456789012.dkr.ecr.us-east-1.amazonaws.com/app/worker:1".into(),
                    })
                    .build(),
                ResourceLifecycle::Live,
            );
        }
    }
    (
        stack.build(),
        StackSettings {
            network: Some(network),
            ..StackSettings::default()
        },
    )
}

#[test]
fn realistic_bounded_setup_artifacts_validate() {
    let (stack, settings) = fixture(true);
    validate_package(stack, settings, "explicit");
}

#[tokio::test]
async fn derived_manual_setup_artifacts_validate() {
    validate_derived_package(SetupManagement::Manual, "manual").await;
}

#[tokio::test]
async fn derived_automatic_setup_artifacts_validate() {
    validate_derived_package(SetupManagement::Automatic, "automatic").await;
}

async fn validate_derived_package(mode: SetupManagement, name: &str) {
    let (mut stack, mut settings) = fixture(mode == SetupManagement::Manual);
    stack.permissions.management = ManagementPermissions::Auto;
    settings.setup_management = mode;
    let config = DeploymentConfig::builder()
        .stack_settings(settings.clone())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2026-01-01T00:00:00Z".into(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    let stack = ManagementPermissionProfileMutation
        .mutate(stack, &StackState::new(Platform::Aws), &config)
        .await
        .expect("management profile should derive for the complete fixture");
    validate_package(stack, settings, name);
}

fn validate_package(stack: Stack, settings: StackSettings, mode: &str) {
    let context = PermissionContext::new()
        .with_stack_prefix("s".repeat(40))
        .with_aws_account_id("123456789012")
        .with_aws_region("ap-southeast-7")
        .with_managing_account_id("210987654321")
        .with_managing_role_arn("arn:aws:iam::210987654321:role/manager");
    let boundary = aws_stack_permissions_boundary(&stack, &context, |_, scope| {
        let suffix = match scope {
            "key-0" => "00000000-0000-0000-0000-000000000001",
            "key-1" => "00000000-0000-0000-0000-000000000002",
            _ => panic!("unexpected exact-resource scope: {scope}"),
        };
        Ok(context
            .clone()
            .with_resource_id(scope)
            .with_resource_name(format!(
                "arn:aws:kms:ap-southeast-7:123456789012:key/{suffix}"
            )))
    })
    .expect("the resolved stack boundary should fit the managed-policy limit");
    let boundary_json = serde_json::to_string(&boundary).expect("boundary JSON");
    assert!(boundary_json.chars().filter(|c| !c.is_whitespace()).count() <= 6144);
    let cf_registry = CfRegistry::built_in();
    let template = generate_cloudformation_template(
        &stack,
        CloudFormationOptions {
            registry: &cf_registry,
            source_stack: &stack,
            target: CloudFormationTarget::Aws,
            stack_settings: settings.clone(),
            setup_target: "aws".into(),
            setup_fingerprint: "validation".into(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: Some("Permissions boundary package validation".into()),
            management_trust: Default::default(),
            pins: Default::default(),
        },
    )
    .expect("complete CloudFormation setup should generate");
    let yaml = to_yaml(&template).expect("template should serialize");
    let tf_registry = TfRegistry::built_in();
    let module = generate_terraform_module(
        &stack,
        TerraformTarget::Aws,
        TerraformOptions {
            display_name: None,
            registry: &tf_registry,
            stack_settings: settings.clone(),
            registration: None,
            setup_identity: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
            pins: Default::default(),
        },
    )
    .expect("complete Terraform setup should generate");
    if let Some(directory) = std::env::var_os("BOUNDARY_PACKAGE_OUTPUT") {
        let directory = PathBuf::from(directory).join(mode);
        fs::create_dir_all(directory.join("terraform")).expect("output directory");
        fs::write(directory.join("template.yaml"), &yaml).expect("write template");
        fs::write(directory.join("boundary.json"), &boundary_json).expect("write boundary");
        fs::write(
            directory.join("stack.json"),
            serde_json::to_vec_pretty(&stack).unwrap(),
        )
        .expect("write stack");
        fs::write(
            directory.join("settings.json"),
            serde_json::to_vec_pretty(&settings).expect("settings JSON"),
        )
        .expect("write settings");
        for (path, contents) in module.iter() {
            fs::write(directory.join("terraform").join(path), contents).expect("write module");
        }
    }
    eprintln!(
        "{mode}: resolved boundary: {} characters",
        boundary_json.len()
    );
    eprintln!(
        "{} stack resources; {} CloudFormation resources; {} template bytes; {} module files",
        stack.resources.len(),
        template.resources.len(),
        yaml.len(),
        module.files.len()
    );
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("complete bounded setup");
    let files: IndexMap<String, String> = module
        .files
        .into_iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .collect();
    alien_terraform::test_utils::terraform_fmt_check(&files).assert_ok("complete bounded setup");
    alien_terraform::test_utils::terraform_validate(&files).assert_ok("complete bounded setup");
}
