//! The `ManagementExternalId` parameter on the roles the managing role assumes.
//!
//! These tests resolve each trust policy the way CloudFormation does for both
//! answers to `HasManagementExternalId`. Empty must give back exactly the
//! trust stacks had before the parameter existed, which is what keeps an old
//! stack's access unchanged when it is updated without a value. Set must make
//! `sts:ExternalId` a requirement.
//!
//! To check against AWS: create a stack from a template rendered before this
//! change, run a change set to the new template with and without
//! `ManagementExternalId`, and confirm the roles show only an in-place
//! `AssumeRolePolicyDocument` modification (Replacement: False). Then assume
//! the management role without the value, with a wrong one and with the right
//! one; only the last succeeds once the value is set.

use super::helpers::{custom_resource_registration, render_built_ins_template, resolve, Declined};
use alien_cloudformation::{CfExpression, CfTemplate, CloudFormationTarget, RegistrationMode};
use alien_core::{
    KubernetesCluster, KubernetesClusterOwnership, KubernetesClusterProvider,
    KubernetesHeartbeatMode, RemoteBindings, RemoteStackManagement, ResourceLifecycle, Stack,
    StackSettings, Storage, MANAGEMENT_EXTERNAL_ID_PARAMETER_PATTERN,
};
use serde_json::{json, Value};
use std::collections::HashMap;

const PARAMETER: &str = "ManagementExternalId";
const CONDITION: &str = "HasManagementExternalId";

fn managed_stack() -> Stack {
    Stack::new("external-id".to_string())
        .add(
            Storage::new("assets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteBindings::new("access".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build()
}

fn render(
    stack: &Stack,
    registration: RegistrationMode,
    target: CloudFormationTarget,
) -> CfTemplate {
    let setup_target = match target {
        CloudFormationTarget::Eks => "kubernetes",
        _ => "aws",
    };
    render_built_ins_template(
        stack,
        StackSettings::default(),
        registration,
        target,
        setup_target,
        "management external id",
    )
    .0
}

fn trust_policy(template: &CfTemplate, role: &str, external_id_set: bool) -> Value {
    let document = template
        .resources
        .get(role)
        .unwrap_or_else(|| panic!("template should contain {role}"))
        .properties
        .get("AssumeRolePolicyDocument")
        .expect("role should have a trust policy");
    let conditions = HashMap::from([(CONDITION, external_id_set)]);
    let resolved = resolve(document, &conditions, Declined::Removed).expect("trust policy");
    match resolved {
        // EKS trust documents are JSON text built with Fn::Sub.
        CfExpression::String(text) => serde_json::from_str(&text).expect("trust policy JSON"),
        other => serde_json::to_value(other).expect("trust policy serializes"),
    }
}

fn managing_statement(policy: &Value) -> &Value {
    policy["Statement"]
        .as_array()
        .expect("statements")
        .iter()
        .find(|statement| statement["Sid"] == "AllowManagingRole")
        .expect("managing-role statement")
}

/// The trust every stack had before `ManagementExternalId` existed.
fn legacy_trust() -> Value {
    json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Sid": "AllowManagingRole",
            "Effect": "Allow",
            "Principal": { "AWS": { "Ref": "ManagingRoleArn" } },
            "Action": "sts:AssumeRole",
            "Condition": { "StringEquals": { "aws:PrincipalArn": { "Ref": "ManagingRoleArn" } } }
        }]
    })
}

#[test]
fn management_and_remote_bindings_roles_require_the_external_id_once_it_is_set() {
    let template = render(
        &managed_stack(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
    );

    for role in ["ManagementRole", "AccessRole"] {
        assert_eq!(
            trust_policy(&template, role, false),
            legacy_trust(),
            "{role}: an empty value must leave the old trust unchanged"
        );
        assert_eq!(
            managing_statement(&trust_policy(&template, role, true))["Condition"],
            json!({ "StringEquals": {
                "aws:PrincipalArn": { "Ref": "ManagingRoleArn" },
                "sts:ExternalId": { "Ref": PARAMETER }
            }}),
            "{role}: a set value must be required as sts:ExternalId"
        );
    }

    let parameter = template.parameters.get(PARAMETER).expect("parameter");
    assert_eq!(parameter.no_echo, Some(true));
    assert_eq!(parameter.default, Some(CfExpression::from("")));
    assert_eq!(
        parameter.allowed_pattern.as_deref(),
        Some(MANAGEMENT_EXTERNAL_ID_PARAMETER_PATTERN)
    );
    assert_eq!(
        serde_json::to_value(template.conditions.get(CONDITION).expect("condition")).unwrap(),
        json!({ "Fn::Not": [{ "Fn::Equals": [{ "Ref": PARAMETER }, ""] }] })
    );
}

#[test]
fn registration_receives_the_value_but_outputs_never_do() {
    let template = render(
        &managed_stack(),
        RegistrationMode::Both {
            lambda_arn: "arn:aws:lambda:us-east-1:123456789012:function:register".to_string(),
            callback_url: None,
        },
        CloudFormationTarget::Aws,
    );

    assert_eq!(
        template.resources["DeploymentRegistration"]
            .properties
            .get(PARAMETER),
        Some(&CfExpression::ref_(PARAMETER)),
        "registration compares the supplied value with the issued one"
    );
    // CloudFormation does not mask NoEcho values in Outputs.
    let outputs = serde_json::to_string(&template.outputs).unwrap();
    assert!(
        !outputs.contains(PARAMETER),
        "an output exposes the ExternalId: {outputs}"
    );
}

#[test]
fn eks_management_trust_adds_the_external_id_only_to_the_managing_role_statement() {
    let stack = Stack::new("kubernetes".to_string())
        .add(
            KubernetesCluster::new("kubernetes".to_string())
                .provider(KubernetesClusterProvider::Eks)
                .ownership(KubernetesClusterOwnership::Managed)
                .namespace("alien".to_string())
                .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let template = render(
        &stack,
        custom_resource_registration(),
        CloudFormationTarget::Eks,
    );

    // The EKS document is JSON text built with Fn::Sub, because the pod
    // statement needs the cluster's OIDC issuer. Fn::If picks one of two texts.
    let document = &template.resources["ManagementRole"].properties["AssumeRolePolicyDocument"];
    let CfExpression::Object(fields) = document else {
        panic!("trust policy should be an Fn::If: {document:?}")
    };
    let CfExpression::List(branches) = &fields["Fn::If"] else {
        panic!("trust policy should be an Fn::If: {document:?}")
    };
    assert_eq!(branches[0], CfExpression::from(CONDITION));
    let branch = |index: usize| {
        let CfExpression::Object(sub) = &branches[index] else {
            panic!("branch should be Fn::Sub")
        };
        let CfExpression::List(arguments) = &sub["Fn::Sub"] else {
            panic!("branch should be Fn::Sub")
        };
        let [CfExpression::String(text), variables] = &arguments[..] else {
            panic!("Fn::Sub should have a text and variables")
        };
        // `${PodTrust}` stands for a whole statement; the other placeholders
        // sit inside JSON strings.
        let policy: Value =
            serde_json::from_str(&text.replace("${PodTrust}", r#"{"Sid":"PodTrust"}"#))
                .expect("trust policy text is JSON");
        (policy, variables.clone())
    };
    let (with, with_variables) = branch(1);
    let (without, without_variables) = branch(2);

    assert_eq!(
        managing_statement(&without)["Condition"],
        json!({ "StringEquals": { "aws:PrincipalArn": "${ManagingRole}" } })
    );
    assert_eq!(
        managing_statement(&with)["Condition"],
        json!({ "StringEquals": {
            "aws:PrincipalArn": "${ManagingRole}",
            "sts:ExternalId": format!("${{{PARAMETER}}}")
        }})
    );
    assert_eq!(with["Statement"][1], json!({ "Sid": "PodTrust" }));
    assert_eq!(without["Statement"][1], json!({ "Sid": "PodTrust" }));
    assert_eq!(
        with_variables, without_variables,
        "the in-cluster manager's web-identity trust must not change"
    );
}

#[test]
fn a_template_without_managed_roles_declares_no_external_id() {
    let stack = Stack::new("storage-only".to_string())
        .add(
            Storage::new("assets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    let template = render(
        &stack,
        custom_resource_registration(),
        CloudFormationTarget::Aws,
    );

    assert!(!template.parameters.contains_key(PARAMETER));
    assert!(!template.conditions.contains_key(CONDITION));
    assert!(!template.resources["DeploymentRegistration"]
        .properties
        .contains_key(PARAMETER));
}
