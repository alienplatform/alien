//! The anchor of a protected deployment: setup writes it once, the deployment's enforcers read it
//! back, and they must accept exactly what the developer's install guide pinned.

use super::helpers::{
    custom_resource_registration, render_built_ins_template, try_render_built_ins,
};
use alien_cloudformation::{CfTemplate, CloudFormationTarget};
use alien_core::{
    Kv, ReleaseTrustSettings, RemoteStackManagement, ResourceLifecycle, Stack, StackSettings,
};
use alien_signing::{Anchor, SessionApproval, SoftwareKey};
use serde_json::{json, Value};

const SIGNING: &str = "SHA256:9f2c3b1a7d6e5f4c8b0a9d2e1f3c4b5a6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a";

fn components() -> String {
    SoftwareKey::from_secret_bytes(&[22; 32])
        .expect("a valid test key")
        .public_key()
        .as_str()
        .to_string()
}

fn release_trust() -> ReleaseTrustSettings {
    ReleaseTrustSettings {
        project: "prj_123".to_string(),
        deployment: "dep_456".to_string(),
        signing: SIGNING.to_string(),
        components: components(),
        session_approval: SessionApproval::DeveloperAndCustomer,
    }
}

fn stack() -> Stack {
    Stack::new("orders".to_string())
        .add(
            RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            Kv::new("sessions".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build()
}

fn protected_settings(release_trust: ReleaseTrustSettings) -> StackSettings {
    StackSettings {
        release_trust: Some(release_trust),
        ..StackSettings::default()
    }
}

fn anchors(template: &CfTemplate) -> Vec<(&String, Value)> {
    template
        .resources
        .iter()
        .filter(|(_, resource)| resource.resource_type == "AWS::SSM::Parameter")
        .map(|(id, resource)| {
            (
                id,
                serde_json::to_value(&resource.properties).expect("properties serialize"),
            )
        })
        .collect()
}

/// What CloudFormation writes into the parameter for a stack named `stack_name`.
fn substituted(value: &Value, stack_name: &str) -> String {
    let template = value["Fn::Sub"]
        .as_str()
        .unwrap_or_else(|| panic!("the anchor is one Fn::Sub string: {value}"));
    let written = template.replace("${AWS::StackName}", stack_name);
    assert!(
        !written.contains("${"),
        "the stack name is the anchor's only substitution: {template}"
    );
    written
}

#[test]
fn setup_writes_an_anchor_the_enforcers_accept() {
    let (template, _yaml) = render_built_ins_template(
        &stack(),
        protected_settings(release_trust()),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "protected deployment anchor",
    );

    let anchors = anchors(&template);
    assert_eq!(anchors.len(), 1, "one anchor: {anchors:?}");
    let (logical_id, properties) = &anchors[0];
    assert_eq!(
        properties["Name"],
        json!({ "Fn::Sub": "/_release-trust/${AWS::StackName}" })
    );
    assert_eq!(properties["Type"], "String");
    assert_eq!(properties["Tier"], "Standard");
    // No deployment tag, so no grant conditioned on that tag reaches the anchor.
    assert!(properties.get("Tags").is_none(), "{properties}");

    let written = substituted(&properties["Value"], "orders-prod");
    let anchor = Anchor::parse(written.as_bytes()).expect("the enforcers accept the anchor");
    assert_eq!(
        anchor,
        Anchor {
            project: "prj_123".to_string(),
            deployment: "dep_456".to_string(),
            protected: true,
            signing: SIGNING.to_string(),
            components: components(),
            session_approval: SessionApproval::DeveloperAndCustomer,
            inputs: Some("aws-ssm:/_deployer-inputs/orders-prod".to_string()),
            resource_prefix: Some("orders-prod".to_string()),
        }
    );
    // The enforcers verify components builds with this key.
    assert_eq!(
        anchor.components_key().expect("components key").as_str(),
        components()
    );

    // The anchor exists before registration hands the deployment to the control plane.
    let registration = &template.resources["DeploymentRegistration"];
    assert!(
        registration.depends_on.contains(logical_id),
        "{:?}",
        registration.depends_on
    );
}

#[test]
fn an_unprotected_deployment_has_no_anchor() {
    let (template, _yaml) = render_built_ins_template(
        &stack(),
        StackSettings::default(),
        custom_resource_registration(),
        CloudFormationTarget::Aws,
        "aws",
        "unprotected deployment",
    );
    assert!(anchors(&template).is_empty());
}

#[test]
fn setup_refuses_an_anchor_the_enforcers_would_refuse() {
    let short_fingerprint = ReleaseTrustSettings {
        signing: "SHA256:9f2c".to_string(),
        ..release_trust()
    };
    let not_a_key = ReleaseTrustSettings {
        components: "ecdsa-p256:bm90IGEga2V5".to_string(),
        ..release_trust()
    };
    let substituting_project = ReleaseTrustSettings {
        project: "${AWS::AccountId}".to_string(),
        ..release_trust()
    };
    for (settings, reason) in [
        (short_fingerprint, "signing"),
        (not_a_key, "components"),
        (substituting_project, "project"),
    ] {
        let error = try_render_built_ins(
            &stack(),
            protected_settings(settings),
            custom_resource_registration(),
            CloudFormationTarget::Aws,
            "aws",
            "invalid anchor",
        )
        .expect_err("an invalid anchor fails the render");
        assert_eq!(error.code, "RELEASE_TRUST_SETTINGS_INVALID", "{reason}");
        assert!(
            error.message.contains(reason),
            "{reason}: {}",
            error.message
        );
    }
}

#[test]
fn a_kubernetes_package_does_not_take_the_cloud_anchor() {
    let error = try_render_built_ins(
        &stack(),
        protected_settings(release_trust()),
        custom_resource_registration(),
        CloudFormationTarget::Eks,
        "eks",
        "protected EKS deployment",
    )
    .expect_err("the chart holds the anchor on Kubernetes");
    assert_eq!(error.code, "OPERATION_NOT_SUPPORTED");
    assert!(
        error.message.contains("releaseSigning"),
        "{}",
        error.message
    );
}
