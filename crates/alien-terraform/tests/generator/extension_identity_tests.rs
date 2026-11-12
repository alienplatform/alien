//! The identity setup creates for an extension: one per extension, holding its types'
//! `provision`, `management` and `heartbeat` sets, and trusted by the manager the way the remote
//! bindings identity is. The stack is the docs' DNS zone from the `acme` extension.

use super::helpers::{
    assert_terraform_valid, normalized, prepare_for_target, render_with_registry,
};
use alien_cloudformation::{
    generate_cloudformation_template, to_yaml, CfEmitter, CfExpression, CfRegistry, CfResource,
    CloudFormationOptions, CloudFormationTarget, RegistrationMode,
};
use alien_core::{
    import::EmitContext, CustomResource, ExtensionManifest, Platform, ResourceLifecycle, Result,
    Stack, StackSettings,
};
use alien_terraform::{
    block::{attr, resource_block},
    expr, TerraformTarget, TfEmitter, TfFragment, TfRegistry,
};
use serde_json::json;

const ZONE: &str = "acme/dns-zone";

fn set(name: &str, aws: &str, gcp: &str, azure: &str) -> serde_json::Value {
    json!({
        "id": format!("{ZONE}/{name}"),
        "description": name,
        "platforms": {
            "aws": [{
                "grant": { "actions": [aws] },
                "binding": { "stack": { "resources": ["*"] }, "resource": { "resources": ["*"] } }
            }],
            "gcp": [{
                "grant": { "permissions": [gcp] },
                "binding": {
                    "stack": { "scope": "projects/${projectName}" },
                    "resource": { "scope": "projects/${projectName}" }
                }
            }],
            "azure": [{
                "grant": { "actions": [azure] },
                "binding": {
                    "stack": { "scope": "/subscriptions/${subscriptionId}/resourceGroups/${resourceGroup}" },
                    "resource": { "scope": "/subscriptions/${subscriptionId}/resourceGroups/${resourceGroup}" }
                }
            }]
        }
    })
}

fn acme() -> ExtensionManifest {
    serde_json::from_value(json!({
        "name": "acme",
        "version": "0.1.0",
        "digest": "sha256:4ff931a2",
        "resourceTypes": { ZONE: {
            "lifecycles": ["frozen", "live"],
            "holdsData": false,
            "setup": {
                "cloudformation": ["aws"],
                "terraform": ["aws", "gcp", "azure"],
                "helm": [],
                "importers": ["aws", "gcp", "azure", "kubernetes"]
            },
            "controllerPlatforms": ["aws", "gcp", "azure", "kubernetes"],
            "permissionSets": [
                set("provision", "route53:CreateHostedZone", "dns.managedZones.create", "Microsoft.Network/dnszones/write"),
                set("heartbeat", "route53:GetHostedZone", "dns.managedZones.get", "Microsoft.Network/dnszones/read"),
                set("records-write", "route53:ChangeResourceRecordSets", "dns.changes.create", "Microsoft.Network/dnszones/A/write")
            ],
            "link": { "permissionSets": [format!("{ZONE}/records-write")] },
            "schemas": { "config": { "type": "object" }, "outputs": { "type": "object" } }
        } }
    }))
    .unwrap()
}

fn stack() -> Stack {
    let zone =
        CustomResource::from_untagged(ZONE, json!({ "id": "zone", "domain": "internal.acme.dev" }))
            .unwrap();
    let mut stack = Stack::new("dns".to_string())
        .add(zone, ResourceLifecycle::Frozen)
        .build();
    stack.extensions = vec![acme()];
    stack
}

/// The zone's own emitter, which the extension's hooks module provides in a real stack.
struct ZoneEmitter;

impl TfEmitter for ZoneEmitter {
    fn emit(&self, ctx: &EmitContext<'_>) -> Result<TfFragment> {
        let label = ctx.name_for(ctx.resource_id).expect("zone label");
        let block = match ctx.platform {
            Platform::Aws => resource_block(
                "aws_route53_zone",
                label,
                [attr("name", expr::raw("\"internal.acme.dev\""))],
            ),
            Platform::Gcp => resource_block(
                "google_dns_managed_zone",
                label,
                [
                    attr("project", expr::raw("var.gcp_project")),
                    attr("name", expr::raw("\"acme-zone\"")),
                    attr("dns_name", expr::raw("\"internal.acme.dev.\"")),
                ],
            ),
            _ => resource_block(
                "azurerm_dns_zone",
                label,
                [
                    attr("name", expr::raw("\"internal.acme.dev\"")),
                    attr(
                        "resource_group_name",
                        expr::raw("var.azure_resource_group_name"),
                    ),
                ],
            ),
        };
        Ok(TfFragment::default().with_resource(block))
    }

    fn emit_import_ref(&self, _ctx: &EmitContext<'_>) -> Result<hcl::Expression> {
        Ok(expr::object([("zoneId", expr::raw("\"zone\""))]))
    }
}

impl CfEmitter for ZoneEmitter {
    fn emit_resources(&self, ctx: &EmitContext<'_>) -> Result<Vec<CfResource>> {
        let mut zone = CfResource::new(
            ctx.name_for(ctx.resource_id)
                .expect("zone logical id")
                .to_string(),
            "AWS::Route53::HostedZone".to_string(),
        );
        zone.properties
            .insert("Name".to_string(), CfExpression::from("internal.acme.dev"));
        Ok(vec![zone])
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<CfExpression> {
        Ok(CfExpression::object([(
            "zoneId",
            CfExpression::ref_(ctx.name_for(ctx.resource_id).expect("zone logical id")),
        )]))
    }
}

async fn render_terraform(target: TerraformTarget) -> String {
    render_stack(stack(), target).await
}

async fn render_stack(stack: Stack, target: TerraformTarget) -> String {
    let mut registry = TfRegistry::built_in();
    registry.register(ZONE, target.cloud_platform(), ZoneEmitter);
    let prepared = prepare_for_target(stack, target).await;
    let module = render_with_registry(&prepared, target, StackSettings::default(), &registry)
        .unwrap_or_else(|error| panic!("{target:?} renders: {error:?}"));
    assert_terraform_valid(&module, &format!("{target:?} extension identity"));
    normalized(&module)
}

/// One HCL block of `main`, from its header to the next block's.
fn block<'a>(main: &'a str, header: &str) -> &'a str {
    let start = main
        .find(header)
        .unwrap_or_else(|| panic!("{header} renders:\n{main}"));
    let rest = &main[start + header.len()..];
    let end = rest.find(" resource \"").unwrap_or(rest.len());
    &main[start..start + header.len() + end]
}

#[tokio::test]
async fn aws_terraform_renders_the_identity_trusted_by_the_managing_role() {
    let main = render_terraform(TerraformTarget::Aws).await;
    let role = block(&main, r#"resource "aws_iam_role" "acme_ext""#);
    assert!(
        role.contains(r#"Principal = { AWS = var.managing_role_arn }"#)
            && role.contains(r#"merge({ "aws:PrincipalArn" = var.managing_role_arn }, var.management_external_id == "" ? {} : { "sts:ExternalId" = var.management_external_id })"#),
        "{role}"
    );
    assert!(!role.contains("lambda.amazonaws.com"), "{role}");
    for action in ["route53:CreateHostedZone", "route53:GetHostedZone"] {
        assert_eq!(main.matches(action).count(), 1, "{action}:\n{main}");
    }
    // The link's set belongs to a workload that links the zone, never to the identity.
    assert!(!main.contains("route53:ChangeResourceRecordSets"), "{main}");
}

#[tokio::test]
async fn gcp_terraform_renders_the_identity_trusted_by_the_federated_manager() {
    let main = render_terraform(TerraformTarget::Gcp).await;
    let account = block(&main, r#"resource "google_service_account" "acme_ext""#);
    assert!(
        account.contains(r#"display_name = "${local.deployment_name}: Extension identity (acme)""#),
        "{account}"
    );
    let trust = block(
        &main,
        r#"resource "google_service_account_iam_member" "acme_ext_manager""#,
    );
    assert!(
        trust.contains("service_account_id = google_service_account.acme_ext.id")
            && trust.contains(r#"role = "roles/iam.workloadIdentityUser""#)
            && trust.contains(
                "principal://iam.googleapis.com/${google_iam_workload_identity_pool.management_manager.name}/subject/${var.managing_oidc_subject}"
            ),
        "{trust}"
    );
    for permission in ["dns.managedZones.create", "dns.managedZones.get"] {
        assert!(main.contains(permission), "{permission}:\n{main}");
    }
    assert!(!main.contains("dns.changes.create"), "{main}");
}

#[tokio::test]
async fn azure_terraform_renders_the_identity_trusted_by_the_managers_issuer() {
    let main = render_terraform(TerraformTarget::Azure).await;
    block(
        &main,
        r#"resource "azurerm_user_assigned_identity" "acme_ext""#,
    );
    let trust = block(
        &main,
        r#"resource "azurerm_federated_identity_credential" "acme_ext_fic""#,
    );
    assert!(
        trust.contains("parent_id = azurerm_user_assigned_identity.acme_ext.id")
            && trust.contains("issuer = var.azure_oidc_issuer")
            && trust.contains("subject = var.azure_oidc_subject"),
        "{trust}"
    );
    for action in [
        "Microsoft.Network/dnszones/write",
        "Microsoft.Network/dnszones/read",
    ] {
        assert!(main.contains(action), "{action}:\n{main}");
    }
    assert!(
        !main.contains("Microsoft.Network/dnszones/A/write"),
        "{main}"
    );
}

#[tokio::test]
async fn cloudformation_renders_the_identity_trusted_by_the_managing_role() {
    let mut registry = CfRegistry::built_in();
    registry.register(ZONE, Platform::Aws, ZoneEmitter);
    let source = stack();
    let prepared = prepare_for_target(source.clone(), TerraformTarget::Aws).await;
    let template = generate_cloudformation_template(
        &prepared,
        CloudFormationOptions {
            registry: &registry,
            source_stack: &source,
            target: CloudFormationTarget::Aws,
            stack_settings: StackSettings::default(),
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: Some("extension identity".to_string()),
            management_trust: Default::default(),
        },
    )
    .unwrap_or_else(|error| panic!("the template renders: {error:?}"));
    let yaml = to_yaml(&template).unwrap();
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("extension identity");

    let document = serde_json::to_value(&template).unwrap();
    let resources = document["Resources"].as_object().unwrap();
    let (_, role) = resources
        .iter()
        .find(|(_, resource)| {
            resource["Type"] == "AWS::IAM::Role"
                && resource["Properties"]["RoleName"]
                    == json!({ "Fn::Sub": "${AWS::StackName}-acme-ext" })
        })
        .unwrap_or_else(|| panic!("the identity's role renders:\n{yaml}"));
    let trust = &role["Properties"]["AssumeRolePolicyDocument"]["Statement"];
    assert_eq!(trust.as_array().unwrap().len(), 1, "{trust}");
    assert_eq!(
        trust[0]["Principal"]["AWS"],
        json!({ "Ref": "ManagingRoleArn" })
    );
    assert_eq!(
        trust[0]["Condition"]["StringEquals"]["Fn::If"][0], "HasManagementExternalId",
        "{trust}"
    );
    let policy = role["Properties"]["Policies"].to_string();
    for action in ["route53:CreateHostedZone", "route53:GetHostedZone"] {
        assert!(policy.contains(action), "{action}: {policy}");
        // No other role in the template holds it, the management role included.
        assert_eq!(yaml.matches(action).count(), 1, "{action}:\n{yaml}");
    }
    assert!(!yaml.contains("route53:ChangeResourceRecordSets"), "{yaml}");
}

/// On Kubernetes an extension's controller runs in a pod of its own, so the identity trusts
/// that pod's Kubernetes service account and nothing else: not the Operator's account, and not
/// the manager, which pushes no deployment there. Helm gets the account with the identity's
/// annotation, as it does for a workload's.
#[tokio::test]
async fn kubernetes_terraform_renders_the_identity_trusted_by_the_controllers_pod() {
    let controller = r#"replace("acme-ext""#;
    let operator = r#"replace("manager""#;
    // Who the identity trusts does not depend on what it may do, so its one set is one the
    // extension catalog takes on every target.
    let mut stack = stack();
    stack.extensions[0].resource_types[0].permission_sets = vec![serde_json::from_value(json!({
        "id": format!("{ZONE}/provision"),
        "description": "provision",
        "platforms": { "aws": [{
            "grant": { "actions": ["route53:CreateHostedZone"] },
            "binding": { "stack": { "resources": ["*"] }, "resource": { "resources": ["*"] } }
        }] }
    }))
    .unwrap()];
    stack.extensions[0].resource_types[0].link.permission_sets = Vec::new();
    for (target, annotation) in [
        (TerraformTarget::Eks, "eks.amazonaws.com/role-arn"),
        (TerraformTarget::Gke, "iam.gke.io/gcp-service-account"),
        (TerraformTarget::Aks, "azure.workload.identity/client-id"),
    ] {
        let main = render_stack(stack.clone(), target).await;
        let trust = match target {
            TerraformTarget::Eks => {
                let role = block(&main, r#"resource "aws_iam_role" "acme_ext""#);
                assert!(
                    role.contains(
                        "assume_role_policy = data.aws_iam_policy_document.acme_ext_assume_role.json"
                    ),
                    "{role}"
                );
                main.split(r#"data "aws_iam_policy_document" "acme_ext_assume_role""#)
                    .nth(1)
                    .unwrap_or_else(|| panic!("the pod trust renders:\n{main}"))
                    .split(" data \"")
                    .next()
                    .unwrap()
                    .to_string()
            }
            TerraformTarget::Gke => block(
                &main,
                r#"resource "google_service_account_iam_binding" "acme_ext_workload_identity""#,
            )
            .to_string(),
            _ => block(
                &main,
                r#"resource "azurerm_federated_identity_credential" "acme_ext_federated""#,
            )
            .to_string(),
        };
        assert!(trust.contains(controller), "{target:?}: {trust}");
        assert!(!trust.contains(operator), "{target:?}: {trust}");
        for manager_trust in [
            "acme_ext_manager",
            "acme_ext_fic",
            "var.managing_role_arn }, var.management_external_id",
        ] {
            assert!(
                !main
                    .split("resource \"")
                    .filter(|resource| resource.contains("acme_ext"))
                    .any(|resource| resource.contains(manager_trust)),
                "{target:?} keeps {manager_trust}:\n{main}"
            );
        }
        let helm = main
            .split("helm_service_accounts = ")
            .nth(1)
            .unwrap_or_else(|| panic!("{target:?} renders the Helm values:\n{main}"));
        let helm = &helm[..helm.find(" helm_").unwrap_or(helm.len())];
        assert!(
            helm.contains("acme-ext") && helm.contains(annotation),
            "{target:?}: {helm}"
        );
    }
}

/// The identity's Kubernetes service account is named `<stack>-<extension>-ext-sa`, which
/// CloudFormation can't cut to a DNS label the way the chart does. An extension whose name
/// would make the two differ is refused.
#[test]
fn eks_cloudformation_refuses_an_identity_whose_service_account_name_would_be_cut() {
    let mut identity =
        alien_core::ServiceAccount::new("contoso-backup-and-restore-ext".to_string())
            .extension("contoso-backup-and-restore".to_string())
            .build();
    identity.stack_permission_sets = vec![acme().resource_types[0].permission_sets[0].clone()];
    let stack = Stack::new("dns".to_string())
        .add(
            alien_core::KubernetesCluster::new("kubernetes".to_string())
                .provider(alien_core::KubernetesClusterProvider::Eks)
                .ownership(alien_core::KubernetesClusterOwnership::Managed)
                .namespace("alien".to_string())
                .heartbeat_mode(alien_core::KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            alien_core::RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(identity, ResourceLifecycle::Frozen)
        .build();
    let registry = CfRegistry::built_in();
    let error = generate_cloudformation_template(
        &stack,
        CloudFormationOptions {
            registry: &registry,
            source_stack: &stack,
            target: CloudFormationTarget::Eks,
            stack_settings: StackSettings::default(),
            setup_target: "kubernetes".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: Some("eks long extension identity".to_string()),
            management_trust: Default::default(),
        },
    )
    .expect_err("the service account name would be cut");
    assert!(
        error.message.contains(
            "extension 'contoso-backup-and-restore' needs a name of at most 20 characters"
        ),
        "{error}"
    );
}

#[test]
fn eks_cloudformation_renders_the_identity_trusted_by_the_controllers_pod() {
    let mut registry = CfRegistry::built_in();
    registry.register(ZONE, Platform::Aws, ZoneEmitter);
    let provision = acme().resource_types[0].permission_sets[0].clone();
    let mut identity = alien_core::ServiceAccount::new("acme-ext".to_string())
        .extension("acme".to_string())
        .build();
    identity.stack_permission_sets = vec![provision];
    let mut stack = Stack::new("dns".to_string())
        .add(
            alien_core::KubernetesCluster::new("kubernetes".to_string())
                .provider(alien_core::KubernetesClusterProvider::Eks)
                .ownership(alien_core::KubernetesClusterOwnership::Managed)
                .namespace("alien".to_string())
                .heartbeat_mode(alien_core::KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
                .build(),
            ResourceLifecycle::Frozen,
        )
        .add(
            alien_core::RemoteStackManagement::new("management".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .add(identity, ResourceLifecycle::Frozen)
        .build();
    stack.resources.insert(
        "zone".to_string(),
        alien_core::ResourceEntry::new(
            CustomResource::from_untagged(ZONE, json!({ "id": "zone" })).unwrap(),
            ResourceLifecycle::Frozen,
        ),
    );
    stack.extensions = vec![acme()];
    let template = generate_cloudformation_template(
        &stack,
        CloudFormationOptions {
            registry: &registry,
            source_stack: &stack,
            target: CloudFormationTarget::Eks,
            stack_settings: StackSettings::default(),
            setup_target: "kubernetes".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: Some("eks extension identity".to_string()),
            management_trust: Default::default(),
        },
    )
    .unwrap_or_else(|error| panic!("the template renders: {error:?}"));
    let yaml = to_yaml(&template).unwrap();
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("eks extension identity");
    let document = serde_json::to_value(&template).unwrap();
    let trust = document["Resources"]
        .as_object()
        .unwrap()
        .values()
        .find(|resource| {
            resource["Properties"]["RoleName"] == json!({ "Fn::Sub": "${AWS::StackName}-acme-ext" })
        })
        .unwrap_or_else(|| panic!("the identity's role renders:\n{yaml}"))["Properties"]
        ["AssumeRolePolicyDocument"]
        .to_string();
    assert!(
        trust.contains("sts:AssumeRoleWithWebIdentity")
            && trust.contains("system:serviceaccount:alien:${AWS::StackName}-acme-ext-sa"),
        "{trust}"
    );
    assert!(!trust.contains("manager-sa"), "{trust}");
    assert!(!trust.contains("AllowManagingRole"), "{trust}");
}

/// What `terraform console` makes of `expression` with `local.resource_prefix` set.
fn terraform_value(resource_prefix: &str, expression: &hcl::Expression) -> String {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("main.tf"),
        format!("locals {{\n  resource_prefix = \"{resource_prefix}\"\n}}\n"),
    )
    .unwrap();
    let mut console = std::process::Command::new("terraform")
        .arg("console")
        .current_dir(dir.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("terraform runs");
    use std::io::Write;
    writeln!(console.stdin.take().unwrap(), "{expression}").unwrap();
    let output = console.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    serde_json::from_str(String::from_utf8(output.stdout).unwrap().trim()).unwrap()
}

/// Direct setup names the identity's AWS role and GCP service account the way the Terraform
/// template does, including where a long name has to be cut to the cloud's limit.
#[test]
fn direct_setup_names_the_identity_as_the_template_does() {
    use alien_terraform::emitters::{aws, gcp};
    for (prefix, id) in [
        ("nd3ef88e", "acme-ext"),
        (
            "customer-provided-resource-prefix",
            "contoso-backup-and-restore-for-postgres-ext",
        ),
    ] {
        assert_eq!(
            terraform_value(prefix, &aws::helpers::iam_role_name_template(id)),
            alien_core::extension_identity_aws_role_name(prefix, id),
            "{id}"
        );
        let label = id.replace('-', "_");
        assert_eq!(
            terraform_value(prefix, &gcp::helpers::service_account_id_template(&label)),
            alien_core::extension_identity_gcp_account_id(prefix, id),
            "{id}"
        );
    }
    assert!(
        alien_core::extension_identity_aws_role_name(
            "customer-provided-resource-prefix",
            "contoso-backup-and-restore-for-postgres-ext"
        )
        .len()
            == 64
    );
}

/// Writes the Terraform module setup renders for one extension's identity, for a live check of
/// minting credentials for it. `ALIEN_LIVE_MINT_TARGET` is `aws`, `gcp` or `azure`, and the
/// module is written under `ALIEN_LIVE_MINT_DIR`. The zone is live, so the module creates only
/// the management identity, the extension's identity, and what they need.
#[tokio::test]
#[ignore = "writes a module for a live mint; set ALIEN_LIVE_MINT_TARGET and ALIEN_LIVE_MINT_DIR"]
async fn write_the_extension_identity_module_for_a_live_mint() {
    let target = match std::env::var("ALIEN_LIVE_MINT_TARGET").as_deref() {
        Ok("aws") => TerraformTarget::Aws,
        Ok("gcp") => TerraformTarget::Gcp,
        Ok("azure") => TerraformTarget::Azure,
        other => panic!("ALIEN_LIVE_MINT_TARGET is aws, gcp or azure, not {other:?}"),
    };
    let dir = std::path::PathBuf::from(std::env::var("ALIEN_LIVE_MINT_DIR").unwrap());
    let zone =
        CustomResource::from_untagged(ZONE, json!({ "id": "zone", "domain": "internal.acme.dev" }))
            .unwrap();
    let mut stack = Stack::new("dns".to_string())
        .add(zone, ResourceLifecycle::Live)
        .build();
    stack.extensions = vec![acme()];
    let mut registry = TfRegistry::built_in();
    registry.register(ZONE, target.cloud_platform(), ZoneEmitter);
    let prepared = prepare_for_target(stack, target).await;
    let module = render_with_registry(&prepared, target, StackSettings::default(), &registry)
        .unwrap_or_else(|error| panic!("{target:?} renders: {error:?}"));
    for (path, contents) in module.iter() {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, contents).unwrap();
    }
}
