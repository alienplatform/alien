//! A stack's extensions through the registries Alien builds for each deployment. The docs' DNS
//! zone renders through the CloudFormation, Terraform and Helm generators by running the acme
//! hooks module, its importer runs in the module, and the first step of the imported zone loads
//! what the importer returned.

use alien_aws_clients::AwsClientConfigExt;
use alien_cloudformation::{
    generate_cloudformation_template, CloudFormationOptions, CloudFormationTarget, RegistrationMode,
};
use alien_core::import::ImportContext;
use alien_core::{
    ClientConfig, DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, Platform,
    ResourceStatus, ResourceType, Stack, StackSettings, StackState,
};
use alien_extension_sdk::testing::{build_module, load_module, HookModule};
use alien_helm::{generate_helm_chart, HelmOptions};
use alien_infra::step_protocol::protocol::{StepRequest, StepResponse};
use alien_infra::step_protocol::test_controller::{ControllerHost, MockCloud, DNS_ZONE};
use alien_infra::step_protocol::{ProxyFactory, Transport, TransportFailure};
use alien_infra::{
    DeploymentRegistries, HostRegistrations, LoadedExtensions, MockPlatformServiceProvider,
    ResourceRegistry, StackExecutor,
};
use alien_terraform::{generate_terraform_module, TerraformOptions, TerraformTarget};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

fn acme_module() -> HookModule {
    static MODULE: OnceLock<HookModule> = OnceLock::new();
    MODULE
        .get_or_init(|| {
            let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
            let target_dir =
                PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("extension-sdk-guests");
            let wasm = build_module(&workspace, "acme-hooks", &target_dir)
                .unwrap_or_else(|error| panic!("{error}"));
            load_module(&wasm).unwrap_or_else(|error| panic!("{error}"))
        })
        .clone()
}

/// A stack with the docs' frozen DNS zone, its extension listing `setup` as the setup paths
/// it supports.
fn stack(setup: Value) -> Stack {
    serde_json::from_value(json!({
        "id": "my-app",
        "resources": {
            "zone": {
                "config": { "type": DNS_ZONE, "id": "zone", "domain": "internal.acme.dev" },
                "lifecycle": "frozen",
                "dependencies": []
            }
        },
        "extensions": [{
            "name": "acme",
            "version": "1.0.0",
            "digest": "sha256:0",
            "resourceTypes": {
                DNS_ZONE: {
                    "lifecycles": ["frozen", "live"],
                    "holdsData": false,
                    "setup": setup,
                    "controllerPlatforms": ["aws"],
                    "permissionSets": [],
                    "link": { "permissionSets": [] },
                    "schemas": { "config": {}, "outputs": {} }
                }
            }
        }]
    }))
    .unwrap()
}

/// Every path the acme module has a hook for.
fn full_setup() -> Value {
    json!({
        "cloudformation": ["aws"],
        "terraform": ["aws", "gcp", "azure"],
        "helm": ["kubernetes"],
        "importers": ["aws", "gcp", "azure"]
    })
}

fn registries(stack: &Stack, extensions: &LoadedExtensions) -> DeploymentRegistries {
    DeploymentRegistries::for_stack(stack, &HostRegistrations::NONE, extensions)
}

fn with_acme() -> LoadedExtensions {
    LoadedExtensions::default().with_hooks("acme", acme_module())
}

fn cloudformation(stack: &Stack, registries: &DeploymentRegistries) -> alien_core::Result<Value> {
    generate_cloudformation_template(
        stack,
        CloudFormationOptions {
            registry: &registries.cloudformation,
            source_stack: stack,
            target: CloudFormationTarget::Aws,
            stack_settings: StackSettings::default(),
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: None,
            management_trust: Default::default(),
        },
    )
    .map(|template| serde_json::to_value(template).unwrap())
}

fn terraform(
    stack: &Stack,
    registries: &DeploymentRegistries,
    target: TerraformTarget,
) -> alien_core::Result<String> {
    generate_terraform_module(
        stack,
        target,
        TerraformOptions {
            registry: &registries.terraform,
            display_name: None,
            stack_settings: StackSettings::default(),
            registration: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )
    .map(|module| {
        module
            .iter()
            .map(|(path, contents)| format!("=== {path} ===\n{contents}"))
            .collect()
    })
}

fn helm(stack: &Stack, registries: &DeploymentRegistries) -> alien_core::Result<String> {
    generate_helm_chart(
        stack,
        HelmOptions {
            registry: &registries.helm,
            stack_settings: StackSettings::default(),
            chart_name: stack.id().to_string(),
        },
    )
    // The resource's infrastructure entry is in the example values a deployer starts from.
    .map(|chart| chart.files["examples/onprem.yaml"].clone())
}

#[test]
fn a_custom_type_renders_through_every_generator_by_running_its_hooks_module() {
    let stack = stack(full_setup());
    let registries = registries(&stack, &with_acme());

    let template = cloudformation(&stack, &registries).unwrap();
    // The emitter read the domain from the resource entry Alien sent it.
    assert_eq!(
        template["Resources"]["Zone"],
        json!({ "Type": "AWS::Route53::HostedZone", "Properties": { "Name": "internal.acme.dev" } })
    );
    assert!(
        template.to_string().contains(r#"{"Ref":"Zone"}"#),
        "the import reference reaches the template: {template}"
    );

    let module = terraform(&stack, &registries, TerraformTarget::Aws).unwrap();
    assert!(
        module.contains(r#"resource "aws_route53_zone" "zone""#),
        "{module}"
    );
    assert!(module.contains(r#"name = "internal.acme.dev""#), "{module}");
    assert!(module.contains("aws_route53_zone.zone.zone_id"), "{module}");
    let module = terraform(&stack, &registries, TerraformTarget::Gcp).unwrap();
    assert!(module.contains("google_dns_managed_zone"), "{module}");

    let values = helm(&stack, &registries).unwrap();
    assert!(values.contains("zone:"), "{values}");
    assert!(values.contains("zoneId: 'your-zone-id'"), "{values}");
}

#[test]
fn a_setup_path_the_descriptor_lacks_is_off_with_the_reason() {
    let stack = stack(json!({
        "cloudformation": [],
        "terraform": ["aws"],
        "helm": [],
        "importers": ["aws"]
    }));
    let registries = registries(&stack, &with_acme());
    let refusals = [
        cloudformation(&stack, &registries).unwrap_err(),
        terraform(&stack, &registries, TerraformTarget::Gcp).unwrap_err(),
        helm(&stack, &registries).unwrap_err(),
    ];
    for (refusal, format) in refusals.iter().zip(["CloudFormation", "Terraform", "Helm"]) {
        assert_eq!(refusal.code, "OPERATION_NOT_SUPPORTED");
        assert!(
            refusal.message.contains(&format!(
                "extension 'acme', which has no {format} emitter for it"
            )),
            "{}",
            refusal.message
        );
    }
    terraform(&stack, &registries, TerraformTarget::Aws).expect("Terraform on AWS stays on");
}

/// Two stacks with different extensions, rendered at the same time in one process.
#[test]
fn deployments_with_different_extensions_never_see_each_others_registrations() {
    let acme_stack = stack(full_setup());
    let other_stack: Stack = serde_json::from_value(json!({
        "id": "other-app",
        "resources": {
            "thing": {
                "config": { "type": "other/thing", "id": "thing" },
                "lifecycle": "frozen",
                "dependencies": []
            }
        },
        "extensions": [{
            "name": "other",
            "version": "1.0.0",
            "digest": "sha256:1",
            "resourceTypes": {
                "other/thing": {
                    "lifecycles": ["frozen"],
                    "holdsData": false,
                    "setup": { "cloudformation": ["aws"], "terraform": [], "helm": [], "importers": [] },
                    "controllerPlatforms": [],
                    "permissionSets": [],
                    "link": { "permissionSets": [] },
                    "schemas": { "config": {}, "outputs": {} }
                }
            }
        }]
    }))
    .unwrap();
    let acme = ResourceType::from(DNS_ZONE);
    let other = ResourceType::from("other/thing");

    std::thread::scope(|scope| {
        let mut threads = Vec::new();
        for _ in 0..4 {
            threads.push(scope.spawn(|| {
                let registries = registries(&acme_stack, &with_acme());
                assert!(registries
                    .cloudformation
                    .emitter(&other, Platform::Aws)
                    .is_none());
                let template = cloudformation(&acme_stack, &registries).unwrap();
                assert!(template["Resources"].get("Zone").is_some());
            }));
            threads.push(scope.spawn(|| {
                // This process loaded the acme module only, and this stack doesn't list acme.
                let registries = registries(&other_stack, &with_acme());
                assert!(registries
                    .cloudformation
                    .emitter(&acme, Platform::Aws)
                    .is_none());
                assert!(registries
                    .importers
                    .importer(&acme, Platform::Aws)
                    .is_none());
                let refusal = cloudformation(&other_stack, &registries).unwrap_err();
                assert!(
                    format!("{refusal:?}").contains("has not loaded the extension's hooks module"),
                    "{refusal:?}"
                );
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }
    });
}

/// Hands each step to a controller in this process, through the same JSON as the wire.
struct InProcess {
    host: ControllerHost,
    requests: Mutex<Vec<Value>>,
}

impl std::fmt::Debug for InProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("InProcess")
    }
}

#[async_trait]
impl Transport for InProcess {
    async fn step(
        &self,
        request: &StepRequest,
        _timeout: Duration,
    ) -> Result<StepResponse, TransportFailure> {
        let wire = serde_json::to_value(request).unwrap();
        self.requests.lock().unwrap().push(wire.clone());
        let response = self
            .host
            .handle(serde_json::from_value(wire).unwrap())
            .await;
        Ok(serde_json::from_value(serde_json::to_value(response).unwrap()).unwrap())
    }
}

#[tokio::test]
async fn the_importer_runs_in_the_module_and_the_first_step_loads_what_it_imported() {
    let stack = stack(full_setup());
    let settings = StackSettings::default();
    let registries = registries(&stack, &with_acme());
    let imported = registries
        .importers
        .run(
            &ResourceType::from(DNS_ZONE),
            Platform::Aws,
            json!({ "zoneId": "ZSETUP" }),
            &ImportContext {
                resource_id: "zone",
                platform: Platform::Aws,
                region: "us-east-1",
                stack_settings: &settings,
                management_config: None,
                resource: &stack.resources["zone"],
            },
        )
        .unwrap();
    let fields = json!({ "zoneId": "ZSETUP", "state": "ready" });
    let saved = imported.internal_state.clone().unwrap();
    assert_eq!(saved["type"], "ExtensionProxyController");
    assert_eq!(saved["pendingImport"], fields);
    assert_eq!(
        serde_json::to_value(imported.outputs.as_ref().unwrap()).unwrap(),
        json!({ "type": DNS_ZONE, "zoneId": "ZSETUP" })
    );

    let cloud = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("registries-import-cloud")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&cloud).unwrap();
    MockCloud::at(&cloud).seed_zone("setup-template", "ZSETUP", "internal.acme.dev");
    let transport = Arc::new(InProcess {
        host: ControllerHost::new(&cloud, None),
        requests: Mutex::default(),
    });
    let extensions = with_acme().with_controller(
        "acme",
        ProxyFactory::new("dep_import", transport.clone(), None),
    );
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(ExternalBindings::default())
        .allow_frozen_changes(false)
        .build();
    let executor = StackExecutor::builder(
        &stack,
        ClientConfig::Aws(Box::new(alien_aws_clients::AwsClientConfig::mock())),
    )
    .deployment_config(&config)
    .resource_registry(Arc::new(ResourceRegistry::with_built_ins_and_extensions(
        &stack,
        &extensions,
    )))
    .service_provider({
        let extensions = Arc::new(extensions.clone());
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_extensions()
            .returning(move || Some(extensions.clone()));
        Arc::new(provider)
    })
    .build()
    .unwrap();

    let mut state = StackState::new(Platform::Aws);
    let mut imported = imported;
    imported.controller_platform = Some(Platform::Aws);
    state.resources.insert("zone".to_string(), imported);
    let stepped = executor.step(state).await.unwrap().next_state;

    let zone = &stepped.resources["zone"];
    assert_eq!(zone.status, ResourceStatus::Running, "{:?}", zone.error);
    let requests = transport.requests.lock().unwrap();
    assert!(requests[0]["state"].is_null());
    assert_eq!(requests[0]["pendingImport"], fields);
    let calls = MockCloud::at(&cloud).state().calls;
    assert!(
        !calls.iter().any(|call| call.starts_with("create ")),
        "{calls:?}"
    );
    assert!(calls.iter().any(|call| call == "get ZSETUP"), "{calls:?}");
}
