use super::helpers::{assert_terraform_valid, render, try_render};
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

/// `deployment_input_values` publishes every deployer answer as a plain
/// module output so registration flows outside Terraform can read it. That is
/// only safe while secret inputs cannot reach the module at all. This holds
/// the two together: if the refusal above is ever relaxed, this fails and
/// whoever relaxes it has to decide what the output does with a secret.
#[test]
fn a_secret_input_cannot_reach_the_input_values_output() {
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

    match try_render(&stack, TerraformTarget::Aws, StackSettings::default()) {
        Err(_) => {}
        Ok(module) => {
            let outputs = module
                .iter()
                .find(|(name, _)| *name == "outputs.tf")
                .map(|(_, contents)| contents.clone())
                .expect("outputs.tf should render");
            assert!(
                !outputs.contains("deployment_input_values"),
                "a secret answer may not be published as a plain output; mark it \
                 sensitive or keep refusing secret inputs:\n{outputs}"
            );
        }
    }
}

/// Distinct ids can normalize to the same Terraform variable; a silent shadow
/// would make both inputs read one variable, so generation must refuse.
#[test]
fn inputs_colliding_after_normalization_are_refused() {
    fn boolean_input(id: &str) -> StackInputDefinition {
        StackInputDefinition {
            id: id.to_string(),
            kind: StackInputKind::Boolean,
            provided_by: vec![StackInputProvider::Deployer],
            required: true,
            label: format!("Input {id}"),
            description: format!("Test input {id}."),
            placeholder: None,
            default: None,
            platforms: None,
            validation: None,
            env: Vec::new(),
        }
    }

    let stack = Stack::new("input-stack".to_string())
        .inputs(vec![boolean_input("fooBar"), boolean_input("foo_bar")])
        .add(
            Storage::new("files".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let error = try_render(&stack, TerraformTarget::Aws, StackSettings::default())
        .expect_err("colliding variable names must refuse to render");
    assert!(error.message.contains("fooBar"), "{}", error.message);
    assert!(error.message.contains("foo_bar"), "{}", error.message);
    assert!(error.message.contains("input_foo_bar"), "{}", error.message);
    assert!(
        !error.message.contains("  "),
        "the message should render without space runs: {}",
        error.message
    );
}
