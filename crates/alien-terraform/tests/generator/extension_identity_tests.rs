//! The identity setup creates for an extension: one per extension, holding its types'
//! `provision`, `management` and `heartbeat` sets, and trusted by the manager the way the remote
//! bindings identity is. The stack is the docs' DNS zone from the `acme` extension.

use super::helpers::{
    assert_terraform_valid, gate_input, normalized, prepare_for_target, render_with_registry,
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

fn stack_with_a_linking_worker() -> Stack {
    let mut stack = stack();
    let mut api = alien_core::Worker::new("api".to_string())
        .code(alien_core::WorkerCode::Image {
            image: "registry.example.com/api:1".to_string(),
        })
        .permissions("api".to_string())
        .build();
    api.links.push(alien_core::ResourceRef::new(
        alien_core::ResourceType::from(ZONE),
        "zone",
    ));
    stack.resources.insert(
        "api".to_string(),
        alien_core::ResourceEntry::new(api, ResourceLifecycle::Live),
    );
    stack
        .permissions
        .profiles
        .insert("api".to_string(), alien_core::PermissionProfile::new());
    stack
}

/// A worker that links a custom resource gets the sets the type links, on every cloud: the
/// setup template is the only writer of a workload's grants after a handoff.
#[tokio::test]
async fn terraform_grants_a_linking_worker_the_type_s_link_set() {
    for (target, permission) in [
        (TerraformTarget::Aws, "route53:ChangeResourceRecordSets"),
        (TerraformTarget::Gcp, "dns.changes.create"),
        (TerraformTarget::Azure, "Microsoft.Network/dnszones/A/write"),
    ] {
        let linked = render_stack(stack_with_a_linking_worker(), target).await;
        let unlinked = render_terraform(target).await;
        assert_eq!(
            linked.matches(permission).count(),
            unlinked.matches(permission).count() + 1,
            "{target:?}: the worker's grant of {permission}\n{linked}"
        );
    }
}

/// A Live custom resource is created after setup, when no one may widen a workload's role, so
/// setup grants the link's set by the stack-level binding the set declares.
#[tokio::test]
async fn terraform_grants_a_linking_worker_on_a_live_custom_resource() {
    for (target, permission) in [
        (TerraformTarget::Aws, "route53:ChangeResourceRecordSets"),
        (TerraformTarget::Gcp, "dns.changes.create"),
        (TerraformTarget::Azure, "Microsoft.Network/dnszones/A/write"),
    ] {
        let mut stack = stack_with_a_linking_worker();
        stack.resources.get_mut("zone").unwrap().lifecycle = ResourceLifecycle::Live;
        let linked = render_stack(stack, target).await;
        assert_eq!(
            linked.matches(permission).count(),
            1,
            "{target:?}: the worker's grant of {permission}\n{linked}"
        );
    }
}

/// A gate on a Live custom resource is checked at render, as on a Frozen one, so a caller that
/// skips preflights cannot gate the worker's grant on an input the module never declares.
#[tokio::test]
async fn terraform_refuses_a_live_custom_resource_gated_on_an_undeclared_input() {
    let mut stack = stack_with_a_linking_worker();
    stack.resources.get_mut("zone").unwrap().lifecycle = ResourceLifecycle::Live;
    let mut prepared = prepare_for_target(stack, TerraformTarget::Aws).await;
    prepared.resources.get_mut("zone").unwrap().enabled_when = Some("zoneEnabled".to_string());
    let mut registry = TfRegistry::built_in();
    registry.register(ZONE, Platform::Aws, ZoneEmitter);
    let error = render_with_registry(
        &prepared,
        TerraformTarget::Aws,
        StackSettings::default(),
        &registry,
    )
    .expect_err("an undeclared gate input is refused");
    assert!(
        error.to_string().contains("never asks the deployer for"),
        "{error}"
    );
}

/// Under automatic setup the runtime creates a workload's role after setup, and nothing then
/// grants a custom resource's sets, so both templates refuse the grant, naming the account.
#[tokio::test]
async fn automatic_setup_refuses_a_grant_on_a_custom_resource() {
    let settings = StackSettings {
        setup_management: alien_core::SetupManagement::Automatic,
        ..Default::default()
    };
    let source = stack_with_a_linking_worker();
    let mut prepared = prepare_for_target(source.clone(), TerraformTarget::Aws).await;
    // Automatic setup refuses a management grant scoped to one resource first; leave only the
    // stack-wide ones so the grant on the custom resource is what the templates meet.
    if let alien_core::ManagementPermissions::Extend(profile) = &mut prepared.permissions.management
    {
        profile.0.retain(|scope, _| scope == "*");
    }

    let mut registry = TfRegistry::built_in();
    registry.register(ZONE, Platform::Aws, ZoneEmitter);
    let error = render_with_registry(&prepared, TerraformTarget::Aws, settings.clone(), &registry)
        .expect_err("the Terraform module refuses the grant");
    let chain = format!("{error:?}");
    assert!(
        chain.contains("AUTOMATIC_SETUP_RESOURCE_UNSUPPORTED") && chain.contains("'zone'"),
        "{chain}"
    );

    let mut registry = CfRegistry::built_in();
    registry.register(ZONE, Platform::Aws, ZoneEmitter);
    let error = generate_cloudformation_template(
        &prepared,
        CloudFormationOptions {
            registry: &registry,
            source_stack: &source,
            target: CloudFormationTarget::Aws,
            stack_settings: settings,
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: Some("linked zone".to_string()),
            management_trust: Default::default(),
            pins: alien_core::SetupPins::compiled_in(),
        },
    )
    .expect_err("the CloudFormation template refuses the grant");
    let chain = format!("{error:?}");
    assert!(
        chain.contains("AUTOMATIC_SETUP_RESOURCE_UNSUPPORTED") && chain.contains("'zone'"),
        "{chain}"
    );
    assert!(!error.retryable);
}

/// The CloudFormation template grants a linking worker the type's link set too.
#[tokio::test]
async fn cloudformation_grants_a_linking_worker_the_type_s_link_set() {
    for lifecycle in [ResourceLifecycle::Frozen, ResourceLifecycle::Live] {
        cloudformation_grants_the_link_set(lifecycle).await;
    }
}

async fn cloudformation_grants_the_link_set(lifecycle: ResourceLifecycle) {
    let mut registry = CfRegistry::built_in();
    registry.register(ZONE, Platform::Aws, ZoneEmitter);
    let mut source = stack_with_a_linking_worker();
    source.resources.get_mut("zone").unwrap().lifecycle = lifecycle;
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
            description: Some("linked zone".to_string()),
            management_trust: Default::default(),
            pins: alien_core::SetupPins::compiled_in(),
        },
    )
    .unwrap_or_else(|error| panic!("the template renders: {error:?}"));
    let yaml = to_yaml(&template).unwrap();
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("linked zone");
    assert_eq!(
        yaml.matches("route53:ChangeResourceRecordSets").count(),
        1,
        "{lifecycle:?}\n{yaml}"
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
            pins: alien_core::SetupPins::compiled_in(),
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

/// On Kubernetes the Operator mints the extension's credentials from its pod, so the identity
/// trusts the Operator's own Kubernetes service account the way the management identity does, and
/// the manager not at all: no deployment there is pushed. No pod runs as the identity, so Helm
/// gets no service account for it.
#[tokio::test]
async fn kubernetes_terraform_renders_the_identity_trusted_by_the_operators_pod() {
    let operator = r#"replace("manager""#;
    for target in [
        TerraformTarget::Eks,
        TerraformTarget::Gke,
        TerraformTarget::Aks,
    ] {
        let main = render_terraform(target).await;
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
        assert!(trust.contains(operator), "{target:?}: {trust}");
        // On GKE the Operator calls Google as its own account, which mints the identity's tokens.
        if matches!(target, TerraformTarget::Gke) {
            assert!(
                trust.contains("serviceAccount:${google_service_account.")
                    && trust.contains(".email}"),
                "{trust}"
            );
        }
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
        assert!(!helm.contains("acme"), "{target:?}: {helm}");
    }
}

/// The identity has no Kubernetes service account of its own, so the IRSA name limit for a
/// profile's service account doesn't apply to it.
#[test]
fn eks_cloudformation_renders_an_identity_too_long_for_a_profile_name() {
    let mut identity = alien_core::ServiceAccount::new("contoso-backups-restorer-ext".to_string())
        .extension("contoso-backups-restorer".to_string())
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
            description: Some("eks long extension identity".to_string()),
            management_trust: Default::default(),
            pins: alien_core::SetupPins::compiled_in(),
        },
    )
    .unwrap_or_else(|error| panic!("the template renders: {error:?}"));
    let yaml = to_yaml(&template).unwrap();
    alien_cloudformation::test_utils::cfn_lint(&yaml).assert_ok("eks long extension identity");
    assert!(
        yaml.contains("${AWS::StackName}-contoso-backups-restorer-ext"),
        "{yaml}"
    );
}

#[test]
fn eks_cloudformation_renders_the_identity_trusted_by_the_operators_pod() {
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
            pins: alien_core::SetupPins::compiled_in(),
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
            && trust.contains("system:serviceaccount:alien:${AWS::StackName}-manager-sa"),
        "{trust}"
    );
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
    if std::env::var("SKIP_LINTERS").as_deref() == Ok("1") {
        return;
    }
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

/// The origins an extension's controller module may call outside the cloud show in the setup
/// template, on the identity the customer reviews the extension's grants on.
#[tokio::test]
async fn cloudformation_shows_the_origins_the_extensions_controllers_call() {
    let mut registry = CfRegistry::built_in();
    registry.register(ZONE, Platform::Aws, ZoneEmitter);
    for (egress, expected) in [
        (
            vec!["https://api.acme.example".to_string()],
            "The identity the acme extension's controllers run as. Its controllers may also call, with none of this deployment's credentials: https://api.acme.example.",
        ),
        (
            vec![],
            "The identity the acme extension's controllers run as.",
        ),
    ] {
        let mut source = stack();
        source.extensions[0].egress = egress;
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
                description: Some("extension egress".to_string()),
                management_trust: Default::default(),
                pins: alien_core::SetupPins::compiled_in(),
            },
        )
        .unwrap_or_else(|error| panic!("the template renders: {error:?}"));
        let document = serde_json::to_value(&template).unwrap();
        let role = document["Resources"]
            .as_object()
            .unwrap()
            .values()
            .find(|resource| {
                resource["Type"] == "AWS::IAM::Role"
                    && resource["Properties"]["RoleName"]
                        == json!({ "Fn::Sub": "${AWS::StackName}-acme-ext" })
            })
            .expect("the identity's role renders");
        assert_eq!(
            role["Metadata"]["ApplicationResource"]["description"], expected,
            "{role:#}"
        );
    }
}

async fn terraform_readme(stack: Stack, target: TerraformTarget) -> String {
    let mut registry = TfRegistry::built_in();
    registry.register(ZONE, target.cloud_platform(), ZoneEmitter);
    let prepared = prepare_for_target(stack, target).await;
    let module = render_with_registry(&prepared, target, StackSettings::default(), &registry)
        .unwrap_or_else(|error| panic!("{target:?} renders: {error:?}"));
    module.files["README.md"].clone()
}

/// The zone's sets without the ones its identity holds, so setup creates no identity for the
/// extension.
fn stack_with_no_extension_identity() -> Stack {
    let mut stack = stack();
    stack.extensions[0].resource_types[0]
        .permission_sets
        .retain(|set| set.id.ends_with("/records-write"));
    stack
}

/// The Terraform module's README lists each extension, the identity its controllers run as, and
/// the origins its controller module may call outside the cloud, so a customer reviewing the
/// module sees them.
#[tokio::test]
async fn terraform_readme_shows_the_origins_the_extensions_controllers_call() {
    for target in [
        TerraformTarget::Aws,
        TerraformTarget::Gcp,
        TerraformTarget::Azure,
    ] {
        let mut source = stack();
        source.extensions[0].egress = vec!["https://api.acme.example".to_string()];
        let readme = terraform_readme(source, target).await;
        assert!(
            readme.contains(
                "## Extensions\n\nThis deployment uses these extensions.\n\n- `acme` version 0.1.0. Its controllers run as `acme-ext`, with only the permissions this module grants it. Its controllers may also call, with none of this deployment's credentials: https://api.acme.example.\n"
            ),
            "{target:?}:\n{readme}"
        );
    }
}

/// An extension setup creates no identity for still shows, with the origins its controllers may
/// call.
#[tokio::test]
async fn terraform_readme_shows_an_extension_with_no_identity() {
    let mut source = stack_with_no_extension_identity();
    source.extensions[0].egress = vec!["https://api.acme.example".to_string()];
    let readme = terraform_readme(source, TerraformTarget::Aws).await;
    assert!(
        readme.contains(
            "- `acme` version 0.1.0. Setup creates no identity for it, so its controllers hold none of this deployment's cloud credentials. Its controllers may also call, with none of this deployment's credentials: https://api.acme.example.\n"
        ),
        "{readme}"
    );
    assert!(!readme.contains("acme-ext"), "{readme}");
}

#[tokio::test]
async fn terraform_readme_has_no_extensions_section_for_a_stack_without_extensions() {
    let mut source = stack();
    source.extensions.clear();
    source.resources.shift_remove("zone");
    let readme = terraform_readme(source, TerraformTarget::Aws).await;
    assert!(!readme.contains("## Extensions"), "{readme}");
}

async fn cloudformation_template(source: Stack) -> alien_cloudformation::CfTemplate {
    let mut registry = CfRegistry::built_in();
    registry.register(ZONE, Platform::Aws, ZoneEmitter);
    let prepared = prepare_for_target(source.clone(), TerraformTarget::Aws).await;
    generate_cloudformation_template(
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
            description: Some("extensions".to_string()),
            management_trust: Default::default(),
            pins: alien_core::SetupPins::compiled_in(),
        },
    )
    .unwrap_or_else(|error| panic!("the template renders: {error:?}"))
}

async fn cloudformation_document(source: Stack) -> serde_json::Value {
    let template = cloudformation_template(source).await;
    alien_cloudformation::test_utils::cfn_lint(&to_yaml(&template).unwrap())
        .assert_ok("extensions");
    serde_json::to_value(&template).unwrap()
}

/// The CloudFormation template's metadata lists each extension, the identity its controllers run
/// as when setup creates one, and the origins its controller module may call outside the cloud.
#[tokio::test]
async fn cloudformation_metadata_lists_the_stacks_extensions() {
    let egress = vec!["https://api.acme.example".to_string()];
    let mut with_identity = stack();
    with_identity.extensions[0].egress = egress.clone();
    let mut without_identity = stack_with_no_extension_identity();
    without_identity.extensions[0].egress = egress;
    for (source, expected) in [
        (
            with_identity,
            json!([{
                "name": "acme",
                "version": "0.1.0",
                "identity": "acme-ext",
                "egress": ["https://api.acme.example"],
            }]),
        ),
        (
            without_identity,
            json!([{
                "name": "acme",
                "version": "0.1.0",
                "egress": ["https://api.acme.example"],
            }]),
        ),
    ] {
        let document = cloudformation_document(source).await;
        assert_eq!(document["Metadata"]["Extensions"], expected);
    }
}

#[tokio::test]
async fn cloudformation_metadata_has_no_extensions_for_a_stack_without_extensions() {
    let mut source = stack();
    source.extensions.clear();
    source.resources.shift_remove("zone");
    let document = cloudformation_document(source).await;
    assert!(
        document["Metadata"].get("Extensions").is_none(),
        "{document:#}"
    );
}

/// A gated Live custom resource no workload links gets no grant, so the template declares no
/// condition for its gate, which nothing would use.
#[tokio::test]
async fn cloudformation_declares_no_gate_for_a_live_custom_resource_with_no_grants() {
    let mut source = stack();
    source.inputs = vec![gate_input(
        "zoneEnabled",
        "Enable the zone",
        "Whether to create the zone.",
    )];
    let zone = source.resources.get_mut("zone").unwrap();
    zone.lifecycle = ResourceLifecycle::Live;
    zone.enabled_when = Some("zoneEnabled".to_string());
    let template = cloudformation_template(source).await;
    assert!(
        template
            .conditions
            .keys()
            .all(|name| !name.to_lowercase().contains("zoneenabled")),
        "{:#?}",
        template.conditions
    );
}
