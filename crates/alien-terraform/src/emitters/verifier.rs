//! A protected install's verifier, `<prefix>-verifier`, on AWS, GCP and Azure.
//!
//! Setup creates it and only setup changes it. Its image is a literal from the CLI build
//! ([`alien_core::SetupPins`]), so the module's digest covers it. Its environment names where
//! setup lets it run what it writes: the network functions join, the network machines join,
//! and on Azure the Container Apps environment of the install's workloads.

use crate::{
    block::{attr, block, nested, resource_block},
    emitter::{TfEmitter, TfFragment},
    emitters::azure::helpers as azure_helpers,
    expr,
    generator::escaped_template,
    registry::TfRegistry,
};
use alien_core::{
    import::EmitContext, AzureContainerAppsEnvironment, ComputeCluster, ErrorData, Network,
    NetworkSettings, Platform, RemoteStackManagement, Result, ServiceAccount, SigningVerifier,
    AWS_LOG_RETENTION_DAYS,
};
use alien_error::AlienError;
use alien_permissions::generators::signing_limits;
use hcl::{Block, Expression};

/// The Terraform label of every verifier block, as the module has always named them.
const LABEL: &str = "signing_verifier";

/// The label of the stack's management resource, if any.
fn management_label<'a>(ctx: &EmitContext<'a>) -> Option<&'a str> {
    ctx.stack
        .resources()
        .find(|(_, entry)| entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE)
        .map(|(id, _)| ctx.name_for(id).unwrap_or(id.as_str()))
}

/// Where setup lets the verifier run what it writes, for `ctx`'s stack.
fn networks(ctx: &EmitContext<'_>, registry: &TfRegistry) -> Result<VerifierNetworks> {
    Ok(VerifierNetworks {
        functions: verifier_function_network(ctx),
        machines: verifier_machine_network(ctx, registry)?,
        environment: verifier_workload_environment(ctx)?,
    })
}

fn verifier_config(ctx: &EmitContext<'_>) -> Result<()> {
    ctx.resource
        .config
        .downcast_ref::<SigningVerifier>()
        .map(|_| ())
        .ok_or_else(|| {
            AlienError::new(ErrorData::UnexpectedResourceType {
                resource_id: ctx.resource_id.to_string(),
                expected: SigningVerifier::RESOURCE_TYPE,
                actual: ctx.resource.config.resource_type(),
            })
        })
}

/// The verifier renders with the registry: on GCP and Azure each compute cluster's emitter
/// names the machines' network.
fn needs_registry() -> AlienError<ErrorData> {
    AlienError::new(ErrorData::OperationNotSupported {
        operation: "render the verifier".to_string(),
        reason: "the verifier renders only with the registry that names machine networks"
            .to_string(),
    })
}

fn fragment(blocks: Vec<Block>) -> TfFragment {
    let mut fragment = TfFragment::default();
    fragment.resource_blocks = blocks;
    fragment
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AwsVerifierEmitter;

impl TfEmitter for AwsVerifierEmitter {
    fn emit(&self, _ctx: &EmitContext<'_>) -> Result<TfFragment> {
        Err(needs_registry())
    }

    fn emit_with_registry(
        &self,
        ctx: &EmitContext<'_>,
        registry: &TfRegistry,
    ) -> Result<TfFragment> {
        verifier_config(ctx)?;
        Ok(fragment(aws_verifier_blocks(
            ctx.pins,
            networks(ctx, registry)?,
        )?))
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        verifier_config(ctx)?;
        Ok(expr::object([(
            "name",
            expr::traversal(["aws_lambda_function", LABEL, "function_name"]),
        )]))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GcpVerifierEmitter;

impl TfEmitter for GcpVerifierEmitter {
    fn emit(&self, _ctx: &EmitContext<'_>) -> Result<TfFragment> {
        Err(needs_registry())
    }

    fn emit_with_registry(
        &self,
        ctx: &EmitContext<'_>,
        registry: &TfRegistry,
    ) -> Result<TfFragment> {
        verifier_config(ctx)?;
        // The runtime service accounts the module creates, by Terraform label; never
        // management's.
        let runtime_service_accounts: Vec<String> = ctx
            .stack
            .resources()
            .filter(|(_, entry)| entry.config.resource_type() == ServiceAccount::RESOURCE_TYPE)
            .filter_map(|(id, _)| ctx.name_for(id).map(str::to_string))
            .collect();
        Ok(fragment(gcp_verifier_blocks(
            ctx.pins,
            management_label(ctx),
            &runtime_service_accounts,
            networks(ctx, registry)?,
        )?))
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        verifier_config(ctx)?;
        Ok(expr::object([(
            "name",
            expr::traversal(["google_cloud_run_v2_service", LABEL, "name"]),
        )]))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AzureVerifierEmitter;

impl TfEmitter for AzureVerifierEmitter {
    fn emit(&self, _ctx: &EmitContext<'_>) -> Result<TfFragment> {
        Err(needs_registry())
    }

    fn emit_with_registry(
        &self,
        ctx: &EmitContext<'_>,
        registry: &TfRegistry,
    ) -> Result<TfFragment> {
        verifier_config(ctx)?;
        Ok(fragment(azure_verifier_blocks(
            ctx.pins,
            management_label(ctx),
            networks(ctx, registry)?,
        )?))
    }

    fn emit_import_ref(&self, ctx: &EmitContext<'_>) -> Result<Expression> {
        verifier_config(ctx)?;
        Ok(expr::object([(
            "name",
            expr::traversal(["azurerm_container_app", LABEL, "name"]),
        )]))
    }
}

/// A protected AWS install's verifier, `<prefix>-verifier`: the same as the CloudFormation
/// template's.
fn aws_verifier_blocks(
    pins: &alien_core::SetupPins,
    networks: VerifierNetworks,
) -> Result<Vec<Block>> {
    let missing = |what: &str| {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: "render a protected AWS setup".to_string(),
            reason: format!(
                "this CLI build has no {what}; protected setups come from released CLI builds"
            ),
        })
    };
    let publisher = pins
        .aws_publisher
        .as_deref()
        .ok_or_else(|| missing("machine-image publisher"))?;
    let image = pins
        .aws_verifier_image
        .as_deref()
        .filter(|image| alien_core::is_pinned_image(image))
        .ok_or_else(|| missing("verifier image pinned by digest"))?;
    let prefix = "${local.resource_prefix}";
    let account = "${data.aws_caller_identity.current.account_id}";
    let region = "${data.aws_region.current.region}";
    let verifier = signing_limits::aws_verifier_name(prefix);
    let setup_tags = || {
        expr::object([
            ("managed-by", Expression::String("setup".to_string())),
            ("Name", expr::template(verifier.clone())),
        ])
    };
    let lambda_trust = serde_json::json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Principal": { "Service": "lambda.amazonaws.com" },
            "Action": "sts:AssumeRole"
        }]
    });
    let blocks = vec![
        resource_block(
            "aws_iam_role",
            "signing_verifier",
            [
                attr("name", expr::template(verifier.clone())),
                attr(
                    "description",
                    Expression::String("The verifier's role. Only setup changes it.".to_string()),
                ),
                attr(
                    "assume_role_policy",
                    crate::emitters::aws::helpers::jsonencode_policy_value(lambda_trust),
                ),
                attr("tags", setup_tags()),
            ],
        ),
        resource_block(
            "aws_iam_role_policy",
            "signing_verifier",
            [
                attr("name", Expression::String("verifier".to_string())),
                attr(
                    "role",
                    expr::traversal(["aws_iam_role", "signing_verifier", "id"]),
                ),
                attr(
                    "policy",
                    crate::emitters::aws::helpers::jsonencode_policy_value(
                        signing_limits::aws_verifier_policy(prefix, account, region, publisher),
                    ),
                ),
            ],
        ),
        // Lambda would otherwise create `/aws/lambda/<function name>` on the first call, outside
        // the module, and a destroy would leave it behind.
        resource_block(
            "aws_cloudwatch_log_group",
            "signing_verifier",
            [
                attr(
                    "name",
                    expr::template(format!("/aws/lambda/{verifier}")),
                ),
                attr(
                    "retention_in_days",
                    Expression::Number(hcl::Number::from(AWS_LOG_RETENTION_DAYS)),
                ),
                attr("tags", setup_tags()),
            ],
        ),
        resource_block(
            "aws_lambda_function",
            "signing_verifier",
            [
                attr("function_name", expr::template(verifier.clone())),
                attr(
                    "description",
                    Expression::String(
                        "Applies only releases the vendor signed. The management role can't change what runs here."
                            .to_string(),
                    ),
                ),
                attr("package_type", Expression::String("Image".to_string())),
                attr(
                    "image_uri",
                    expr::template(format!(
                        "{publisher}.dkr.ecr.${{data.aws_region.current.region}}.amazonaws.com/{image}"
                    )),
                ),
                attr(
                    "role",
                    expr::traversal(["aws_iam_role", "signing_verifier", "arn"]),
                ),
                attr("timeout", Expression::Number(300.into())),
                attr("memory_size", Expression::Number(512.into())),
                // The verifier image is built for x86_64; say so rather than lean on Lambda's
                // default.
                attr(
                    "architectures",
                    Expression::Array(vec![Expression::String("x86_64".to_string())]),
                ),
                nested(block(
                    "environment",
                    [attr(
                        "variables",
                        expr::object(
                            [("VERIFIER_CLOUD", Expression::String("aws".to_string()))]
                                .into_iter()
                                .chain(networks.env()),
                        ),
                    )],
                )),
                attr("tags", setup_tags()),
                attr(
                    "depends_on",
                    Expression::Array(vec![
                        expr::traversal(["aws_iam_role_policy", "signing_verifier"]),
                        expr::traversal(["aws_cloudwatch_log_group", "signing_verifier"]),
                    ]),
                ),
            ],
        ),
    ];
    Ok(blocks)
}

/// A protected Azure install's verifier: the Container App `<prefix>-verifier` in a managed
/// environment of its own, running as the verifier identity the management emitter creates
/// (the deny's second excluded principal) and reading the signing vault. The management roles
/// hold no Container App writes on a protected install, so only setup changes its image and
/// environment.
///
/// Its ingress is public, behind Container Apps authentication: only the management identity's
/// token gets a request through to it.
fn azure_verifier_blocks(
    pins: &alien_core::SetupPins,
    management_label: Option<&str>,
    networks: VerifierNetworks,
) -> Result<Vec<Block>> {
    let image = pins
        .azure_verifier_image
        .as_deref()
        .filter(|image| alien_core::is_pinned_image(image))
        .ok_or_else(|| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "render a protected Azure setup".to_string(),
                reason: "this CLI build has no Azure verifier image pinned by digest; protected setups come from released CLI builds".to_string(),
            })
        })?;
    let management = management_label.ok_or_else(|| {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: "render a protected Azure setup".to_string(),
            reason: "the stack has no remote management, which creates the verifier identity"
                .to_string(),
        })
    })?;
    let identity = format!("{management}_verifier");
    let vault = format!("{management}_signing");
    let name = expr::template("${local.resource_prefix}-verifier".to_string());
    let setup_tags = || expr::object([("managed-by", Expression::String("setup".to_string()))]);
    let env = |name: &str, value: Expression| {
        nested(block(
            "env",
            [
                attr("name", Expression::String(name.to_string())),
                attr("value", value),
            ],
        ))
    };
    Ok(vec![
        resource_block(
            "azurerm_container_app_environment",
            "signing_verifier",
            [
                attr("name", name.clone()),
                attr("resource_group_name", expr::raw("var.azure_resource_group_name")),
                attr("location", expr::raw("var.azure_location")),
                attr("tags", setup_tags()),
            ],
        ),
        resource_block(
            "azurerm_container_app",
            "signing_verifier",
            [
                attr("name", name),
                attr("resource_group_name", expr::raw("var.azure_resource_group_name")),
                attr(
                    "container_app_environment_id",
                    expr::traversal(["azurerm_container_app_environment", "signing_verifier", "id"]),
                ),
                attr("revision_mode", Expression::String("Single".to_string())),
                nested(block(
                    "identity",
                    [
                        attr("type", Expression::String("UserAssigned".to_string())),
                        attr(
                            "identity_ids",
                            Expression::Array(vec![expr::traversal([
                                "azurerm_user_assigned_identity",
                                &identity,
                                "id",
                            ])]),
                        ),
                    ],
                )),
                nested(block(
                    "template",
                    [
                        attr("min_replicas", Expression::Number(0.into())),
                        attr("max_replicas", Expression::Number(1.into())),
                        nested(block(
                            "container",
                            [
                                attr("name", Expression::String("verifier".to_string())),
                                attr("image", Expression::String(image.to_string())),
                                attr("cpu", expr::raw("0.25")),
                                attr("memory", Expression::String("0.5Gi".to_string())),
                                env("VERIFIER_CLOUD", Expression::String("azure".to_string())),
                                env(
                                    "VERIFIER_AZURE_RESOURCE_GROUP",
                                    expr::template(
                                        "/subscriptions/${var.azure_subscription_id}/resourceGroups/${var.azure_resource_group_name}"
                                            .to_string(),
                                    ),
                                ),
                                env(
                                    "VERIFIER_AZURE_VAULT",
                                    expr::traversal(["azurerm_key_vault", &vault, "vault_uri"]),
                                ),
                                env(
                                    "VERIFIER_AZURE_CLIENT_ID",
                                    expr::traversal([
                                        "azurerm_user_assigned_identity",
                                        &identity,
                                        "client_id",
                                    ]),
                                ),
                            ]
                            .into_iter()
                            .chain(networks.env().map(|(name, value)| env(name, value))),
                        )),
                    ],
                )),
                nested(block(
                    "ingress",
                    [
                        attr("external_enabled", Expression::Bool(true)),
                        attr("target_port", Expression::Number(8080.into())),
                        attr("transport", Expression::String("auto".to_string())),
                        nested(block(
                            "traffic_weight",
                            [
                                attr("latest_revision", Expression::Bool(true)),
                                attr("percentage", Expression::Number(100.into())),
                            ],
                        )),
                    ],
                )),
                attr("tags", setup_tags()),
                attr(
                    "depends_on",
                    Expression::Array(vec![
                        expr::traversal(["azurerm_role_assignment", &identity]),
                        expr::traversal(["azurerm_role_assignment", &format!("{vault}_verifier")]),
                    ]),
                ),
            ],
        ),
        // Container Apps authentication answers every request before the verifier sees it: 401
        // without a token, 403 with a token of anyone but the management identity. Management
        // presents its token for Azure Resource Manager, which every credential it has can
        // mint, so no app registration is needed.
        resource_block(
            "azapi_resource",
            "signing_verifier_auth",
            [
                attr(
                    "type",
                    Expression::String(format!("{AUTH_CONFIG_TYPE}@{AUTH_CONFIG_API_VERSION}")),
                ),
                attr("name", Expression::String("current".to_string())),
                attr(
                    "parent_id",
                    expr::traversal(["azurerm_container_app", LABEL, "id"]),
                ),
                attr(
                    "body",
                    expr::object([(
                        "properties",
                        expr::object([
                            (
                                "platform",
                                expr::object([("enabled", Expression::Bool(true))]),
                            ),
                            (
                                "globalValidation",
                                expr::object([(
                                    "unauthenticatedClientAction",
                                    Expression::String("Return401".to_string()),
                                )]),
                            ),
                            (
                                "identityProviders",
                                expr::object([(
                                    "azureActiveDirectory",
                                    expr::object([
                                        ("enabled", Expression::Bool(true)),
                                        (
                                            "registration",
                                            expr::object([
                                                (
                                                    "clientId",
                                                    expr::traversal([
                                                        "azurerm_user_assigned_identity",
                                                        management,
                                                        "client_id",
                                                    ]),
                                                ),
                                                (
                                                    "openIdIssuer",
                                                    expr::template(format!(
                                                        "https://sts.windows.net/${{data.azurerm_client_config.{management}_current.tenant_id}}/"
                                                    )),
                                                ),
                                            ]),
                                        ),
                                        (
                                            "validation",
                                            expr::object([
                                                (
                                                    "allowedAudiences",
                                                    Expression::Array(
                                                        MANAGEMENT_TOKEN_AUDIENCES
                                                            .iter()
                                                            .map(|audience| {
                                                                Expression::String(
                                                                    audience.to_string(),
                                                                )
                                                            })
                                                            .collect(),
                                                    ),
                                                ),
                                                (
                                                    "defaultAuthorizationPolicy",
                                                    expr::object([(
                                                        "allowedPrincipals",
                                                        expr::object([(
                                                            "identities",
                                                            Expression::Array(vec![
                                                                expr::traversal([
                                                                    "azurerm_user_assigned_identity",
                                                                    management,
                                                                    "principal_id",
                                                                ]),
                                                            ]),
                                                        )]),
                                                    )]),
                                                ),
                                            ]),
                                        ),
                                    ]),
                                )]),
                            ),
                        ]),
                    )]),
                ),
            ],
        ),
    ])
}

/// The Container Apps authentication resource on the verifier.
const AUTH_CONFIG_TYPE: &str = "Microsoft.App/containerApps/authConfigs";
const AUTH_CONFIG_API_VERSION: &str = "2024-03-01";

/// The audience of the management identity's token the verifier accepts: Azure Resource
/// Manager's, with and without the trailing slash Azure issues either way.
const MANAGEMENT_TOKEN_AUDIENCES: &[&str] = &[
    "https://management.azure.com",
    "https://management.azure.com/",
];

/// What the GCP verifier may change on the install's own machines: their templates and groups,
/// and the machines and disks those create (byoc review §1). Bound under a condition on the
/// resources' names.
const GCP_VERIFIER_OWN_RESOURCE_PERMISSIONS: &[&str] = &[
    "compute.instanceTemplates.create",
    "compute.instanceTemplates.delete",
    "compute.instanceTemplates.get",
    "compute.instanceTemplates.useReadOnly",
    "compute.instanceGroupManagers.create",
    "compute.instanceGroupManagers.update",
    "compute.instanceGroupManagers.delete",
    "compute.instanceGroupManagers.get",
    "compute.instances.create",
    "compute.instances.setMetadata",
    "compute.instances.setServiceAccount",
    "compute.instances.delete",
    "compute.disks.create",
];

/// What the GCP verifier may do with Cloud Run services and jobs. Bound without a condition:
/// Cloud Run doesn't evaluate `resource.name` in IAM conditions (a binding conditioned on the
/// service's name denies even a get of a service it names), and it checks a create against the
/// parent location. The verifier only ever writes `<prefix>-<workload>`; management holds none of
/// these on a protected install.
const GCP_VERIFIER_CLOUD_RUN_PERMISSIONS: &[&str] = &[
    "run.services.create",
    "run.services.update",
    "run.services.delete",
    "run.services.get",
    // A service it creates for a private worker admits the worker's own account as invoker.
    "run.services.getIamPolicy",
    "run.services.setIamPolicy",
    "run.jobs.create",
    "run.jobs.update",
    "run.jobs.delete",
    "run.jobs.get",
    "run.jobs.run",
];

/// What the GCP verifier may do with the install's buckets: who may reach a bucket (its IAM
/// policy) and whether it may be public. Bound under a condition on the bucket's name; the
/// verifier checks every policy against the signed stack before it sets it.
const GCP_VERIFIER_BUCKET_PERMISSIONS: &[&str] = &[
    "storage.buckets.getIamPolicy",
    "storage.buckets.setIamPolicy",
    "storage.buckets.update",
];

/// What the GCP verifier uses that the install doesn't name: the network its machines join, the
/// images they boot, the template list (it picks the next template number) and the operations
/// it waits on.
const GCP_VERIFIER_SHARED_PERMISSIONS: &[&str] = &[
    "compute.subnetworks.use",
    "compute.images.useReadOnly",
    "compute.instanceTemplates.list",
    "compute.globalOperations.get",
    "compute.regionOperations.get",
    "compute.zoneOperations.get",
];

/// A protected GCP install's verifier: the Cloud Run service `<prefix>-verifier` running as a
/// service account with one custom role on the install's own resources, `actAs` on each
/// runtime service account only, and reads of the signing parameters. Management never writes
/// compute; it asks the verifier, which only it may invoke, with an ID token of its own.
fn gcp_verifier_blocks(
    pins: &alien_core::SetupPins,
    management_label: Option<&str>,
    runtime_service_accounts: &[String],
    networks: VerifierNetworks,
) -> Result<Vec<Block>> {
    let image = pins
        .gcp_verifier_image
        .as_deref()
        .filter(|image| alien_core::is_pinned_image(image))
        .ok_or_else(|| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "render a protected GCP setup".to_string(),
                reason: "this CLI build has no GCP verifier image pinned by digest; protected setups come from released CLI builds".to_string(),
            })
        })?;
    let member = expr::template("serviceAccount:${google_service_account.signing_verifier.email}");
    let custom_role = |label: &str, suffix: &str, title: &str, permissions: &[&str]| {
        resource_block(
            "google_project_iam_custom_role",
            label,
            [
                attr("project", expr::raw("var.gcp_project")),
                attr(
                    "role_id",
                    expr::raw(format!(
                        "format(\"%s_{suffix}\", replace(local.resource_prefix, \"-\", \"_\"))"
                    )),
                ),
                attr(
                    "title",
                    expr::template(format!("${{local.resource_prefix}} {title}")),
                ),
                attr(
                    "permissions",
                    Expression::Array(
                        permissions
                            .iter()
                            .map(|permission| Expression::String(permission.to_string()))
                            .collect(),
                    ),
                ),
            ],
        )
    };
    let own_resources = [
        "instanceTemplates",
        "instanceGroupManagers",
        "instances",
        "disks",
    ]
    .iter()
    .map(|collection| {
        format!(
            r#"resource.name.extract("/{collection}/{{name}}").startsWith("${{local.resource_prefix}}-")"#
        )
    })
    .collect::<Vec<_>>()
    .join(" || ");
    let mut blocks = vec![
        resource_block(
            "google_service_account",
            "signing_verifier",
            [
                attr("project", expr::raw("var.gcp_project")),
                attr(
                    "account_id",
                    crate::emitters::gcp::helpers::service_account_id_template("verifier"),
                ),
                attr(
                    "display_name",
                    expr::template("${local.resource_prefix}-verifier"),
                ),
                attr(
                    "description",
                    Expression::String(
                        "Applies only releases the vendor signed. The management identity can't change what runs here."
                            .to_string(),
                    ),
                ),
            ],
        ),
        custom_role(
            "signing_verifier",
            "verifier",
            "verifier: the install's machines",
            GCP_VERIFIER_OWN_RESOURCE_PERMISSIONS,
        ),
        custom_role(
            "signing_verifier_cloud_run",
            "verifier_cloud_run",
            "verifier: the install's Cloud Run services and jobs",
            GCP_VERIFIER_CLOUD_RUN_PERMISSIONS,
        ),
        custom_role(
            "signing_verifier_buckets",
            "verifier_buckets",
            "verifier: the install's buckets",
            GCP_VERIFIER_BUCKET_PERMISSIONS,
        ),
        custom_role(
            "signing_verifier_shared",
            "verifier_shared",
            "verifier: network and images",
            GCP_VERIFIER_SHARED_PERMISSIONS,
        ),
        resource_block(
            "google_project_iam_member",
            "signing_verifier",
            [
                attr("project", expr::raw("var.gcp_project")),
                attr(
                    "role",
                    expr::traversal(["google_project_iam_custom_role", "signing_verifier", "name"]),
                ),
                attr("member", member.clone()),
                nested(block(
                    "condition",
                    [
                        attr("title", Expression::String("The install's own resources".to_string())),
                        attr("expression", escaped_template(&own_resources)),
                    ],
                )),
            ],
        ),
        resource_block(
            "google_project_iam_member",
            "signing_verifier_cloud_run",
            [
                attr("project", expr::raw("var.gcp_project")),
                attr(
                    "role",
                    expr::traversal([
                        "google_project_iam_custom_role",
                        "signing_verifier_cloud_run",
                        "name",
                    ]),
                ),
                attr("member", member.clone()),
            ],
        ),
        resource_block(
            "google_project_iam_member",
            "signing_verifier_buckets",
            [
                attr("project", expr::raw("var.gcp_project")),
                attr(
                    "role",
                    expr::traversal([
                        "google_project_iam_custom_role",
                        "signing_verifier_buckets",
                        "name",
                    ]),
                ),
                attr("member", member.clone()),
                nested(block(
                    "condition",
                    [
                        attr("title", Expression::String("The install's own buckets".to_string())),
                        attr(
                            "expression",
                            escaped_template(
                                r#"resource.type == "storage.googleapis.com/Bucket" && resource.name.startsWith("projects/_/buckets/${local.resource_prefix}-")"#,
                            ),
                        ),
                    ],
                )),
            ],
        ),
        resource_block(
            "google_project_iam_member",
            "signing_verifier_shared",
            [
                attr("project", expr::raw("var.gcp_project")),
                attr(
                    "role",
                    expr::traversal([
                        "google_project_iam_custom_role",
                        "signing_verifier_shared",
                        "name",
                    ]),
                ),
                attr("member", member.clone()),
            ],
        ),
        // Cloud Run lets a principal deploy an image only if it may read it, and the verifier
        // deploys the workloads' images. Reading an image changes nothing.
        resource_block(
            "google_project_iam_member",
            "signing_verifier_pulls_images",
            [
                attr("project", expr::raw("var.gcp_project")),
                attr(
                    "role",
                    Expression::String("roles/artifactregistry.reader".to_string()),
                ),
                attr("member", member.clone()),
            ],
        ),
        // The settings are a public value; reading them needs no name condition.
        resource_block(
            "google_project_iam_member",
            "signing_verifier_reads_settings",
            [
                attr("project", expr::raw("var.gcp_project")),
                attr(
                    "role",
                    // It lists a parameter's versions and reads the newest in full
                    // (parameterVersions.list and .get); the accessor role only renders.
                    Expression::String("roles/parametermanager.parameterViewer".to_string()),
                ),
                attr("member", member.clone()),
            ],
        ),
    ];
    // On the ledger secret alone: read its newest version and add one.
    for (label, role) in [
        (
            "signing_verifier_reads_ledger",
            "roles/secretmanager.secretAccessor",
        ),
        (
            "signing_verifier_records_ledger",
            "roles/secretmanager.secretVersionAdder",
        ),
    ] {
        blocks.push(resource_block(
            "google_secret_manager_secret_iam_member",
            label,
            [
                attr("project", expr::raw("var.gcp_project")),
                attr(
                    "secret_id",
                    expr::traversal(["google_secret_manager_secret", "signing_state", "secret_id"]),
                ),
                attr("role", Expression::String(role.to_string())),
                attr("member", member.clone()),
            ],
        ));
    }
    for label in runtime_service_accounts {
        blocks.push(resource_block(
            "google_service_account_iam_member",
            &format!("signing_verifier_acts_as_{label}"),
            [
                attr(
                    "service_account_id",
                    expr::traversal(["google_service_account", label, "name"]),
                ),
                attr(
                    "role",
                    Expression::String("roles/iam.serviceAccountUser".to_string()),
                ),
                attr("member", member.clone()),
            ],
        ));
    }
    let env = |name: &str, value: Expression| {
        nested(block(
            "env",
            [
                attr("name", Expression::String(name.to_string())),
                attr("value", value),
            ],
        ))
    };
    let mut container = vec![
        attr("image", Expression::String(image.to_string())),
        nested(block(
            "ports",
            [attr("container_port", Expression::Number(8080.into()))],
        )),
        env("VERIFIER_CLOUD", Expression::String("gcp".to_string())),
    ];
    container.extend(networks.env().map(|(name, network)| env(name, network)));
    blocks.push(resource_block(
        "google_cloud_run_v2_service",
        "signing_verifier",
        [
            attr("name", expr::template("${local.resource_prefix}-verifier")),
            attr("project", expr::raw("var.gcp_project")),
            attr("location", expr::raw("var.gcp_region")),
            attr(
                "ingress",
                Expression::String("INGRESS_TRAFFIC_ALL".to_string()),
            ),
            attr(
                "labels",
                expr::object([("managed-by", Expression::String("setup".to_string()))]),
            ),
            attr("deletion_protection", Expression::Bool(false)),
            nested(block(
                "template",
                [
                    attr(
                        "service_account",
                        expr::traversal(["google_service_account", "signing_verifier", "email"]),
                    ),
                    nested(block(
                        "scaling",
                        [
                            attr("min_instance_count", Expression::Number(0.into())),
                            attr("max_instance_count", Expression::Number(1.into())),
                        ],
                    )),
                    nested(block("containers", container)),
                ],
            )),
            attr(
                "depends_on",
                Expression::Array(vec![
                    expr::traversal(["google_project_iam_member", "signing_verifier"]),
                    expr::traversal(["google_project_iam_member", "signing_verifier_shared"]),
                    expr::traversal(["google_project_iam_member", "signing_verifier_cloud_run"]),
                    expr::traversal(["google_project_iam_member", "signing_verifier_pulls_images"]),
                    expr::traversal([
                        "google_project_iam_member",
                        "signing_verifier_reads_settings",
                    ]),
                    expr::traversal([
                        "google_secret_manager_secret_iam_member",
                        "signing_verifier_reads_ledger",
                    ]),
                    expr::traversal([
                        "google_secret_manager_secret_iam_member",
                        "signing_verifier_records_ledger",
                    ]),
                ]),
            ),
        ],
    ));
    if let Some(management) = management_label {
        let management_member = expr::template(format!(
            "serviceAccount:${{google_service_account.{management}.email}}"
        ));
        blocks.push(resource_block(
            "google_cloud_run_v2_service_iam_member",
            "signing_verifier_invoker",
            [
                attr("project", expr::raw("var.gcp_project")),
                attr("location", expr::raw("var.gcp_region")),
                attr(
                    "name",
                    expr::traversal(["google_cloud_run_v2_service", "signing_verifier", "name"]),
                ),
                attr("role", Expression::String("roles/run.invoker".to_string())),
                attr("member", management_member.clone()),
            ],
        ));
        // An ID token of its own proves only who management already is; it grants nothing.
        blocks.push(resource_block(
            "google_service_account_iam_member",
            "signing_management_id_token",
            [
                attr(
                    "service_account_id",
                    expr::traversal(["google_service_account", management, "name"]),
                ),
                attr(
                    "role",
                    Expression::String("roles/iam.serviceAccountOpenIdTokenCreator".to_string()),
                ),
                attr("member", management_member),
            ],
        ));
    }
    Ok(blocks)
}

/// Where setup lets a protected install's verifier run what it writes.
struct VerifierNetworks {
    /// Where it runs functions on AWS and GCP: `VERIFIER_FUNCTION_VPC`, JSON.
    functions: Option<Expression>,
    /// Where it launches machines: `VERIFIER_MACHINE_NETWORK`, JSON.
    machines: Option<Expression>,
    /// Where it runs functions on Azure: `VERIFIER_AZURE_ENVIRONMENT`, the resource id of the
    /// Container Apps environment.
    environment: Option<Expression>,
}

impl VerifierNetworks {
    /// The verifier's environment variables that carry them.
    fn env(self) -> impl Iterator<Item = (&'static str, Expression)> {
        [
            ("VERIFIER_FUNCTION_VPC", self.functions),
            ("VERIFIER_MACHINE_NETWORK", self.machines),
            ("VERIFIER_AZURE_ENVIRONMENT", self.environment),
        ]
        .into_iter()
        .filter_map(|(name, network)| network.map(|network| (name, network)))
    }
}

/// Where a protected install's verifier runs functions, when the stack has a network: the
/// subnets an unprotected install's functions join. On AWS, `{"subnetIds", "securityGroupIds"}`
/// of the private subnets and the workload security group (none on the default VPC, which
/// functions stay out of). On GCP, `{"subnetIds": [subnet self link], "securityGroupIds": [],
/// "egress"}` for Cloud Run's Direct VPC egress. Azure functions take their network from the
/// Container Apps environment.
fn verifier_function_network(ctx: &EmitContext<'_>) -> Option<Expression> {
    let (stack, labels) = (ctx.stack, ctx.names);
    let (network_id, entry) = stack
        .resources()
        .find(|(_, entry)| entry.config.downcast_ref::<Network>().is_some())?;
    let network = entry.config.downcast_ref::<Network>()?;
    let label = labels.get(network_id)?;
    match ctx.platform {
        Platform::Aws => {
            let ctx = EmitContext {
                resource: entry,
                resource_id: network_id,
                ..*ctx
            };
            Some(expr::jsonencode(expr::object([
                (
                    "subnetIds",
                    crate::emitters::aws::helpers::private_subnet_ids_expr(&ctx),
                ),
                (
                    "securityGroupIds",
                    crate::emitters::aws::helpers::security_group_ids_expr(&ctx),
                ),
            ])))
        }
        Platform::Gcp => match network.settings {
            NetworkSettings::Create { .. }
            | NetworkSettings::UseDefault
            | NetworkSettings::ByoVpcGcp { .. } => Some(expr::jsonencode(expr::object([
                (
                    "subnetIds",
                    crate::emitters::gcp::network::subnet_self_links(label, &network.settings),
                ),
                ("securityGroupIds", Expression::Array(Vec::new())),
                (
                    "egress",
                    crate::emitters::gcp::network::function_egress(&network.settings),
                ),
            ]))),
            _ => None,
        },
        _ => None,
    }
}

/// The Container Apps environment a protected Azure install's workloads run in, when the stack
/// has one: the one setup creates, or the one the stack is bound to. The install's group also
/// holds the verifier's own environment, so the verifier takes this one from setup instead of
/// choosing. A stack with several is refused: its workers' environment would be a guess.
fn verifier_workload_environment(ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
    let stack = ctx.stack;
    if ctx.platform != Platform::Azure {
        return Ok(None);
    }
    let environments: Vec<_> = stack
        .resources()
        .filter(|(_, entry)| {
            entry.config.resource_type() == AzureContainerAppsEnvironment::RESOURCE_TYPE
        })
        .collect();
    let (environment_id, entry) = match environments.as_slice() {
        [] => return Ok(None),
        [environment] => *environment,
        _ => {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "render a protected Azure setup".to_string(),
                reason: format!(
                    "the stack has {} Container Apps environments; the verifier runs a protected install's workers in exactly one",
                    environments.len()
                ),
            }))
        }
    };
    let ctx = EmitContext {
        resource: entry,
        resource_id: environment_id,
        ..*ctx
    };
    if let Some(binding) = azure_helpers::container_apps_environment_binding(&ctx, environment_id)?
    {
        return azure_helpers::binding_string_expr(
            environment_id,
            "resource_id",
            &binding.resource_id,
        )
        .map(Some);
    }
    Ok(Some(expr::traversal([
        "azurerm_container_app_environment",
        azure_helpers::required_label(&ctx)?,
        "id",
    ])))
}

/// Where a protected install's verifier launches machines, when the stack has any: JSON
/// `{"subnetIds": [..], "securityGroupIds": [..]}` (GCP and Azure add `identity`). On AWS it
/// comes from the network setup creates or the customer names; machines in the default VPC
/// have no subnets setup fixes, so the render is refused. On GCP and Azure each compute
/// cluster's emitter names it ([`crate::emitter::TfEmitter::machine_network`]); a cluster
/// that names none, or clusters that name different networks, are refused.
fn verifier_machine_network(
    ctx: &EmitContext<'_>,
    registry: &TfRegistry,
) -> Result<Option<Expression>> {
    let stack = ctx.stack;
    let clusters: Vec<_> = stack
        .resources()
        .filter(|(_, entry)| entry.config.resource_type() == ComputeCluster::RESOURCE_TYPE)
        .collect();
    if clusters.is_empty() {
        return Ok(None);
    }
    match ctx.platform {
        Platform::Aws => aws_verifier_machine_network(ctx),
        Platform::Gcp | Platform::Azure => {
            let mut network: Option<Expression> = None;
            for (cluster_id, entry) in clusters {
                let refuse = |reason: &str| {
                    AlienError::new(ErrorData::OperationNotSupported {
                        operation: format!(
                            "protect the machines of compute cluster '{cluster_id}'"
                        ),
                        reason: reason.to_string(),
                    })
                };
                let ctx = EmitContext {
                    resource: entry,
                    resource_id: cluster_id,
                    ..*ctx
                };
                let named = registry
                    .require(&ComputeCluster::RESOURCE_TYPE, ctx.platform)?
                    .machine_network(&ctx)?
                    .ok_or_else(|| {
                        refuse(
                            "its emitter names no network for the verifier to launch machines into",
                        )
                    })?;
                match &network {
                    Some(existing) if *existing != named => {
                        return Err(refuse(
                            "the verifier launches every cluster's machines into one network, and this cluster names another",
                        ));
                    }
                    Some(_) => {}
                    None => network = Some(named),
                }
            }
            Ok(network)
        }
        _ => Ok(None),
    }
}

fn aws_verifier_machine_network(ctx: &EmitContext<'_>) -> Result<Option<Expression>> {
    let stack = ctx.stack;
    let refuse = || {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: "protect machines in the default VPC".to_string(),
            reason: "the verifier launches machines only into subnets setup fixes; create a network or name existing subnets".to_string(),
        })
    };
    let (network_id, entry) = stack
        .resources()
        .find(|(_, entry)| entry.config.downcast_ref::<Network>().is_some())
        .ok_or_else(refuse)?;
    if matches!(
        entry
            .config
            .downcast_ref::<Network>()
            .map(|network| &network.settings),
        Some(NetworkSettings::UseDefault)
    ) {
        return Err(refuse());
    }
    let ctx = EmitContext {
        resource: entry,
        resource_id: network_id,
        ..*ctx
    };
    Ok(Some(expr::jsonencode(expr::object([
        (
            "subnetIds",
            crate::emitters::aws::helpers::private_subnet_ids_expr(&ctx),
        ),
        (
            "securityGroupIds",
            crate::emitters::aws::helpers::security_group_ids_expr(&ctx),
        ),
    ]))))
}
