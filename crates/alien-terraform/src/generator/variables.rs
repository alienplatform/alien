//! Terraform `variables.tf` generation: input-variable blocks, stack-input
//! wiring, and the advanced-settings default payload.

use super::providers::{
    deployment_model, has_dynamic_aws_network_settings, has_dynamic_gcp_network_settings,
    heartbeats_mode, telemetry_mode, updates_mode,
};
use super::{NetworkVariables, TerraformHelmInstall, TerraformRegistration};
use crate::{
    block::{attr, block, nested},
    expr,
    target::TerraformTarget,
};
use alien_core::{
    ErrorData, KubernetesCertificateMode, KubernetesExposureSettings, KubernetesSettings, Result,
    Stack, StackInputDefaultValue, StackInputDefinition, StackInputKind, StackInputProvider,
    StackInputValidation, StackSettings,
};
use alien_error::{AlienError, IntoAlienError};
use hcl::{
    expr::Expression,
    structure::{Block, BlockLabel, Body, Structure},
    Identifier,
};

pub(super) fn stack_inputs_for_terraform(
    stack: &Stack,
    target: TerraformTarget,
) -> Vec<StackInputDefinition> {
    let platform = target.deployment_platform();
    stack
        .inputs()
        .iter()
        .filter(|input| {
            input.provided_by.contains(&StackInputProvider::Deployer)
                && input
                    .platforms
                    .as_ref()
                    .is_none_or(|platforms| platforms.contains(&platform))
        })
        .cloned()
        .collect()
}

pub(super) fn validate_stack_inputs_for_terraform(inputs: &[StackInputDefinition]) -> Result<()> {
    let secret_inputs: Vec<&str> = inputs
        .iter()
        .filter(|input| input.kind == StackInputKind::Secret)
        .map(|input| input.id.as_str())
        .collect();
    if secret_inputs.is_empty() {
        return Ok(());
    }

    Err(AlienError::new(ErrorData::OperationNotSupported {
        operation: "generate_terraform_module".to_string(),
        reason: format!(
            "Terraform deployer-provided secret stack inputs are not enabled because this provider cannot prove values stay out of Terraform state yet. Use the deployment portal, CloudFormation, or deploy CLI for secret inputs, or move these inputs out of the Terraform setup path: {}",
            secret_inputs.join(", ")
        ),
    }))
}

pub(super) fn terraform_stack_input_variable_name(input: &StackInputDefinition) -> String {
    format!("input_{}", snake_case_identifier(&input.id))
}

fn snake_case_identifier(value: &str) -> String {
    let mut output = String::new();
    let mut previous_was_separator = true;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase() && !previous_was_separator && !output.ends_with('_') {
                output.push('_');
            }
            output.push(ch.to_ascii_lowercase());
            previous_was_separator = false;
        } else if !output.ends_with('_') {
            output.push('_');
            previous_was_separator = true;
        }
    }
    let output = output.trim_matches('_').to_string();
    if output.is_empty() {
        "value".to_string()
    } else if output
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_digit())
    {
        format!("value_{output}")
    } else {
        output
    }
}

pub(super) fn variables_body(
    target: TerraformTarget,
    network_vars: &NetworkVariables,
    stack_settings: &StackSettings,
    registration: Option<&TerraformRegistration>,
    helm_install: Option<&TerraformHelmInstall>,
    deployment_name_default: &str,
    needs_azure_management_inputs: bool,
    supported_aws_regions: &[String],
    stack_inputs: &[StackInputDefinition],
) -> Result<Body> {
    let mut blocks: Vec<Structure> = Vec::new();
    let advanced_settings_default = advanced_settings_default_json(target, stack_settings)?;

    blocks.push(nested(resource_prefix_variable_block()));

    blocks.push(nested(variable_block(
        "name",
        "Human-readable application name shown in setup and cloud IAM review metadata.",
        registration
            .is_some()
            .then(|| Expression::String(deployment_name_default.to_string())),
        false,
    )));
    blocks.push(nested(variable_block(
        "token",
        "Install token from the application setup page. This is the same token used by the deploy CLI --token flag.",
        None,
        true,
    )));
    blocks.push(nested(variable_block(
        "management_url",
        "Optional management endpoint used by pull-style runtimes.",
        Some(Expression::String("".to_string())),
        false,
    )));
    blocks.push(nested(string_enum_variable_block(
        "deployment_model",
        "How runtime updates are delivered after setup.",
        deployment_model(stack_settings.deployment_model),
        &["push", "pull"],
    )));
    blocks.push(nested(variable_block(
        "advanced_settings_json",
        "Advanced JSON-encoded deployment settings. Most installations should use the typed variables in this module instead.",
        Some(Expression::String(advanced_settings_default)),
        true,
    )));
    blocks.push(nested(variable_block(
        "advanced_settings_overlay_json",
        "JSON-encoded deployment settings merged over the package defaults. Use this for partial advanced-setting overrides that must preserve generated defaults such as compute selections.",
        Some(Expression::String("{}".to_string())),
        true,
    )));
    blocks.push(nested(string_enum_variable_block(
        "updates_mode",
        "How application updates are delivered after setup.",
        updates_mode(stack_settings.updates),
        &["auto", "approval-required"],
    )));
    blocks.push(nested(string_enum_variable_block(
        "telemetry_mode",
        "How logs, metrics, and traces are collected.",
        telemetry_mode(stack_settings.telemetry),
        &["off", "auto", "approval-required"],
    )));
    blocks.push(nested(string_enum_variable_block(
        "heartbeats_mode",
        "Whether runtime health checks are enabled.",
        heartbeats_mode(stack_settings.heartbeats),
        &["off", "on"],
    )));

    if matches!(target.cloud_platform(), alien_core::Platform::Aws) {
        blocks.push(nested(aws_region_variable_block(supported_aws_regions)));
        blocks.push(nested(variable_block(
            "managing_role_arn",
            "ARN of the management identity allowed to assume setup-created roles.",
            Some(Expression::String(String::new())),
            false,
        )));
        blocks.push(nested(variable_block(
            "managing_account_id",
            "AWS account ID that hosts application container images. Empty disables scoped cross-account image-pull grants.",
            Some(Expression::String(String::new())),
            false,
        )));
    }
    if matches!(target.cloud_platform(), alien_core::Platform::Aws)
        && has_dynamic_aws_network_settings(stack_settings.network.as_ref())
    {
        blocks.push(nested(variable_block(
            "network_mode",
            "Choose whether this setup creates a new network, uses an existing network, or uses the default network. Values: create-new, use-existing, use-default.",
            Some(Expression::String("create-new".to_string())),
            false,
        )));
        blocks.push(nested(variable_block(
            "vpc_cidr",
            "CIDR for a newly-created network. Empty uses 10.42.0.0/16.",
            Some(Expression::String(String::new())),
            false,
        )));
        blocks.push(nested(number_variable_block(
            "availability_zones",
            "Number of availability zones to use when creating a new network.",
            Some(2),
        )));
        blocks.push(nested(variable_block(
            "vpc_id",
            "Existing VPC ID. Required when network is use-existing.",
            Some(Expression::String(String::new())),
            false,
        )));
        blocks.push(nested(list_variable_block(
            "public_subnet_ids",
            "Existing public subnet IDs. Required when network is use-existing.",
            Some(vec![]),
        )));
        blocks.push(nested(list_variable_block(
            "private_subnet_ids",
            "Existing private subnet IDs. Required when network is use-existing.",
            Some(vec![]),
        )));
        blocks.push(nested(list_variable_block(
            "security_group_ids",
            "Existing security group IDs. Required when network is use-existing.",
            Some(vec![]),
        )));
        blocks.push(nested(list_variable_block(
            "unsupported_availability_zone_ids",
            "Availability Zone IDs to exclude when selecting EKS control-plane subnets. \
             AWS publishes EKS-disallowed zones by ID, not by name, because AZ names are \
             account-local — the same physical zone can be `us-east-1e` in one account and \
             `us-east-1c` in another. The defaults cover the AZs AWS documents as not supporting \
             EKS control plane today (see \
             https://docs.aws.amazon.com/eks/latest/userguide/network-reqs.html); override per \
             region when AWS deprecates more.",
            Some(vec![
                // us-east-1e
                "use1-az3".to_string(),
                // us-west-1b
                "usw1-az2".to_string(),
                // ca-central-1d
                "cac1-az3".to_string(),
            ]),
        )));
    }
    if matches!(target.cloud_platform(), alien_core::Platform::Gcp) {
        blocks.push(nested(variable_block(
            "gcp_project",
            "GCP project ID.",
            None,
            false,
        )));
        blocks.push(nested(variable_block(
            "gcp_region",
            "GCP region.",
            Some(Expression::String("us-central1".to_string())),
            false,
        )));
        blocks.push(nested(variable_block(
            "managing_service_account_email",
            "Email of the management service account allowed to impersonate setup-created identities. Empty disables the binding.",
            Some(Expression::String(String::new())),
            false,
        )));
        blocks.push(nested(bool_variable_block(
            "gcp_manage_custom_roles",
            "Whether this module creates the GCP project custom roles it binds. Set to false when those roles are managed outside this stack.",
            Some(true),
        )));
        blocks.push(nested(variable_block(
            "gcp_custom_role_prefix",
            "Prefix used for GCP project custom role IDs when gcp_manage_custom_roles is false. Empty uses resource_prefix.",
            Some(Expression::String(String::new())),
            false,
        )));
        if has_dynamic_gcp_network_settings(stack_settings.network.as_ref()) {
            blocks.push(nested(variable_block(
                "network_mode",
                "Choose whether this setup creates a new network, uses an existing network, or uses the default network. Values: create-new, use-existing, use-default.",
                Some(Expression::String("create-new".to_string())),
                false,
            )));
            blocks.push(nested(variable_block(
                "network_cidr",
                "CIDR for a newly-created network. Empty uses 10.44.0.0/16.",
                Some(Expression::String(String::new())),
                false,
            )));
            blocks.push(nested(number_variable_block(
                "availability_zones",
                "Reserved for cross-cloud network-mode parity. GCP creates one regional subnet.",
                Some(2),
            )));
            blocks.push(nested(variable_block(
                "network_name",
                "Existing VPC network name. Required when network is use-existing.",
                Some(Expression::String(String::new())),
                false,
            )));
            blocks.push(nested(variable_block(
                "subnet_name",
                "Existing subnet name. Required when network is use-existing.",
                Some(Expression::String(String::new())),
                false,
            )));
            blocks.push(nested(variable_block(
                "network_region",
                "Existing subnet region. Empty uses gcp_region.",
                Some(Expression::String(String::new())),
                false,
            )));
        }
    }
    if matches!(target.cloud_platform(), alien_core::Platform::Azure) {
        blocks.push(nested(variable_block(
            "azure_location",
            "Azure location.",
            Some(Expression::String("eastus".to_string())),
            false,
        )));
        blocks.push(nested(variable_block(
            "azure_subscription_id",
            "Azure subscription ID hosting the stack.",
            None,
            false,
        )));
        if target == TerraformTarget::Aks {
            blocks.push(nested(variable_block(
                "azure_tenant_id",
                "Azure tenant ID hosting the target AKS Kubernetes API identities.",
                None,
                false,
            )));
        }
        blocks.push(nested(variable_block(
            "azure_resource_group_name",
            "Azure resource group name.",
            None,
            false,
        )));
        if needs_azure_management_inputs {
            blocks.push(nested(variable_block(
                "azure_managing_tenant_id",
                "Azure tenant ID that hosts the management identity for cross-tenant access.",
                Some(Expression::String(String::new())),
                false,
            )));
            blocks.push(nested(variable_block(
                "azure_oidc_issuer",
                "OIDC issuer URL for Azure Federated Identity Credential.",
                Some(Expression::String(String::new())),
                false,
            )));
            blocks.push(nested(variable_block(
                "azure_oidc_subject",
                "OIDC subject claim for Azure Federated Identity Credential.",
                Some(Expression::String(String::new())),
                false,
            )));
        }
    }
    if target.is_kubernetes() {
        blocks.push(nested(variable_block(
            "kubernetes_cluster_mode",
            "Kubernetes cluster mode. Values: create or existing.",
            Some(Expression::String("create".to_string())),
            false,
        )));
        blocks.push(nested(variable_block(
            "kubernetes_namespace",
            "Kubernetes namespace for runtime resources.",
            Some(Expression::String("default".to_string())),
            false,
        )));
        if matches!(target, TerraformTarget::Eks) {
            let custom_domain_defaults =
                EksCustomDomainDefaults::from_settings(stack_settings.kubernetes.as_ref());
            blocks.push(nested(variable_block(
                "custom_domain_name",
                "Optional custom domain for public Kubernetes routes. Leave empty to use the generated load balancer hostname.",
                Some(Expression::String(
                    custom_domain_defaults.domain_name.unwrap_or_default(),
                )),
                false,
            )));
            blocks.push(nested(custom_domain_certificate_arn_variable_block(
                custom_domain_defaults.certificate_arn.unwrap_or_default(),
            )));
        }
    }
    if target.is_kubernetes() && registration.is_some() && helm_install.is_some() {
        blocks.push(nested(bool_variable_block(
            "helm_install_enabled",
            "Whether this module installs the runtime Helm chart after registering the deployment.",
            Some(true),
        )));
        blocks.push(nested(variable_block(
            "helm_release_name",
            "Helm release name used for the runtime chart.",
            Some(Expression::String(
                helm_install.expect("checked above").release_name.clone(),
            )),
            false,
        )));
        blocks.push(nested(variable_block(
            "helm_chart",
            "OCI Helm chart reference to install.",
            Some(Expression::String(
                helm_install.expect("checked above").chart_ref.clone(),
            )),
            false,
        )));
    }
    if matches!(target, TerraformTarget::Eks) {
        blocks.push(nested(variable_block(
            "eks_cluster_name",
            "Existing EKS cluster name that Helm will install into.",
            Some(Expression::String(String::new())),
            false,
        )));
    }
    if matches!(target, TerraformTarget::Gke) {
        blocks.push(nested(variable_block(
            "gke_cluster_name",
            "Existing GKE cluster name that Helm will install into.",
            Some(Expression::String(String::new())),
            false,
        )));
        blocks.push(nested(variable_block(
            "gke_cluster_location",
            "Existing GKE cluster location (region or zone).",
            Some(Expression::String(String::new())),
            false,
        )));
    }
    if matches!(target, TerraformTarget::Aks) {
        blocks.push(nested(variable_block(
            "aks_cluster_name",
            "Existing AKS cluster name that Helm will install into.",
            Some(Expression::String(String::new())),
            false,
        )));
        blocks.push(nested(variable_block(
            "aks_cluster_resource_group_name",
            "Resource group containing the existing AKS cluster.",
            Some(Expression::String(String::new())),
            false,
        )));
    }

    for (name, description, default) in &network_vars.extra_string_vars {
        blocks.push(nested(variable_block(
            name,
            description,
            default.clone(),
            false,
        )));
    }
    for (name, description, default) in &network_vars.extra_list_vars {
        blocks.push(nested(list_variable_block(
            name,
            description,
            default.clone(),
        )));
    }
    for input in stack_inputs {
        blocks.push(nested(stack_input_variable_block(input)));
    }

    Ok(Body::from(blocks))
}

pub(super) fn resource_prefix_variable_block() -> Block {
    let body = vec![
        attr("type", expr::raw("string")),
        attr(
            "description",
            Expression::String(
                "Optional stable physical resource prefix. Leave empty to generate one."
                    .to_string(),
            ),
        ),
        attr("default", Expression::String(String::new())),
        nested(block(
            "validation",
            [
                attr(
                    "condition",
                    expr::raw(
                        "var.resource_prefix == \"\" || (can(regex(\"^[a-z][a-z0-9-]{1,38}[a-z0-9]$\", var.resource_prefix)) && length(regexall(\"--\", var.resource_prefix)) == 0)",
                    ),
                ),
                attr(
                    "error_message",
                    Expression::String(
                        "resource_prefix must be 3-40 characters: lowercase letters, numbers, and hyphens; start with a letter; end with a letter or number; and not contain consecutive hyphens."
                            .to_string(),
                    ),
                ),
            ],
        )),
    ];
    Block {
        identifier: Identifier::sanitized("variable"),
        labels: vec![BlockLabel::String("resource_prefix".to_string())],
        body: Body::from(body),
    }
}

fn advanced_settings_default_json(
    target: TerraformTarget,
    stack_settings: &StackSettings,
) -> Result<String> {
    let mut value = serde_json::to_value(stack_settings)
        .into_alien_error()
        .map_err(|err| {
            AlienError::new(ErrorData::JsonSerializationFailed {
                reason: format!("failed to serialize StackSettings: {err}"),
            })
        })?;

    if let serde_json::Value::Object(ref mut object) = value {
        object.remove("deploymentModel");
        object.remove("updates");
        object.remove("telemetry");
        object.remove("heartbeats");
        if (matches!(target.cloud_platform(), alien_core::Platform::Aws)
            && has_dynamic_aws_network_settings(stack_settings.network.as_ref()))
            || (matches!(target.cloud_platform(), alien_core::Platform::Gcp)
                && has_dynamic_gcp_network_settings(stack_settings.network.as_ref()))
        {
            object.remove("network");
        }
        if target.is_kubernetes() {
            remove_kubernetes_exposure_default(object);
        }
    }

    serde_json::to_string(&sort_json_object_keys(value))
        .into_alien_error()
        .map_err(|err| {
            AlienError::new(ErrorData::JsonSerializationFailed {
                reason: format!("failed to serialize advanced settings default: {err}"),
            })
        })
}

fn sort_json_object_keys(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .into_iter()
                .map(sort_json_object_keys)
                .collect::<Vec<_>>(),
        ),
        serde_json::Value::Object(map) => {
            let mut entries = map.into_iter().collect::<Vec<_>>();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));

            let mut sorted = serde_json::Map::new();
            for (key, value) in entries {
                sorted.insert(key, sort_json_object_keys(value));
            }
            serde_json::Value::Object(sorted)
        }
        value => value,
    }
}

fn remove_kubernetes_exposure_default(object: &mut serde_json::Map<String, serde_json::Value>) {
    let Some(serde_json::Value::Object(kubernetes)) = object.get_mut("kubernetes") else {
        return;
    };
    kubernetes.remove("exposure");
    if kubernetes.is_empty() {
        object.remove("kubernetes");
    }
}

#[derive(Default)]
struct EksCustomDomainDefaults {
    domain_name: Option<String>,
    certificate_arn: Option<String>,
}

impl EksCustomDomainDefaults {
    fn from_settings(settings: Option<&KubernetesSettings>) -> Self {
        let Some(KubernetesSettings {
            exposure:
                Some(KubernetesExposureSettings::Custom {
                    domain,
                    certificate,
                    ..
                }),
            ..
        }) = settings
        else {
            return Self::default();
        };

        Self {
            domain_name: Some(domain.clone()),
            certificate_arn: match certificate {
                KubernetesCertificateMode::AwsAcmArn { certificate_arn } => {
                    Some(certificate_arn.clone())
                }
                _ => None,
            },
        }
    }
}

fn aws_region_variable_block(supported_aws_regions: &[String]) -> Block {
    let default_region = supported_aws_regions
        .first()
        .cloned()
        .unwrap_or_else(|| "us-east-1".to_string());
    let mut body: Vec<Structure> = vec![
        attr("type", expr::raw("string")),
        attr(
            "description",
            Expression::String("AWS region used by the AWS provider.".to_string()),
        ),
        attr("default", Expression::String(default_region)),
    ];

    if !supported_aws_regions.is_empty() {
        let supported = Expression::Array(
            supported_aws_regions
                .iter()
                .cloned()
                .map(Expression::String)
                .collect(),
        );
        let regions = supported_aws_regions.join(", ");
        body.push(nested(block(
            "validation",
            [
                attr(
                    "condition",
                    Expression::FuncCall(Box::new(
                        hcl::expr::FuncCall::builder(Identifier::sanitized("contains"))
                            .arg(supported)
                            .arg(expr::raw("var.aws_region"))
                            .build(),
                    )),
                ),
                attr(
                    "error_message",
                    Expression::String(format!(
                        "aws_region must be one of the AWS regions supported by this environment: {regions}."
                    )),
                ),
            ],
        )));
    }

    Block {
        identifier: Identifier::sanitized("variable"),
        labels: vec![BlockLabel::String("aws_region".to_string())],
        body: Body::from(body),
    }
}

fn list_variable_block(name: &str, description: &str, default: Option<Vec<String>>) -> Block {
    let mut body: Vec<Structure> = vec![
        attr("type", expr::raw("list(string)")),
        attr("description", Expression::String(description.to_string())),
    ];
    if let Some(default) = default {
        body.push(attr(
            "default",
            Expression::Array(default.into_iter().map(Expression::String).collect()),
        ));
    }
    Block {
        identifier: Identifier::sanitized("variable"),
        labels: vec![BlockLabel::String(name.to_string())],
        body: Body::from(body),
    }
}

fn number_variable_block(name: &str, description: &str, default: Option<i64>) -> Block {
    let mut body: Vec<Structure> = vec![
        attr("type", expr::raw("number")),
        attr("description", Expression::String(description.to_string())),
    ];
    if let Some(default) = default {
        body.push(attr(
            "default",
            Expression::Number(hcl::Number::from(default)),
        ));
    }
    Block {
        identifier: Identifier::sanitized("variable"),
        labels: vec![BlockLabel::String(name.to_string())],
        body: Body::from(body),
    }
}

fn bool_variable_block(name: &str, description: &str, default: Option<bool>) -> Block {
    let mut body: Vec<Structure> = vec![
        attr("type", expr::raw("bool")),
        attr("description", Expression::String(description.to_string())),
    ];
    if let Some(default) = default {
        body.push(attr("default", Expression::Bool(default)));
    }
    Block {
        identifier: Identifier::sanitized("variable"),
        labels: vec![BlockLabel::String(name.to_string())],
        body: Body::from(body),
    }
}

fn string_enum_variable_block(
    name: &str,
    description: &str,
    default: &str,
    allowed_values: &[&str],
) -> Block {
    let allowed = Expression::Array(
        allowed_values
            .iter()
            .map(|value| Expression::String((*value).to_string()))
            .collect(),
    );
    let allowed_text = allowed_values.join(", ");
    Block {
        identifier: Identifier::sanitized("variable"),
        labels: vec![BlockLabel::String(name.to_string())],
        body: Body::from(vec![
            attr("type", expr::raw("string")),
            attr("description", Expression::String(description.to_string())),
            attr("default", Expression::String(default.to_string())),
            nested(block(
                "validation",
                [
                    attr(
                        "condition",
                        Expression::FuncCall(Box::new(
                            hcl::expr::FuncCall::builder(Identifier::sanitized("contains"))
                                .arg(allowed)
                                .arg(expr::raw(format!("var.{name}")))
                                .build(),
                        )),
                    ),
                    attr(
                        "error_message",
                        Expression::String(format!("{name} must be one of: {allowed_text}.")),
                    ),
                ],
            )),
        ]),
    }
}

fn variable_block(
    name: &str,
    description: &str,
    default: Option<Expression>,
    sensitive: bool,
) -> Block {
    let mut body: Vec<Structure> = vec![
        attr("type", expr::raw("string")),
        attr("description", Expression::String(description.to_string())),
    ];
    if let Some(default) = default {
        body.push(attr("default", default));
    }
    if sensitive {
        body.push(attr("sensitive", Expression::Bool(true)));
    }
    Block {
        identifier: Identifier::sanitized("variable"),
        labels: vec![BlockLabel::String(name.to_string())],
        body: Body::from(body),
    }
}

fn stack_input_variable_block(input: &StackInputDefinition) -> Block {
    let variable_name = terraform_stack_input_variable_name(input);
    let mut body: Vec<Structure> = vec![
        attr("type", stack_input_terraform_type(input)),
        attr(
            "description",
            Expression::String(format!("{} {}", input.label, input.description)),
        ),
    ];
    if let Some(default) = input.default.as_ref().map(stack_input_default_expression) {
        body.push(attr("default", default));
    } else if !input.required {
        body.push(attr("default", expr::raw("null")));
        body.push(attr("nullable", Expression::Bool(true)));
    }
    if input.kind == StackInputKind::Secret {
        body.push(attr("sensitive", Expression::Bool(true)));
    }
    for condition in stack_input_validation_conditions(input, &variable_name) {
        body.push(nested(block(
            "validation",
            [
                attr("condition", expr::raw(condition)),
                attr(
                    "error_message",
                    Expression::String(stack_input_validation_message(input)),
                ),
            ],
        )));
    }
    Block {
        identifier: Identifier::sanitized("variable"),
        labels: vec![BlockLabel::String(variable_name)],
        body: Body::from(body),
    }
}

fn stack_input_terraform_type(input: &StackInputDefinition) -> Expression {
    match input.kind {
        StackInputKind::Number | StackInputKind::Integer => expr::raw("number"),
        StackInputKind::Boolean => expr::raw("bool"),
        StackInputKind::StringList => expr::raw("list(string)"),
        StackInputKind::String | StackInputKind::Secret | StackInputKind::Enum => {
            expr::raw("string")
        }
    }
}

fn stack_input_default_expression(default: &StackInputDefaultValue) -> Expression {
    match default {
        StackInputDefaultValue::String(value) => Expression::String(value.clone()),
        StackInputDefaultValue::Number(value) => expr::raw(value),
        StackInputDefaultValue::Boolean(value) => Expression::Bool(*value),
        StackInputDefaultValue::StringList(values) => {
            Expression::Array(values.iter().cloned().map(Expression::String).collect())
        }
    }
}

fn stack_input_validation_conditions(
    input: &StackInputDefinition,
    variable_name: &str,
) -> Vec<String> {
    let mut conditions = Vec::new();
    let variable = format!("var.{variable_name}");
    let optional_prefix = (!input.required && input.default.is_none())
        .then(|| format!("{variable} == null || "))
        .unwrap_or_default();

    if input.kind == StackInputKind::Integer {
        conditions.push(format!("{optional_prefix}floor({variable}) == {variable}"));
    }

    if let Some(validation) = &input.validation {
        add_stack_input_validation_conditions(
            validation,
            input,
            &variable,
            &optional_prefix,
            &mut conditions,
        );
    }

    conditions
}

fn add_stack_input_validation_conditions(
    validation: &StackInputValidation,
    input: &StackInputDefinition,
    variable: &str,
    optional_prefix: &str,
    conditions: &mut Vec<String>,
) {
    if let Some(values) = &validation.values {
        let allowed = hcl_string_array(values);
        conditions.push(format!("{optional_prefix}contains({allowed}, {variable})"));
    }
    if let Some(pattern) = &validation.pattern {
        let whole_pattern = format!("^(?:{pattern})$");
        conditions.push(format!(
            "{optional_prefix}can(regex({}, {variable}))",
            hcl_string(&whole_pattern)
        ));
    }
    if matches!(
        input.kind,
        StackInputKind::String | StackInputKind::Secret | StackInputKind::Enum
    ) {
        if let Some(min) = validation.min_length {
            conditions.push(format!("{optional_prefix}length({variable}) >= {min}"));
        }
        if let Some(max) = validation.max_length {
            conditions.push(format!("{optional_prefix}length({variable}) <= {max}"));
        }
    }
    if matches!(input.kind, StackInputKind::Number | StackInputKind::Integer) {
        if let Some(min) = &validation.min {
            conditions.push(format!("{optional_prefix}{variable} >= {min}"));
        }
        if let Some(max) = &validation.max {
            conditions.push(format!("{optional_prefix}{variable} <= {max}"));
        }
    }
    if input.kind == StackInputKind::StringList {
        if let Some(min) = validation.min_items {
            conditions.push(format!("{optional_prefix}length({variable}) >= {min}"));
        }
        if let Some(max) = validation.max_items {
            conditions.push(format!("{optional_prefix}length({variable}) <= {max}"));
        }
    }
}

fn stack_input_validation_message(input: &StackInputDefinition) -> String {
    format!("{} is invalid. {}", input.label, input.description)
}

fn hcl_string(value: &str) -> String {
    serde_json::to_string(value).expect("serializing string literal should not fail")
}

fn hcl_string_array(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| hcl_string(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

fn custom_domain_certificate_arn_variable_block(default: String) -> Block {
    Block {
        identifier: Identifier::sanitized("variable"),
        labels: vec![BlockLabel::String(
            "custom_domain_certificate_arn".to_string(),
        )],
        body: Body::from(vec![
            attr("type", expr::raw("string")),
            attr(
                "description",
                Expression::String(
                    "ACM certificate ARN for custom_domain_name. Required when custom_domain_name is set."
                        .to_string(),
                ),
            ),
            attr("default", Expression::String(default)),
            nested(block(
                "validation",
                [
                    attr(
                        "condition",
                        expr::raw(
                            r#"var.custom_domain_certificate_arn == "" ? var.custom_domain_name == "" : can(regex("^arn:aws(-[a-z]+)?:acm:[a-z0-9-]+:[0-9]{12}:certificate/.+$", var.custom_domain_certificate_arn))"#,
                        ),
                    ),
                    attr(
                        "error_message",
                        Expression::String(
                            "custom_domain_certificate_arn must be a valid AWS ACM certificate ARN when custom_domain_name is set."
                                .to_string(),
                        ),
                    ),
                ],
            )),
        ]),
    }
}
