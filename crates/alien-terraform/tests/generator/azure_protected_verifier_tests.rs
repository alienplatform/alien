//! Where a protected Azure module's verifier creates the install's workers. The install's
//! resource group holds two Container Apps environments, the stack's and the verifier's own, so
//! setup names the stack's to the verifier; checked by evaluating the module with Terraform.

use alien_core::{
    AzureContainerAppsEnvironment, AzureResourceGroup, ContainerAppsEnvironmentBinding,
    ExternalBinding, ExternalBindings, RemoteStackManagement, ResourceLifecycle, SigningVerifier,
    Stack, StackSettings, StackSigning, Worker, WorkerCode,
};
use alien_terraform::TerraformTarget;

use super::helpers::{linter_files, render, test_utils, try_render};

const SHARED_ENVIRONMENT: &str =
    "/subscriptions/sub-123/resourceGroups/shared-rg/providers/Microsoft.App/managedEnvironments/shared-env";

fn stack(environments: &[&str]) -> Stack {
    let permissions: Stack = serde_json::from_value(serde_json::json!({
        "id": "app",
        "resources": {},
        "permissions": { "profiles": { "execution": {} }, "management": "auto" }
    }))
    .expect("stack");
    let mut stack = Stack::new("app".to_string())
        .permissions(permissions.permissions().clone())
        .add(
            AzureResourceGroup::new("default-resource-group".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        );
    for environment in environments {
        stack = stack.add(
            AzureContainerAppsEnvironment::new(environment.to_string()).build(),
            ResourceLifecycle::Frozen,
        );
    }
    // What the preflight adds to a protected stack with a function.
    stack
        .add(
            SigningVerifier::new(SigningVerifier::ID.to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Worker::new("api".to_string())
                .code(WorkerCode::Image {
                    image: format!("acmeprod.azurecr.io/api@sha256:{}", "a".repeat(64)),
                })
                .permissions("execution".to_string())
                .build(),
            ResourceLifecycle::Live,
        )
        .build()
}

fn protected() -> StackSettings {
    StackSettings {
        signing: Some(StackSigning {
            fingerprint: format!("SHA256:{}", "9f".repeat(32)),
            channel: "production".to_string(),
        }),
        ..StackSettings::default()
    }
}

/// Plans `module` against mocked providers, with the verifier environment's id set, and runs
/// `checks` (HCL in the run block: more overrides and assertions) on the plan.
fn assert_planned(module: &alien_terraform::ModuleFiles, checks: &str, context: &str) {
    let mut files = linter_files(module);
    files.insert(
        "tests/verifier.tftest.hcl".to_string(),
        format!(
            r#"
mock_provider "azurerm" {{
  mock_data "azurerm_client_config" {{
    defaults = {{ tenant_id = "{UUID}", object_id = "{UUID}", client_id = "{UUID}" }}
  }}
}}
mock_provider "azapi" {{}}
override_resource {{
  target = azurerm_container_app_environment.signing_verifier
  values = {{ id = "{VERIFIER_ENVIRONMENT_ID}" }}
  override_during = plan
}}
variables {{
  name = "app"
  resource_prefix = "acme"
  token = "test-token"
  azure_subscription_id = "00000000-0000-0000-0000-000000000000"
  azure_resource_group_name = "acme-rg"
  azure_location = "eastus"
}}
run "verifier" {{
  command = plan
  {checks}
}}
"#
        ),
    );
    test_utils::terraform_test(&files).assert_ok(context);
}

/// What the mocked client reports as ids Azure validates as UUIDs.
const UUID: &str = "00000000-0000-0000-0000-000000000001";
const VERIFIER_ENVIRONMENT_ID: &str =
    "/subscriptions/0000/resourceGroups/acme-rg/providers/Microsoft.App/managedEnvironments/acme-verifier";
const STACK_ENVIRONMENT_ID: &str =
    "/subscriptions/0000/resourceGroups/acme-rg/providers/Microsoft.App/managedEnvironments/acme-default-container-env";

/// The verifier's `VERIFIER_AZURE_ENVIRONMENT`, as an HCL expression.
const VERIFIER_ENVIRONMENT: &str = r#"one([for e in azurerm_container_app.signing_verifier.template[0].container[0].env : e.value if e.name == "VERIFIER_AZURE_ENVIRONMENT"])"#;

#[test]
fn the_verifier_creates_workers_in_the_stacks_environment_not_its_own() {
    let module = render(
        &stack(&["default-container-env"]),
        TerraformTarget::Azure,
        protected(),
    );
    assert_planned(
        &module,
        &format!(
            r#"
  override_resource {{
    target = azurerm_container_app_environment.default_container_env
    values = {{ id = "{STACK_ENVIRONMENT_ID}" }}
    override_during = plan
  }}
  assert {{
    condition = {VERIFIER_ENVIRONMENT} == "{STACK_ENVIRONMENT_ID}"
    error_message = "The verifier must be told the stack's environment."
  }}
  assert {{
    condition = azurerm_container_app.signing_verifier.container_app_environment_id == "{VERIFIER_ENVIRONMENT_ID}"
    error_message = "The verifier runs in an environment of its own."
  }}
"#
        ),
        "protected Azure verifier environment",
    );
}

#[test]
fn a_stack_bound_to_a_shared_environment_names_that_one() {
    let mut bindings = ExternalBindings::new();
    bindings.insert(
        "default-container-env",
        ExternalBinding::ContainerAppsEnvironment(ContainerAppsEnvironmentBinding::new(
            "shared-env",
            SHARED_ENVIRONMENT,
            "shared-rg",
            "shared.example.azurecontainerapps.io",
        )),
    );
    let module = render(
        &stack(&["default-container-env"]),
        TerraformTarget::Azure,
        StackSettings {
            external_bindings: Some(bindings),
            ..protected()
        },
    );
    assert_planned(
        &module,
        &format!(
            r#"
  assert {{
    condition = {VERIFIER_ENVIRONMENT} == "{SHARED_ENVIRONMENT}"
    error_message = "The verifier must be told the bound environment."
  }}
"#
        ),
        "protected Azure verifier with a bound environment",
    );
}

#[test]
fn a_stack_with_several_environments_is_refused() {
    let error = try_render(
        &stack(&["default-container-env", "other-container-env"]),
        TerraformTarget::Azure,
        protected(),
    )
    .expect_err("the verifier's workers' environment would be a guess");
    assert_eq!(error.code, "OPERATION_NOT_SUPPORTED");
    assert!(
        error
            .to_string()
            .contains("the stack has 2 Container Apps environments"),
        "{error}"
    );
}
