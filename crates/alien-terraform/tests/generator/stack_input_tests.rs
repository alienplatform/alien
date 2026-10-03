use super::helpers::{assert_terraform_valid, render, try_render};
use alien_core::{
    deployer_secret_location, deployer_secret_vault_key, DeployerSecretLocationContext,
    ResourceLifecycle, Stack, StackInputDefinition, StackInputEnvironmentMapping, StackInputKind,
    StackInputProvider, StackInputValidation, StackSettings, Storage, Vault, VaultBinding,
};
use alien_terraform::TerraformTarget;
use hcl::eval::{Context, Evaluate, FuncArgs, FuncDef, ParamType};
use hcl::Value;

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

fn deployer_secret_stack(provided_by: Vec<StackInputProvider>) -> Stack {
    Stack::new("secret-input-stack".to_string())
        .inputs(vec![StackInputDefinition {
            id: "apiKey".to_string(),
            kind: StackInputKind::Secret,
            provided_by,
            required: true,
            label: "API key".to_string(),
            description: "Secret key for setup.".to_string(),
            placeholder: None,
            default: None,
            platforms: None,
            validation: None,
            env: vec![StackInputEnvironmentMapping {
                name: "API_KEY".to_string(),
                target_resources: None,
                var_type: None,
            }],
        }])
        .add(
            Vault::new("secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build()
}

/// Evaluates output `name` of `outputs.tf` with `local.resource_prefix` bound
/// to `resource_prefix`.
fn evaluate_output_name(outputs_tf: &str, output: &str, resource_prefix: &str) -> String {
    fn join(args: FuncArgs) -> Result<Value, String> {
        let delimiter = args[0].as_str().ok_or("delimiter must be a string")?;
        let parts = args[1]
            .as_array()
            .ok_or("parts must be a list")?
            .iter()
            .map(|part| {
                part.as_str()
                    .map(str::to_string)
                    .ok_or("parts must be strings")
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Value::from(parts.join(delimiter)))
    }

    let body: hcl::Body = hcl::from_str(outputs_tf).expect("outputs.tf parses");
    let block = body
        .blocks()
        .find(|block| {
            block.identifier() == "output"
                && block.labels().first().map(|label| label.as_str()) == Some(output)
        })
        .unwrap_or_else(|| panic!("output {output} in:\n{outputs_tf}"));
    let value = block
        .body()
        .attributes()
        .find(|attribute| attribute.key() == "value")
        .expect("output value")
        .expr()
        .clone();

    let mut context = Context::new();
    context.declare_var(
        "local",
        hcl::value!({ resource_prefix = (resource_prefix) }),
    );
    context.declare_func(
        "join",
        FuncDef::builder()
            .param(ParamType::String)
            .param(ParamType::Array(Box::new(ParamType::String)))
            .build(join),
    );
    let value = value.evaluate(&context).expect("output evaluates");
    value
        .as_object()
        .and_then(|object| object.get("name"))
        .and_then(Value::as_str)
        .expect("name is a string")
        .to_string()
}

/// No Terraform variable can carry a deployer secret, so it never reaches
/// the module's state, its outputs or the registration call to the control
/// plane. The module names where the deployer writes it instead.
#[test]
fn deployer_secrets_never_reach_terraform_variables_or_state() {
    for provided_by in [
        vec![StackInputProvider::Deployer],
        vec![StackInputProvider::Developer, StackInputProvider::Deployer],
    ] {
        let module = render(
            &deployer_secret_stack(provided_by),
            TerraformTarget::Aws,
            StackSettings::default(),
        );
        for (path, contents) in module.iter() {
            assert!(
                !contents.contains("input_api_key") && !contents.contains("apiKey"),
                "{path} refers to the deployer secret input:\n{contents}"
            );
        }

        let outputs = module.get("outputs.tf").expect("outputs.tf");
        let expected = deployer_secret_location(
            &VaultBinding::parameter_store("acme-prod-secrets"),
            &deployer_secret_vault_key("apiKey"),
            &DeployerSecretLocationContext::default(),
        )
        .expect("location");
        assert_eq!(
            evaluate_output_name(outputs, "deployer_secret_api_key", "acme-prod"),
            expected.name
        );

        assert_terraform_valid(&module, "deployer secret");
    }
}

#[test]
fn deployer_secrets_need_the_secrets_vault() {
    let stack = Stack::new("secret-input-stack".to_string())
        .inputs(deployer_secret_stack(vec![StackInputProvider::Deployer]).inputs)
        .add(
            Storage::new("data".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();

    let error = try_render(&stack, TerraformTarget::Aws, StackSettings::default())
        .expect_err("a deployer secret without the secrets vault is refused");
    assert!(
        error.message.contains("'secrets' vault"),
        "{}",
        error.message
    );
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
