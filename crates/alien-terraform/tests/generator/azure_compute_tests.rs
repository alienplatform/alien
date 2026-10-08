//! Azure compute & artifacts — function / build / artifact-registry.
//!
//! Mirror of `gcp_compute_tests.rs` for Azure. Each scenario produces
//! one multi-file snapshot. `terraform fmt -check` + `terraform validate`
//! run on every render against the real `hashicorp/azurerm` provider.

use super::helpers::{assert_terraform_valid, render, snapshot_module};
use alien_core::{
    ArtifactRegistry, AzureContainerAppsEnvironment, AzureResourceGroup, Build,
    ComputePoolSelection, ComputeSettings, ResourceLifecycle, Stack, StackSettings, Worker,
    WorkerCode,
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
            ResourceLifecycle::Frozen,
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
                .expect("literal Worker timeout is within supported range")
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
fn azure_container_apps_environment_names_stay_within_azure_limits() {
    let stack = Stack::new("acme-fn".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .add(container_apps_environment(), ResourceLifecycle::Frozen)
        .build();
    let module = render(&stack, TerraformTarget::Azure, StackSettings::default());
    let mut files: super::helpers::test_utils::LinterFiles = module
        .iter()
        .map(|(path, content)| (path.to_string(), content.to_string()))
        .collect();
    files.insert(
        "tests/names.tftest.hcl".to_string(),
        r#"
mock_provider "azurerm" {}
variables {
  name = "test-environment"
  token = "test-token"
  azure_subscription_id = "00000000-0000-0000-0000-000000000000"
  azure_resource_group_name = "example-rg"
}
run "long_prefix" {
  command = plan
  variables {
    resource_prefix = "e2e-10-azure-terrafor-w-c2e6a7-98357704a"
  }
  assert {
    condition = length(var.resource_prefix) == 40
    error_message = "This run must use the longest prefix the module accepts"
  }
  assert {
    condition = length(azurerm_log_analytics_workspace.default_container_apps_environment_logs.name) <= 63 && can(regex("^[a-z0-9][a-z0-9-]+[a-z0-9]$", azurerm_log_analytics_workspace.default_container_apps_environment_logs.name))
    error_message = "Log Analytics workspace name must be 4-63 characters and end with a letter or digit: ${azurerm_log_analytics_workspace.default_container_apps_environment_logs.name}"
  }
  assert {
    condition = startswith(azurerm_log_analytics_workspace.default_container_apps_environment_logs.name, "e2e-10-azure-terrafor-w-c2e6a7-98357704a-")
    error_message = "Log Analytics workspace name must keep the deployment prefix: ${azurerm_log_analytics_workspace.default_container_apps_environment_logs.name}"
  }
  assert {
    condition = length(azurerm_container_app_environment.default_container_apps_environment.name) <= 60 && can(regex("^[a-z0-9][a-z0-9-]+[a-z0-9]$", azurerm_container_app_environment.default_container_apps_environment.name))
    error_message = "Managed environment name must be 2-60 lowercase characters and end with a letter or digit: ${azurerm_container_app_environment.default_container_apps_environment.name}"
  }
}
run "short_prefix" {
  command = plan
  variables {
    resource_prefix = "acme"
  }
  assert {
    condition = azurerm_log_analytics_workspace.default_container_apps_environment_logs.name == "acme-default-container-apps-environment-logs"
    error_message = "A short prefix must keep the readable workspace name"
  }
  assert {
    condition = azurerm_container_app_environment.default_container_apps_environment.name == "acme-default-container-apps-environment"
    error_message = "A short prefix must keep the readable environment name"
  }
}
"#
        .to_string(),
    );
    super::helpers::test_utils::terraform_test(&files)
        .assert_ok("Azure Container Apps environment names");
}

#[test]
fn advanced_settings_overlay_preserves_generated_compute_defaults() {
    let stack = Stack::new("acme-overlay".to_string())
        .add(resource_group(), ResourceLifecycle::Frozen)
        .build();
    let module = render(
        &stack,
        TerraformTarget::Azure,
        StackSettings {
            compute: Some(ComputeSettings {
                pools: [(
                    "general".to_string(),
                    ComputePoolSelection::Fixed {
                        machines: 1,
                        machine: Some("Standard_D2as_v5".to_string()),
                        failure_domains: None,
                    },
                )]
                .into_iter()
                .collect(),
            }),
            ..StackSettings::default()
        },
    );

    let variables = module.get("variables.tf").expect("variables should render");
    assert!(variables.contains(r#"variable "advanced_settings_overlay_json""#));
    assert!(variables.contains(r#"\"machine\":\"Standard_D2as_v5\""#));
    let locals = module.get("locals.tf").expect("locals should render");
    assert!(locals.contains("advanced_settings"));
    assert!(locals.contains("jsondecode(var.advanced_settings_json)"));
    assert!(locals.contains("jsondecode(var.advanced_settings_overlay_json)"));
    assert!(locals.contains("deployment_settings"));
    assert!(locals.contains("merge(local.advanced_settings"));
    assert_terraform_valid(&module, "azure_advanced_settings_overlay");
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
    assert!(rendered.contains("environmentName"));
    assert!(rendered.contains("\"shared-env\""));
    assert!(rendered.contains("resourceGroup"));
    assert!(rendered.contains("\"shared-rg\""));
    assert!(rendered.contains("resourceGroupName"));
    assert!(rendered.contains("defaultDomain"));
    assert!(rendered.contains("\"shared.example.azurecontainerapps.io\""));
    assert_terraform_valid(&module, "azure_function_shared_env");
}
