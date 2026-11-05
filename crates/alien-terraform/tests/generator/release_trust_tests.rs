//! The anchor of a protected deployment, as Terraform setup writes it: the deployment's
//! enforcers must accept exactly what the developer's install guide pinned.

use super::helpers::{assert_terraform_valid, render, try_render};
use alien_core::{
    Kv, ReleaseTrustSettings, RemoteStackManagement, ResourceLifecycle, Stack, StackSettings,
};
use alien_release_trust::{Anchor, SessionApproval, SoftwareKey};
use alien_terraform::{ModuleFiles, TerraformTarget};
use hcl::eval::{Context, Evaluate};

const SIGNING: &str = "SHA256:9f2c3b1a7d6e5f4c8b0a9d2e1f3c4b5a6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a";

fn components() -> String {
    SoftwareKey::from_secret_bytes(&[22; 32])
        .expect("a valid test key")
        .public_key()
        .as_str()
        .to_string()
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

fn protected() -> StackSettings {
    StackSettings {
        release_trust: Some(ReleaseTrustSettings {
            project: "prj_123".to_string(),
            deployment: "dep_456".to_string(),
            signing: SIGNING.to_string(),
            components: components(),
            session_approval: SessionApproval::Developer,
        }),
        ..StackSettings::default()
    }
}

/// The `aws_ssm_parameter` blocks of the module.
fn parameters(module: &ModuleFiles) -> Vec<(String, hcl::Block)> {
    module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .flat_map(|(path, contents)| {
            let body: hcl::Body =
                hcl::parse(contents).unwrap_or_else(|error| panic!("{path} parses: {error}"));
            body.into_blocks()
                .filter(|block| {
                    block.identifier() == "resource"
                        && block.labels().first().map(|label| label.as_str())
                            == Some("aws_ssm_parameter")
                })
                .map(|block| (path.to_string(), block))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn attribute<'a>(block: &'a hcl::Block, name: &str) -> &'a hcl::Expression {
    block
        .body()
        .attributes()
        .find(|attribute| attribute.key() == name)
        .unwrap_or_else(|| panic!("the anchor sets {name}"))
        .expr()
}

/// The addresses the registration resource in `registration.tf` names in `depends_on`.
fn registration_depends_on(module: &ModuleFiles) -> Vec<String> {
    let body: hcl::Body =
        hcl::parse(&module.files["registration.tf"]).expect("registration parses");
    let registration = body
        .into_blocks()
        .find(|block| block.identifier() == "resource")
        .expect("registration.tf declares the registration resource");
    let hcl::Expression::Array(depends_on) = attribute(&registration, "depends_on") else {
        panic!("depends_on is a list");
    };
    depends_on
        .iter()
        .map(|address| hcl::format::to_string(address).expect("an address formats"))
        .collect()
}

#[test]
fn setup_writes_an_anchor_the_enforcers_accept() {
    let module = render(&stack(), TerraformTarget::Aws, protected());
    assert_terraform_valid(&module, "protected deployment anchor");

    let parameters = parameters(&module);
    assert_eq!(parameters.len(), 1, "one anchor");
    let (path, anchor) = &parameters[0];
    assert_eq!(path, "release_trust.tf");
    assert_eq!(anchor.labels()[1].as_str(), "release_trust_anchor");
    assert!(
        anchor
            .body()
            .attributes()
            .all(|attribute| attribute.key() != "tags"),
        "no deployment tag, so no grant conditioned on that tag reaches the anchor"
    );

    // Evaluate what Terraform writes for a module whose resource prefix is `orders-prod`.
    let mut context = Context::new();
    context.declare_var(
        "local",
        hcl::value::Value::from_iter([("resource_prefix", hcl::value::Value::from("orders-prod"))]),
    );
    let name = attribute(anchor, "name")
        .evaluate(&context)
        .expect("the name evaluates");
    assert_eq!(name, hcl::value::Value::from("/_release-trust/orders-prod"));
    assert_eq!(
        attribute(anchor, "type").evaluate(&context).unwrap(),
        hcl::value::Value::from("String")
    );
    let hcl::Expression::FuncCall(call) = attribute(anchor, "value") else {
        panic!("the value is jsonencode(...)");
    };
    assert_eq!(call.name.to_string(), "jsonencode");
    let document = call.args[0]
        .evaluate(&context)
        .expect("the anchor document evaluates");
    let written = serde_json::to_vec(&document).expect("the document serializes");
    assert_eq!(
        Anchor::parse(&written).expect("the enforcers accept the anchor"),
        Anchor {
            project: "prj_123".to_string(),
            deployment: "dep_456".to_string(),
            protected: true,
            signing: SIGNING.to_string(),
            components: components(),
            session_approval: SessionApproval::Developer,
            inputs: Some("aws-ssm:/_deployer-inputs/orders-prod".to_string()),
            resource_prefix: Some("orders-prod".to_string()),
        }
    );

    // Registration waits for the anchor, so the control plane never sees the deployment
    // before it exists.
    let depends_on = registration_depends_on(&module);
    assert!(
        depends_on.contains(&"aws_ssm_parameter.release_trust_anchor".to_string()),
        "{depends_on:?}"
    );
}

#[test]
fn an_unprotected_deployment_has_no_anchor() {
    let module = render(&stack(), TerraformTarget::Aws, StackSettings::default());
    assert!(parameters(&module).is_empty());
    assert!(!module.files.contains_key("release_trust.tf"));
}

#[test]
fn gcp_setup_writes_the_anchor_into_a_secret_only_setup_writes() {
    let module = render(&stack(), TerraformTarget::Gcp, protected());
    assert_terraform_valid(&module, "protected GCP deployment anchor");
    let blocks = release_trust_blocks(&module);
    let [secret, version] = blocks.as_slice() else {
        panic!("a secret and its version, got {}", blocks.len());
    };
    assert_eq!(secret.labels()[0].as_str(), "google_secret_manager_secret");
    assert_eq!(
        version.labels()[0].as_str(),
        "google_secret_manager_secret_version"
    );
    assert!(
        secret
            .body()
            .attributes()
            .all(|attribute| attribute.key() != "labels"),
        "no deployment label, so no grant conditioned on it reaches the anchor"
    );

    let mut context = Context::new();
    context.declare_var(
        "local",
        hcl::value::Value::from_iter([("resource_prefix", hcl::value::Value::from("orders-prod"))]),
    );
    // Every secret grant is conditioned on a name starting with the resource prefix.
    assert_eq!(
        attribute(secret, "secret_id").evaluate(&context).unwrap(),
        hcl::value::Value::from("_release-trust-orders-prod")
    );
    assert_eq!(
        hcl::format::to_string(attribute(version, "secret")).unwrap(),
        "google_secret_manager_secret.release_trust_anchor.id"
    );
    let hcl::Expression::FuncCall(call) = attribute(version, "secret_data") else {
        panic!("the value is jsonencode(...)");
    };
    let document = call.args[0]
        .evaluate(&context)
        .expect("the anchor document evaluates");
    let written = serde_json::to_vec(&document).expect("the document serializes");
    assert_eq!(
        Anchor::parse(&written).expect("the enforcers accept the anchor"),
        Anchor {
            project: "prj_123".to_string(),
            deployment: "dep_456".to_string(),
            protected: true,
            signing: SIGNING.to_string(),
            components: components(),
            session_approval: SessionApproval::Developer,
            inputs: Some("gcp-secret:_deployer-inputs-orders-prod".to_string()),
            resource_prefix: Some("orders-prod".to_string()),
        }
    );
    let depends_on = registration_depends_on(&module);
    assert!(
        depends_on
            .contains(&"google_secret_manager_secret_version.release_trust_anchor".to_string()),
        "{depends_on:?}"
    );
}

#[test]
fn kubernetes_modules_take_no_cloud_anchor() {
    for target in [
        TerraformTarget::Eks,
        TerraformTarget::Gke,
        TerraformTarget::Aks,
    ] {
        let error = try_render(&stack(), target, protected())
            .expect_err("the chart's values hold a Kubernetes deployment's anchor");
        assert_eq!(error.code, "OPERATION_NOT_SUPPORTED", "{target:?}");
    }
}

/// The resource blocks of `release_trust.tf`.
fn release_trust_blocks(module: &ModuleFiles) -> Vec<hcl::Block> {
    let contents = &module.files["release_trust.tf"];
    hcl::parse(contents)
        .expect("release_trust.tf parses")
        .into_blocks()
        .filter(|block| block.identifier() == "resource")
        .collect()
}
