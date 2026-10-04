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
            Vault::new("secrets".to_string()).build(),
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
            &VaultBinding::parameter_store(format!("{stack_name}-secrets")),
            &deployer_secret_vault_key("tailScaleAuthKey"),
            &DeployerSecretLocationContext::default(),
        )
        .expect("location");
        assert_eq!(evaluate(&output.value, stack_name), expected.name);
    }
}

#[test]
fn deployer_secrets_need_the_secrets_vault() {
    let stack = Stack::new("inputs".to_string())
        .inputs(vec![deployer_secret_input(vec![
            StackInputProvider::Deployer,
        ])])
        .add(
            SampleResource {
                id: "logs-bucket".to_string(),
            },
            ResourceLifecycle::Frozen,
        )
        .build();
    let registry = sample_registry();

    let error = generate_cloudformation_template(
        &stack,
        CloudFormationOptions {
            registry: &registry,
            target: CloudFormationTarget::Aws,
            stack_settings: StackSettings::default(),
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: None,
        },
    )
    .expect_err("a deployer secret without the secrets vault is refused");
    assert!(
        error.message.contains("'secrets' vault"),
        "{}",
        error.message
    );
}
