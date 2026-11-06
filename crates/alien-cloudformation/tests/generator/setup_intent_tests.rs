//! The verified setup intent copies the import fields into the registration's
//! metadata, where the stack's own management role reads them back.

use super::helpers::{custom_resource_registration, render_built_ins_template};
use alien_cloudformation::{
    add_verified_setup_intent, CfExpression, CfParameter, CloudFormationTarget,
};
use alien_core::{RemoteStackManagement, ResourceLifecycle, Stack, StackSettings, Storage};
use serde_json::Value;

fn template() -> alien_cloudformation::CfTemplate {
    let stack = Stack::new("intent".to_string())
        .add(
            Storage::new("assets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    render_built_ins_template(
        &stack,
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "setup intent",
    )
    .0
}

#[test]
fn intent_carries_the_import_fields_and_a_read_policy_on_the_own_stack() {
    let mut template = template();
    add_verified_setup_intent(&mut template).unwrap();
    let yaml = alien_cloudformation::to_yaml(&template).unwrap();
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("setup intent");

    let registration = &template.resources["DeploymentRegistration"];
    let intent = &registration.metadata["SetupIntent"];
    assert_eq!(intent["version"], "1");
    assert_eq!(
        intent["installId"],
        serde_json::json!({"Ref": "AWS::StackId"})
    );
    assert_eq!(
        intent["audience"],
        serde_json::json!({"Ref": "SetupManagerAudience"})
    );
    let source = intent["source"].as_object().unwrap();
    for field in [
        "DeploymentName",
        "ResourcePrefix",
        "Platform",
        "Region",
        "SetupTarget",
        "SetupImportFormatVersion",
        "SetupFingerprint",
        "SetupFingerprintVersion",
        "ManagementConfig",
        "StackSettings",
        "Resources",
    ] {
        assert!(source.contains_key(field), "intent lacks {field}");
    }
    assert!(!source.contains_key("SourceKind"));
    // Metadata is static, so CloudFormation only invokes the resource on a
    // real change. The identical property makes an intent change an update.
    assert_eq!(
        serde_json::to_value(&registration.properties["SetupIntent"]).unwrap(),
        *intent
    );

    let policy = serde_json::to_value(&template.resources["SetupIntentReadPolicy"]).unwrap();
    let statement = &policy["Properties"]["PolicyDocument"]["Statement"][0];
    assert_eq!(statement["Action"], "cloudformation:DescribeStackResource");
    assert_eq!(
        statement["Resource"],
        serde_json::json!({"Ref": "AWS::StackId"})
    );
    assert_eq!(
        policy["Properties"]["Roles"][0]["Ref"],
        Value::from("ManagementRole")
    );
}

#[test]
fn intent_refuses_a_sensitive_parameter_and_leaves_the_template_unchanged() {
    let mut template = template();
    template.parameters.insert(
        "Hidden".into(),
        CfParameter {
            parameter_type: "String".into(),
            description: None,
            default: None,
            allowed_values: None,
            allowed_pattern: None,
            constraint_description: None,
            min_length: None,
            max_length: None,
            min_value: None,
            max_value: None,
            no_echo: Some(true),
        },
    );
    template
        .resources
        .get_mut("DeploymentRegistration")
        .unwrap()
        .properties
        .insert("DeploymentName".into(), CfExpression::ref_("Hidden"));
    let before = alien_cloudformation::to_yaml(&template).unwrap();
    assert!(add_verified_setup_intent(&mut template).is_err());
    assert_eq!(alien_cloudformation::to_yaml(&template).unwrap(), before);
}
