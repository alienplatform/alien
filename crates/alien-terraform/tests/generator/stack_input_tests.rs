use super::helpers::{assert_terraform_valid, render};
use alien_core::{
    ResourceLifecycle, Stack, StackInputDefinition, StackInputEnvironmentMapping, StackInputKind,
    StackInputProvider, StackInputValidation, StackSettings, Storage,
};
use alien_terraform::{generate_terraform_module, TerraformOptions, TerraformTarget, TfRegistry};

fn plain_input_stack() -> Stack {
    Stack::new("input-stack".to_string())
        .inputs(vec![StackInputDefinition {
            id: "apiBaseUrl".to_string(),
            kind: StackInputKind::String,
            provided_by: vec![StackInputProvider::Deployer],
            required: true,
            label: "API base URL".to_string(),
            description: "Base URL inside the customer environment.".to_string(),
            placeholder: None,
            default: None,
            platforms: None,
            validation: Some(StackInputValidation {
                min_length: Some(8),
                max_length: Some(200),
                pattern: Some("https://.+".to_string()),
                format: None,
                min: None,
                max: None,
                values: None,
                min_items: None,
                max_items: None,
            }),
            env: vec![StackInputEnvironmentMapping {
                name: "API_BASE_URL".to_string(),
                target_resources: None,
                var_type: None,
            }],
        }])
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build()
}

#[test]
fn terraform_emits_non_secret_stack_input_variables_and_registration_values() {
    let module = render(
        &plain_input_stack(),
        TerraformTarget::Aws,
        StackSettings::default(),
    );
    let variables = module.get("variables.tf").expect("variables.tf");
    assert!(variables.contains("variable \"input_api_base_url\""));
    assert!(variables.contains("length(var.input_api_base_url) >= 8"));
    assert!(variables.contains("can(regex(\"^(?:https://.+)$\", var.input_api_base_url))"));

    let registration = module.get("registration.tf").expect("registration.tf");
    assert!(registration.contains("inputValues = {"));
    assert!(registration.contains("apiBaseUrl = var.input_api_base_url"));

    assert_terraform_valid(&module, "stack_inputs");
}

#[test]
fn terraform_rejects_deployer_secret_inputs_until_provider_state_safety_exists() {
    let stack = Stack::new("secret-input-stack".to_string())
        .inputs(vec![StackInputDefinition {
            id: "apiKey".to_string(),
            kind: StackInputKind::Secret,
            provided_by: vec![StackInputProvider::Deployer],
            required: true,
            label: "API key".to_string(),
            description: "Secret key for setup.".to_string(),
            placeholder: None,
            default: None,
            platforms: None,
            validation: None,
            env: vec![],
        }])
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let registry = TfRegistry::built_in();

    let err = generate_terraform_module(
        &stack,
        TerraformTarget::Aws,
        TerraformOptions {
            registry: &registry,
            display_name: None,
            stack_settings: StackSettings::default(),
            registration: None,
            helm_install: None,
            supported_aws_regions: Vec::new(),
        },
    )
    .expect_err("secret deployer inputs should be blocked");

    assert!(err
        .message
        .contains("Terraform deployer-provided secret stack inputs are not enabled"));
}
