use super::helpers::{sample_registry, SampleResource};
use alien_cloudformation::{
    generate_cloudformation_template, to_yaml, CfExpression, CloudFormationOptions,
    CloudFormationTarget, RegistrationMode,
};
use alien_core::{
    ResourceLifecycle, Stack, StackInputDefinition, StackInputEnvironmentMapping, StackInputKind,
    StackInputProvider, StackInputSetupMethod, StackInputValidation, StackSettings,
};

fn stack_with_inputs() -> Stack {
    Stack::new("inputs".to_string())
        .inputs(vec![
            StackInputDefinition {
                id: "apiBaseUrl".to_string(),
                kind: StackInputKind::String,
                provided_by: vec![StackInputProvider::Deployer],
                required: true,
                label: "API base URL".to_string(),
                description: "Base URL inside the customer environment.".to_string(),
                placeholder: None,
                default: None,
                platforms: None,
                setup_methods: Some(vec![StackInputSetupMethod::CloudFormation]),
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
            },
            StackInputDefinition {
                id: "tailScaleAuthKey".to_string(),
                kind: StackInputKind::Secret,
                provided_by: vec![StackInputProvider::Deployer],
                required: true,
                label: "Tailscale auth key".to_string(),
                description: "Auth key used by the setup runtime.".to_string(),
                placeholder: None,
                default: None,
                platforms: None,
                setup_methods: Some(vec![StackInputSetupMethod::CloudFormation]),
                validation: None,
                env: vec![],
            },
        ])
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

    let secret = template
        .parameters
        .get("InputTailScaleAuthKey")
        .expect("secret input parameter");
    assert_eq!(secret.no_echo, Some(true));

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
        &CfExpression::object([
            ("apiBaseUrl", CfExpression::ref_("InputApiBaseUrl")),
            ("tailScaleAuthKey", CfExpression::ref_("InputTailScaleAuthKey")),
        ])
    );

    let yaml = to_yaml(&template).expect("template should serialize");
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("stack inputs");
}
