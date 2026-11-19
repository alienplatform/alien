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
            when: None,
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
            generate: None,
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
            when: None,
            provided_by,
            required: true,
            label: "API key".to_string(),
            description: "Secret key for setup.".to_string(),
            placeholder: None,
            default: None,
            platforms: None,
            validation: None,
            generate: None,
            env: vec![StackInputEnvironmentMapping {
                name: "API_KEY".to_string(),
                target_resources: None,
                var_type: None,
            }],
        }])
        .add(
            Vault::new("deployer-secrets".to_string()).build(),
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
            &VaultBinding::parameter_store("acme-prod-deployer-secrets"),
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

/// On a Kubernetes target pods read deployer secrets from Kubernetes Secrets in the release
/// namespace, so the module's outputs name the Secret the deployer writes. The cloud vault's IAM
/// stays in the module, at its earlier addresses: an install from an earlier release still
/// records that vault, and removing it is a separate change.
#[test]
fn kubernetes_targets_put_deployer_secrets_in_the_release_namespace() {
    fn lower(args: FuncArgs) -> Result<Value, String> {
        Ok(Value::from(
            args[0]
                .as_str()
                .ok_or("lower takes a string")?
                .to_lowercase(),
        ))
    }

    for target in [
        TerraformTarget::Eks,
        TerraformTarget::Gke,
        TerraformTarget::Aks,
    ] {
        let module = render(
            &deployer_secret_stack(vec![StackInputProvider::Deployer]),
            target,
            StackSettings::default(),
        );
        let rendered = module.files.values().cloned().collect::<String>();
        // No removed block: an earlier release's deployer vault resources stay managed.
        assert!(!rendered.contains("removed {"), "{target:?}");

        let outputs = module.get("outputs.tf").expect("outputs.tf");
        let body: hcl::Body = hcl::from_str(outputs).expect("outputs.tf parses");
        let value = body
            .blocks()
            .find(|block| {
                block.identifier() == "output"
                    && block.labels().first().map(|label| label.as_str())
                        == Some("deployer_secret_api_key")
            })
            .expect("deployer secret output")
            .body()
            .attributes()
            .find(|attribute| attribute.key() == "value")
            .expect("output value")
            .expr()
            .clone();
        let mut context = Context::new();
        context.declare_var("local", hcl::value!({ resource_prefix = "Acme-Prod" }));
        context.declare_var("var", hcl::value!({ kubernetes_namespace = "apps" }));
        context.declare_func(
            "lower",
            FuncDef::builder().param(ParamType::String).build(lower),
        );
        let output = value.evaluate(&context).expect("output evaluates");
        let object = output.as_object().expect("object output");

        let expected = deployer_secret_location(
            &VaultBinding::kubernetes_secret("apps", "acme-prod-deployer-secrets"),
            &deployer_secret_vault_key("apiKey"),
            &DeployerSecretLocationContext::default(),
        )
        .expect("location");
        assert_eq!(object["name"].as_str(), Some(expected.name.as_str()));
        assert_eq!(object["namespace"].as_str(), Some("apps"));
        assert_eq!(object["key"].as_str(), Some("value"));

        assert_terraform_valid(&module, "kubernetes deployer secret");
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
            when: None,
            provided_by: vec![StackInputProvider::Deployer],
            required: true,
            label: format!("Input {id}"),
            description: format!("Test input {id}."),
            placeholder: None,
            default: None,
            platforms: None,
            validation: None,
            generate: None,
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

#[test]
fn conditional_inputs_validate_with_terraform_plan() {
    for provider in [StackInputProvider::Deployer, StackInputProvider::Developer] {
        conditional_inputs_validate_with_terraform_plan_for_provider(provider);
    }
}

fn conditional_inputs_validate_with_terraform_plan_for_provider(provider: StackInputProvider) {
    let mut stack = plain_input_stack();
    let mut input = serde_json::to_value(&stack.inputs[0]).unwrap();
    input["when"] = serde_json::json!({"input": "enabled", "equals": true});
    stack.inputs[0] = serde_json::from_value(input).unwrap();
    stack.inputs.push(StackInputDefinition::deployer_boolean(
        "enabled",
        "Enabled",
        "Enable the endpoint",
        Some(true),
    ));
    stack.inputs.last_mut().unwrap().provided_by = if provider == StackInputProvider::Developer {
        vec![StackInputProvider::Developer, StackInputProvider::Deployer]
    } else {
        vec![StackInputProvider::Deployer]
    };
    let mut list = stack.inputs[0].clone();
    list.id = "labels".into();
    list.kind = StackInputKind::StringList;
    list.required = false;
    list.validation = None;
    stack.inputs.push(list);
    let module = render(&stack, TerraformTarget::Aws, StackSettings::default());
    assert_terraform_valid(&module, "conditional_inputs");

    // Exercise the generated input interface without provisioning cloud resources.
    let variables: hcl::Body = hcl::from_str(module.get("variables.tf").unwrap()).unwrap();
    let variables = hcl::Body::from(
        variables
            .into_iter()
            .filter(|structure| match structure {
                hcl::Structure::Block(block) => {
                    block.identifier.as_str() == "variable"
                        && block
                            .labels
                            .first()
                            .is_some_and(|label| label.as_str().starts_with("input_"))
                }
                _ => false,
            })
            .collect::<Vec<_>>(),
    );
    let files =
        indexmap::IndexMap::from([("inputs.tf".to_string(), hcl::to_string(&variables).unwrap())]);
    let disabled = super::helpers::test_utils::terraform_plan_with_vars(
        &files,
        &[("input_enabled", "false"), ("input_api_base_url", "\"\"")],
    );
    disabled.assert_ok("inactive required input accepts empty value");
    for (enabled, labels, accepted) in [
        ("false", "[42]", true),
        ("true", "[42]", false),
        ("true", "[null]", false),
        ("true", "[\"label\"]", true),
    ] {
        let plan = super::helpers::test_utils::terraform_plan_with_vars(
            &files,
            &[
                ("input_enabled", enabled),
                ("input_api_base_url", "\"https://example.test\""),
                ("input_labels", labels),
            ],
        );
        assert_eq!(
            plan.is_ok(),
            accepted,
            "{enabled}/{labels}: {}",
            plan.stderr
        );
        if !accepted {
            assert!(plan.stderr.contains("input_labels"), "{}", plan.stderr);
        }
    }

    let enabled = super::helpers::test_utils::terraform_plan_with_vars(
        &files,
        &[("input_enabled", "true"), ("input_api_base_url", "\"\"")],
    );
    assert!(!enabled.is_ok(), "an active required input cannot be empty");
    assert!(
        enabled.stderr.contains("input_api_base_url"),
        "{}",
        enabled.stderr
    );
    super::helpers::test_utils::terraform_plan_with_vars(
        &files,
        &[
            ("input_enabled", "true"),
            ("input_api_base_url", "\"https://example.test\""),
        ],
    )
    .assert_ok("active valid input");
}
