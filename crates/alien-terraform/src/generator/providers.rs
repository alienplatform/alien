//! Terraform `providers.tf` / `locals.tf` / `resource_prefix.tf` generation
//! and the deployment-settings expressions they depend on.

use crate::{
    block::{attr, block, resource_block},
    expr,
    target::TerraformTarget,
};
use alien_core::{
    DeploymentModel, HeartbeatsMode, NetworkSettings, Result, StackSettings, TelemetryMode,
    UpdatesMode,
};
use hcl::{
    expr::Expression,
    structure::{Block, BlockLabel, Body, Structure},
    Identifier,
};
use indexmap::IndexMap;

pub(super) fn providers_body(
    target: TerraformTarget,
    include_kubernetes_provider: bool,
    include_helm_provider: bool,
    include_azapi_provider: bool,
) -> Body {
    let mut structures: Vec<Structure> = Vec::new();
    match target.cloud_platform() {
        alien_core::Platform::Aws => {
            structures.push(Structure::Block(Block {
                identifier: Identifier::sanitized("provider"),
                labels: vec![BlockLabel::String("aws".to_string())],
                body: Body::from(vec![attr("region", expr::raw("var.aws_region"))]),
            }));
            structures.push(Structure::Block(Block {
                identifier: Identifier::sanitized("data"),
                labels: vec![
                    BlockLabel::String("aws_caller_identity".to_string()),
                    BlockLabel::String("current".to_string()),
                ],
                body: Body::default(),
            }));
            structures.push(Structure::Block(Block {
                identifier: Identifier::sanitized("data"),
                labels: vec![
                    BlockLabel::String("aws_region".to_string()),
                    BlockLabel::String("current".to_string()),
                ],
                body: Body::default(),
            }));
        }
        alien_core::Platform::Gcp => {
            structures.push(Structure::Block(Block {
                identifier: Identifier::sanitized("provider"),
                labels: vec![BlockLabel::String("google".to_string())],
                body: Body::from(vec![
                    attr("project", expr::raw("var.gcp_project")),
                    attr("region", expr::raw("var.gcp_region")),
                ]),
            }));
        }
        alien_core::Platform::Azure => {
            structures.push(Structure::Block(Block {
                identifier: Identifier::sanitized("provider"),
                labels: vec![BlockLabel::String("azurerm".to_string())],
                body: Body::from(vec![
                    attr(
                        "resource_provider_registrations",
                        Expression::String("none".to_string()),
                    ),
                    Structure::Block(block("features", [])),
                ]),
            }));
            if include_azapi_provider {
                structures.push(Structure::Block(Block {
                    identifier: Identifier::sanitized("provider"),
                    labels: vec![BlockLabel::String("azapi".to_string())],
                    body: Body::default(),
                }));
            }
        }
        _ => {}
    }
    if include_kubernetes_provider {
        structures.push(Structure::Block(Block {
            identifier: Identifier::sanitized("provider"),
            labels: vec![BlockLabel::String("kubernetes".to_string())],
            body: Body::from(kubernetes_provider_body(target)),
        }));
    }
    if include_helm_provider {
        structures.push(Structure::Block(Block {
            identifier: Identifier::sanitized("provider"),
            labels: vec![BlockLabel::String("helm".to_string())],
            body: Body::from(vec![attr(
                "kubernetes",
                provider_config_object(kubernetes_provider_body(target)),
            )]),
        }));
    }
    Body::from(structures)
}

fn provider_config_object(items: Vec<Structure>) -> Expression {
    expr::object(items.into_iter().filter_map(|item| {
        let Structure::Attribute(attribute) = item else {
            return None;
        };
        Some((attribute.key.to_string(), attribute.expr))
    }))
}

fn kubernetes_provider_body(target: TerraformTarget) -> Vec<Structure> {
    match target {
        // EKS: use the `exec` auth plugin instead of
        // `data.aws_eks_cluster_auth.target.token` because that data source
        // resolves the token at *plan* time. The token's lifetime is ~15
        // minutes; a multi-step apply (cluster creation, IAM provisioning,
        // then Helm install) routinely exceeds that and surfaces as "the
        // server has asked for the client to provide credentials" mid-apply.
        // `exec` regenerates the token per Kubernetes API call. The
        // generated `kubernetes_kubeconfig` local already uses this pattern
        // for the `kubectl` CLI output; mirroring it here keeps both paths
        // honoring the same auth flow.
        TerraformTarget::Eks => vec![
            attr("host", expr::raw("data.aws_eks_cluster.target.endpoint")),
            attr(
                "cluster_ca_certificate",
                expr::raw("base64decode(data.aws_eks_cluster.target.certificate_authority[0].data)"),
            ),
            // `exec` is expressed as an *attribute* (object literal), not a
            // nested HCL block, because this same body is reused inside the
            // `helm` provider as `kubernetes = { … }` (object form, which
            // only takes attributes — `provider_config_object` drops
            // anything that isn't an Attribute). The kubernetes provider
            // accepts both block and attribute forms, so attribute form
            // works in both places.
            attr(
                "exec",
                expr::object([
                    (
                        "api_version",
                        Expression::String(
                            "client.authentication.k8s.io/v1beta1".to_string(),
                        ),
                    ),
                    ("command", Expression::String("aws".to_string())),
                    (
                        "args",
                        expr::raw(
                            "[\"eks\", \"get-token\", \"--cluster-name\", data.aws_eks_cluster.target.name, \"--region\", var.aws_region]",
                        ),
                    ),
                ]),
            ),
        ],
        TerraformTarget::Gke => vec![
            attr(
                "host",
                expr::raw("\"https://${data.google_container_cluster.target.endpoint}\""),
            ),
            attr(
                "cluster_ca_certificate",
                expr::raw(
                    "base64decode(data.google_container_cluster.target.master_auth[0].cluster_ca_certificate)",
                ),
            ),
            attr("token", expr::raw("data.google_client_config.current.access_token")),
        ],
        TerraformTarget::Aks => vec![
            attr(
                "host",
                expr::raw("data.azurerm_kubernetes_cluster.target.kube_config[0].host"),
            ),
            attr(
                "cluster_ca_certificate",
                expr::raw(
                    "base64decode(data.azurerm_kubernetes_cluster.target.kube_config[0].cluster_ca_certificate)",
                ),
            ),
            attr(
                "client_certificate",
                expr::raw(
                    "base64decode(data.azurerm_kubernetes_cluster.target.kube_config[0].client_certificate)",
                ),
            ),
            attr(
                "client_key",
                expr::raw("base64decode(data.azurerm_kubernetes_cluster.target.kube_config[0].client_key)"),
            ),
        ],
        _ => Vec::new(),
    }
}

pub(super) fn resource_prefix_body() -> Body {
    Body::from(vec![Structure::Block(resource_block(
        "random_id",
        "resource_prefix",
        [attr(
            "byte_length",
            Expression::Number(hcl::Number::from(4)),
        )],
    ))])
}

pub(super) fn locals_body(
    target: TerraformTarget,
    stack_settings: &StackSettings,
    registration_resources: Vec<(Option<String>, Expression)>,
    extra: &IndexMap<String, Expression>,
    has_remote_management: bool,
) -> Result<Body> {
    let mut body: Vec<Structure> = Vec::new();

    body.push(attr(
        "resource_prefix",
        expr::raw(
            "var.resource_prefix == \"\" ? format(\"a%s\", random_id.resource_prefix.hex) : var.resource_prefix",
        ),
    ));
    body.push(attr("deployment_name", expr::raw("var.name")));
    if matches!(target.cloud_platform(), alien_core::Platform::Gcp) {
        body.push(attr(
            "gcp_custom_role_prefix",
            expr::raw(
                "substr(replace(lower(var.gcp_custom_role_prefix == \"\" ? local.resource_prefix : var.gcp_custom_role_prefix), \"-\", \"_\"), 0, 18)",
            ),
        ));
    }
    body.push(attr(
        "deployment_platform",
        Expression::String(target.deployment_platform().as_str().to_string()),
    ));
    if let Some(base_platform) = target.base_platform() {
        body.push(attr(
            "deployment_base_platform",
            Expression::String(base_platform.as_str().to_string()),
        ));
    }
    body.push(attr(
        "deployment_target",
        Expression::String(target.name().to_string()),
    ));
    body.push(attr("deployment_region", region_expression(target)));
    body.push(attr(
        "deployment_management_config",
        if has_remote_management {
            management_config_expression(target)
        } else {
            expr::raw("null")
        },
    ));
    body.push(attr(
        "advanced_settings",
        expr::raw(
            "merge(jsondecode(var.advanced_settings_json), jsondecode(var.advanced_settings_overlay_json))",
        ),
    ));
    if target.is_kubernetes() {
        let generated_kubernetes_exposure = extra
            .get("kubernetes_exposure")
            .cloned()
            .unwrap_or_else(|| {
                expr::object([("mode", Expression::String("disabled".to_string()))])
            });
        if matches!(target, TerraformTarget::Eks) {
            body.push(attr(
                "generated_kubernetes_exposure",
                generated_kubernetes_exposure,
            ));
            body.push(attr(
                "kubernetes_exposure",
                expr::raw(
                    r#"jsondecode(var.custom_domain_name == "" ? jsonencode(local.generated_kubernetes_exposure) : jsonencode(merge(local.generated_kubernetes_exposure, {
  mode   = "custom"
  domain = var.custom_domain_name
  certificate = {
    mode           = "awsAcmArn"
    certificateArn = var.custom_domain_certificate_arn
  }
})))"#,
                ),
            ));
        } else {
            body.push(attr("kubernetes_exposure", generated_kubernetes_exposure));
        }
        body.push(attr(
            "deployment_kubernetes_settings",
            expr::raw(
                r#"merge(try(local.advanced_settings.kubernetes, {}), {
  exposure = jsondecode(try(local.advanced_settings.kubernetes.exposure, null) == null ? jsonencode(local.kubernetes_exposure) : jsonencode(local.advanced_settings.kubernetes.exposure))
})"#,
            ),
        ));
    }
    body.push(attr(
        "deployment_settings",
        stack_settings_expression(target, stack_settings),
    ));
    body.push(attr(
        "deployment_resources",
        crate::emitters::enabled::registration_list(registration_resources),
    ));

    if target.is_kubernetes() {
        for (name, value) in [
            ("kubernetes_kubeconfig", Expression::String(String::new())),
            ("kubernetes_kube_context", Expression::String(String::new())),
        ] {
            if !extra.contains_key(name) {
                body.push(attr(name, value));
            }
        }
    }

    for (name, value) in extra {
        if target.is_kubernetes() && name == "kubernetes_exposure" {
            continue;
        }
        body.push(attr(name, value.clone()));
    }
    Ok(Body::from(vec![Structure::Block(Block {
        identifier: Identifier::sanitized("locals"),
        labels: vec![],
        body: Body::from(body),
    })]))
}

fn region_expression(target: TerraformTarget) -> Expression {
    match target.cloud_platform() {
        alien_core::Platform::Aws => expr::raw("data.aws_region.current.region"),
        alien_core::Platform::Gcp => expr::raw("var.gcp_region"),
        alien_core::Platform::Azure => expr::raw("var.azure_location"),
        platform => Expression::String(platform.as_str().to_string()),
    }
}

fn management_config_expression(target: TerraformTarget) -> Expression {
    match target.cloud_platform() {
        alien_core::Platform::Aws => expr::raw(
            r#"var.deployment_model == "push" ? {
  platform        = "aws"
  managingRoleArn = var.managing_role_arn
} : null"#,
        ),
        alien_core::Platform::Gcp => expr::raw(
            r#"var.deployment_model == "push" ? {
  platform            = "gcp"
  projectId           = var.gcp_project
  serviceAccountEmail = var.managing_service_account_email
} : null"#,
        ),
        alien_core::Platform::Azure => expr::raw(
            r#"var.deployment_model == "push" ? {
  platform          = "azure"
  managingTenantId  = var.azure_managing_tenant_id
  oidcIssuer        = var.azure_oidc_issuer
  oidcSubject       = var.azure_oidc_subject
} : null"#,
        ),
        platform => Expression::String(platform.as_str().to_string()),
    }
}

fn stack_settings_expression(
    target: TerraformTarget,
    stack_settings: &StackSettings,
) -> Expression {
    match target.cloud_platform() {
        alien_core::Platform::Aws
            if has_dynamic_aws_network_settings(stack_settings.network.as_ref()) =>
        {
            let kubernetes_settings = if target.is_kubernetes() {
                "\n  kubernetes = local.deployment_kubernetes_settings"
            } else {
                ""
            };
            return expr::raw(format!(
                r#"merge(local.advanced_settings, {{
  deploymentModel = var.deployment_model
  updates    = var.updates_mode
  telemetry  = var.telemetry_mode
  heartbeats = var.heartbeats_mode
  network = jsondecode(
    var.network_mode == "create-new" ? jsonencode({{
      type              = "create"
      cidr              = var.vpc_cidr == "" ? null : var.vpc_cidr
      availabilityZones = var.availability_zones
    }}) : var.network_mode == "use-existing" ? jsonencode({{
      type             = "byo-vpc-aws"
      vpcId            = var.vpc_id
      publicSubnetIds  = var.public_subnet_ids
      privateSubnetIds = var.private_subnet_ids
      securityGroupIds = var.security_group_ids
    }}) : jsonencode({{
      type = "use-default"
    }})
  )
{kubernetes_settings}
}})"#
            ));
        }
        alien_core::Platform::Gcp
            if has_dynamic_gcp_network_settings(stack_settings.network.as_ref()) =>
        {
            let kubernetes_settings = if target.is_kubernetes() {
                "\n  kubernetes = local.deployment_kubernetes_settings"
            } else {
                ""
            };
            return expr::raw(format!(
                r#"merge(local.advanced_settings, {{
  deploymentModel = var.deployment_model
  updates    = var.updates_mode
  telemetry  = var.telemetry_mode
  heartbeats = var.heartbeats_mode
  network = jsondecode(
    var.network_mode == "create-new" ? jsonencode({{
      type              = "create"
      cidr              = var.network_cidr == "" ? null : var.network_cidr
      availabilityZones = var.availability_zones
    }}) : var.network_mode == "use-existing" ? jsonencode({{
      type        = "byo-vpc-gcp"
      networkName = var.network_name
      subnetName  = var.subnet_name
      region      = var.network_region == "" ? var.gcp_region : var.network_region
    }}) : jsonencode({{
      type = "use-default"
    }})
  )
{kubernetes_settings}
}})"#
            ));
        }
        _ if target.is_kubernetes() => {
            return expr::raw(
                r#"merge(local.advanced_settings, {
  deploymentModel = var.deployment_model
  updates    = var.updates_mode
  telemetry  = var.telemetry_mode
  heartbeats = var.heartbeats_mode
  kubernetes = local.deployment_kubernetes_settings
})"#,
            );
        }
        _ => {
            return expr::raw(
                r#"merge(local.advanced_settings, {
  deploymentModel = var.deployment_model
  updates    = var.updates_mode
  telemetry  = var.telemetry_mode
  heartbeats = var.heartbeats_mode
})"#,
            );
        }
    }
}

pub(super) fn deployment_model(model: DeploymentModel) -> &'static str {
    match model {
        DeploymentModel::Push => "push",
        DeploymentModel::Pull => "pull",
    }
}

pub(super) fn updates_mode(mode: UpdatesMode) -> &'static str {
    match mode {
        UpdatesMode::Auto => "auto",
        UpdatesMode::ApprovalRequired => "approval-required",
    }
}

pub(super) fn telemetry_mode(mode: TelemetryMode) -> &'static str {
    match mode {
        TelemetryMode::Off => "off",
        TelemetryMode::Auto => "auto",
        TelemetryMode::ApprovalRequired => "approval-required",
    }
}

pub(super) fn heartbeats_mode(mode: HeartbeatsMode) -> &'static str {
    match mode {
        HeartbeatsMode::Off => "off",
        HeartbeatsMode::On => "on",
    }
}

pub(super) fn has_dynamic_aws_network_settings(network: Option<&NetworkSettings>) -> bool {
    matches!(
        network,
        Some(
            NetworkSettings::Create { .. }
                | NetworkSettings::UseDefault
                | NetworkSettings::ByoVpcAws { .. }
        )
    )
}

pub(super) fn has_dynamic_gcp_network_settings(network: Option<&NetworkSettings>) -> bool {
    matches!(network, Some(NetworkSettings::Create { .. }))
}
