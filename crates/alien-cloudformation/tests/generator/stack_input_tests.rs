use super::helpers::{
    custom_resource_registration, render_built_ins_template, sample_registry, SampleResource,
};
use alien_cloudformation::{
    generate_cloudformation_template, to_yaml, CfExpression, CloudFormationOptions,
    CloudFormationTarget, RegistrationMode,
};
use alien_core::{
    deployer_secret_location, deployer_secret_vault_key, DeployerSecretLocationContext,
    ResourceLifecycle, Stack, StackInputDefinition, StackInputEnvironmentMapping, StackInputKind,
    StackInputProvider, StackInputValidation, StackSettings, Vault, VaultBinding,
};

fn stack_with_inputs() -> Stack {
    Stack::new("inputs".to_string())
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
            SampleResource {
                id: "logs-bucket".to_string(),
            },
            ResourceLifecycle::Frozen,
        )
        .build()
}

#[test]
fn cloudformation_emits_stack_inputs_as_parameters_and_registration_values() {
    let registry = sample_registry();
    let template = generate_cloudformation_template(
        &stack_with_inputs(),
        CloudFormationOptions {
            registry: &registry,
            target: CloudFormationTarget::Aws,
            stack_settings: StackSettings::default(),
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::CustomResource {
                lambda_arn: "arn:aws:lambda:us-east-1:123456789012:function:register".to_string(),
                callback_url: None,
            },
            description: Some("stack inputs".to_string()),
            management_trust: Default::default(),
        },
    )
    .expect("template should render");

    let api = template
        .parameters
        .get("InputApiBaseUrl")
        .expect("api input parameter");
    assert_eq!(api.parameter_type, "String");
    assert_eq!(api.min_length, Some(8));
    assert_eq!(api.max_length, Some(200));
    assert_eq!(api.allowed_pattern.as_deref(), Some("https://.+"));

    let registration = template
        .resources
        .get("DeploymentRegistration")
        .expect("registration custom resource");
    let input_values = registration
        .properties
        .get("InputValues")
        .expect("registration input values");
    assert_eq!(
        input_values,
        &CfExpression::object([("apiBaseUrl", CfExpression::ref_("InputApiBaseUrl"))])
    );

    let yaml = to_yaml(&template).expect("template should serialize");
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("stack inputs");
}

fn deployer_secret_input(provided_by: Vec<StackInputProvider>) -> StackInputDefinition {
    StackInputDefinition {
        id: "tailScaleAuthKey".to_string(),
        kind: StackInputKind::Secret,
        when: None,
        provided_by,
        required: true,
        label: "Tailscale auth key".to_string(),
        description: "Auth key used by the setup runtime.".to_string(),
        placeholder: None,
        default: None,
        platforms: None,
        validation: None,
        generate: None,
        env: vec![StackInputEnvironmentMapping {
            name: "TAILSCALE_AUTH_KEY".to_string(),
            target_resources: None,
            var_type: None,
        }],
    }
}

fn stack_with_deployer_secret(provided_by: Vec<StackInputProvider>) -> Stack {
    Stack::new("inputs".to_string())
        .inputs(vec![deployer_secret_input(provided_by)])
        .add(
            Vault::new("deployer-secrets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build()
}

/// Evaluates the `Fn::Sub`/`Fn::Join` subset outputs use, with the pseudo
/// parameter `AWS::StackName` bound to `stack_name`.
fn evaluate(expression: &CfExpression, stack_name: &str) -> String {
    match expression {
        CfExpression::String(value) => value.clone(),
        CfExpression::Object(function) => match function.iter().next() {
            Some((name, CfExpression::String(template))) if name == "Fn::Sub" => {
                template.replace("${AWS::StackName}", stack_name)
            }
            Some((name, CfExpression::List(arguments))) if name == "Fn::Join" => {
                let [CfExpression::String(delimiter), CfExpression::List(parts)] =
                    arguments.as_slice()
                else {
                    panic!("malformed Fn::Join: {expression:?}");
                };
                parts
                    .iter()
                    .map(|part| evaluate(part, stack_name))
                    .collect::<Vec<_>>()
                    .join(delimiter)
            }
            _ => panic!("unsupported expression: {expression:?}"),
        },
        _ => panic!("unsupported expression: {expression:?}"),
    }
}

#[test]
fn deployer_secrets_never_pass_through_the_template() {
    for provided_by in [
        vec![StackInputProvider::Deployer],
        vec![StackInputProvider::Developer, StackInputProvider::Deployer],
    ] {
        let (template, yaml) = render_built_ins_template(
            &stack_with_deployer_secret(provided_by),
            StackSettings::default(),
            custom_resource_registration(),
            CloudFormationTarget::Aws,
            "aws",
            "deployer secret",
        );

        // No parameter can carry the value, so neither the stack nor the
        // registration call to the control plane ever sees it.
        assert!(
            template
                .parameters
                .keys()
                .all(|name| !name.starts_with("Input")),
            "unexpected input parameters: {:?}",
            template.parameters.keys().collect::<Vec<_>>()
        );
        assert!(!yaml.contains("InputTailScaleAuthKey"));
        assert!(!template.resources["DeploymentRegistration"]
            .properties
            .contains_key("InputValues"));

        // The output names the SSM parameter the deployment checks and the
        // workload reads.
        let output = &template.outputs["DeployerSecretTailScaleAuthKey"];
        let stack_name = "acme-prod";
        let expected = deployer_secret_location(
            &VaultBinding::parameter_store(format!("{stack_name}-deployer-secrets")),
            &deployer_secret_vault_key("tailScaleAuthKey"),
            &DeployerSecretLocationContext::default(),
        )
        .expect("location");
        assert_eq!(evaluate(&output.value, stack_name), expected.name);
    }
}

#[test]
fn conditional_inputs_produce_a_valid_cloudformation_template() {
    for provider in [StackInputProvider::Deployer, StackInputProvider::Developer] {
        conditional_inputs_produce_a_valid_cloudformation_template_for_provider(provider);
    }
}

fn conditional_inputs_produce_a_valid_cloudformation_template_for_provider(
    provider: StackInputProvider,
) {
    let mut stack = stack_with_inputs();
    let mut input = serde_json::to_value(&stack.inputs[0]).unwrap();
    input["when"] = serde_json::json!({"input": "enabled", "equals": true});
    stack.inputs[0] = serde_json::from_value(input).unwrap();
    stack.inputs.push(StackInputDefinition::deployer_boolean(
        "enabled",
        "Enabled",
        "Enable the endpoint",
        Some(true),
    ));
    stack.inputs.last_mut().unwrap().provided_by = vec![provider];
    let mut labels = stack.inputs[0].clone();
    labels.id = "labels".into();
    labels.kind = StackInputKind::StringList;
    labels.required = false;
    labels.validation = None;
    stack.inputs.push(labels);
    let registry = sample_registry();
    let template = generate_cloudformation_template(
        &stack,
        CloudFormationOptions {
            registry: &registry,
            target: CloudFormationTarget::Aws,
            stack_settings: StackSettings::default(),
            setup_target: "aws".into(),
            setup_fingerprint: "test".into(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::CustomResource {
                lambda_arn: "arn:aws:lambda:us-east-1:123456789012:function:register".into(),
                callback_url: None,
            },
            description: None,
            management_trust: Default::default(),
        },
    )
    .unwrap();
    alien_cloudformation::test_utils::cfn_lint(&to_yaml(&template).unwrap())
        .assert_ok("conditional input template");
    let registered = serde_json::to_value(
        &template.resources["DeploymentRegistration"].properties["InputValues"],
    )
    .unwrap();
    for (raw, expected) in [
        ("", serde_json::json!([])),
        ("one,two", serde_json::json!(["one", "two"])),
    ] {
        let mut parameters = serde_json::json!({"InputEnabled":"true", "InputLabels": raw});
        for (name, condition) in &template.conditions {
            if name == "HasInputLabels" || name == "InputLabelsApplies" {
                parameters[name] =
                    evaluate_rule(&serde_json::to_value(condition).unwrap(), &parameters);
            }
        }
        assert_eq!(evaluate_rule(&registered["labels"], &parameters), expected);
    }
    let rule = template
        .rules
        .get("InputApiBaseUrlRequired")
        .expect("conditional required rule");
    for (enabled, url, accepted) in [
        ("false", "", true),
        ("true", "", false),
        ("true", "https://example.test", true),
    ] {
        let parameters = serde_json::json!({"InputEnabled": enabled, "InputApiBaseUrl": url});
        for assertion in &rule.assertions {
            assert_eq!(
                evaluate_rule(
                    &serde_json::to_value(&assertion.assertion).unwrap(),
                    &parameters
                ),
                serde_json::json!(accepted)
            );
        }
    }
}

// Evaluate the documented CloudFormation rule operators over parameter values.
fn evaluate_rule(
    expression: &serde_json::Value,
    parameters: &serde_json::Value,
) -> serde_json::Value {
    if let Some(reference) = expression.get("Ref") {
        return parameters[reference.as_str().unwrap()].clone();
    }
    if let Some(values) = expression.get("Fn::If") {
        let branch = if parameters[values[0].as_str().unwrap()].as_bool().unwrap() {
            1
        } else {
            2
        };
        return evaluate_rule(&values[branch], parameters);
    }
    if let Some(values) = expression.get("Fn::Split") {
        let value = evaluate_rule(&values[1], parameters);
        return serde_json::json!(value
            .as_str()
            .unwrap()
            .split(values[0].as_str().unwrap())
            .collect::<Vec<_>>());
    }
    if let Some(values) = expression.get("Fn::Equals") {
        return serde_json::json!(
            evaluate_rule(&values[0], parameters) == evaluate_rule(&values[1], parameters)
        );
    }
    if let Some(values) = expression.get("Fn::Not") {
        return serde_json::json!(!evaluate_rule(&values[0], parameters).as_bool().unwrap());
    }
    if let Some(values) = expression.get("Fn::Or") {
        return serde_json::json!(values.as_array().unwrap().iter().any(|value| evaluate_rule(
            value, parameters
        )
        .as_bool()
        .unwrap()));
    }
    assert!(
        !expression.is_object(),
        "unsupported rule operator: {expression}"
    );
    expression.clone()
}
