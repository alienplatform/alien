//! Azure compute & artifacts — function / build / artifact-registry.
//!
//! Mirror of `gcp_compute_tests.rs` for Azure. Each scenario produces
//! one multi-file snapshot. `terraform fmt -check` + `terraform validate`
//! run on every render against the real `hashicorp/azurerm` provider.

use super::helpers::{assert_terraform_valid, render, snapshot_module};
use alien_core::{
    ArtifactRegistry, AzureContainerAppsEnvironment, AzureResourceGroup, Build, Ingress,
    ResourceLifecycle, Stack, StackSettings, Worker, WorkerCode,
};
use alien_core::{ContainerAppsEnvironmentBinding, ExternalBinding, ExternalBindings};
use alien_terraform::TerraformTarget;

fn resource_group() -> AzureResourceGroup {
    AzureResourceGroup::new("default-resource-group".to_string()).build()
}

fn container_apps_environment() -> AzureContainerAppsEnvironment {
    AzureContainerAppsEnvironment::new("default-container-apps-environment".to_string()).build()
}

#[test]
fn azure_artifact_registry_renders_premium_acr_with_pull_push_uami() {
    let stack = Stack::new("acme-ar".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(
            ArtifactRegistry::new("registry".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    snapshot_module("azure_artifact_registry", &module);
    assert_terraform_valid(&module, "azure_artifact_registry");
}

#[test]
fn azure_build_renders_acr_task() {
    let stack = Stack::new("acme-build".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(
            ArtifactRegistry::new("registry".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Build::new("builder".to_string())
                .permissions("execution".to_string())
                .environment([("PROFILE".to_string(), "release".to_string())].into())
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    snapshot_module("azure_build", &module);
    assert_terraform_valid(&module, "azure_build");
}

#[test]
fn azure_function_basic_container_app() {
    let stack = Stack::new("acme-fn".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(container_apps_environment(), ResourceLifecycle::Frozen)
        .add(
            Worker::new("api".to_string())
                .code(WorkerCode::Image {
                    image: "acmeprod.azurecr.io/api:1".to_string(),
                })
                .permissions("execution".to_string())
                .timeout_seconds(30)
                .memory_mb(256)
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    snapshot_module("azure_function_basic", &module);
    assert_terraform_valid(&module, "azure_function_basic");
}

#[test]
fn azure_function_public_ingress_enables_external_ingress() {
    let stack = Stack::new("acme-public".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(container_apps_environment(), ResourceLifecycle::Frozen)
        .add(
            Worker::new("public-api".to_string())
                .code(WorkerCode::Image {
                    image: "acmeprod.azurecr.io/api:1".to_string(),
                })
                .permissions("execution".to_string())
                .ingress(Ingress::Public)
                .timeout_seconds(60)
                .memory_mb(512)
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    snapshot_module("azure_function_public", &module);
    assert_terraform_valid(&module, "azure_function_public");
}

#[test]
fn azure_function_reuses_external_container_apps_environment() {
    let stack = Stack::new("acme-fn-shared-env".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(container_apps_environment(), ResourceLifecycle::Frozen)
        .add(
            Worker::new("api".to_string())
                .code(WorkerCode::Image {
                    image: "acmeprod.azurecr.io/api:1".to_string(),
                })
                .permissions("execution".to_string())
                .build(),
            ResourceLifecycle::Live,
        )
        .build();
    let mut external_bindings = ExternalBindings::new();
    external_bindings.insert(
        "default-container-apps-environment",
        ExternalBinding::ContainerAppsEnvironment(ContainerAppsEnvironmentBinding::new(
            "shared-env",
            "/subscriptions/sub-123/resourceGroups/shared-rg/providers/Microsoft.App/managedEnvironments/shared-env",
            "shared-rg",
            "shared.example.azurecontainerapps.io",
        )),
    );
    let module = render(
        &stack,
        TerraformTarget::Azure,
        StackSettings {
            external_bindings: Some(external_bindings),
            ..StackSettings::default()
        },
    );
    let rendered = module
        .iter()
        .map(|(_, contents)| contents)
        .collect::<String>();

    assert!(!rendered.contains("azurerm_container_app_environment"));
    assert!(!rendered.contains("azurerm_log_analytics_workspace"));
    assert!(rendered.contains(
        "/subscriptions/sub-123/resourceGroups/shared-rg/providers/Microsoft.App/managedEnvironments/shared-env"
    ));
    assert!(rendered.contains("environmentName = \"shared-env\""));
    assert!(rendered.contains("resourceGroup   = \"shared-rg\""));
    assert!(rendered.contains("resourceGroupName"));
    assert!(rendered.contains("defaultDomain   = \"shared.example.azurecontainerapps.io\""));
    assert_terraform_valid(&module, "azure_function_shared_env");
}
