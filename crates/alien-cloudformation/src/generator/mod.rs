mod expressions;
mod parameters;

#[cfg(test)]
mod tests;

use crate::{
    registry::CfRegistry,
    template::{CfExpression, CfMapping, CfParameter, CfResource, CfTemplate},
};
use alien_core::{
    import::{EmitContext, CURRENT_SETUP_IMPORT_FORMAT_VERSION},
    ownership_policy_for_resource_type, CapacityGroup, ComputeCluster, ComputePoolSelection,
    DeploymentModel, ErrorData, KubernetesCluster, NetworkSettings, Platform, Result, Stack,
    StackInputDefaultValue, StackInputDefinition, StackInputKind, StackInputProvider,
    StackSettings, Worker, WorkerCode,
};
use alien_error::AlienError;
use expressions::{
    domains_expression, kubernetes_settings_expression, management_config_expression,
    network_expression, output,
};
use indexmap::{indexmap, IndexMap};
use parameters::{
    add_custom_domain_certificate_rule, add_standard_conditions, add_standard_parameters,
    add_supported_region_rule,
};
use serde_json::{json, Value};
use std::collections::HashSet;

const TEMPLATE_VERSION: &str = "2010-09-09";
const LANGUAGE_EXTENSIONS_TRANSFORM: &str = "AWS::LanguageExtensions";

const PARAM_TOKEN: &str = "Token";
const PARAM_MANAGING_ROLE_ARN: &str = "ManagingRoleArn";
const PARAM_MANAGING_ACCOUNT_ID: &str = "ManagingAccountId";
const PARAM_NETWORK_MODE: &str = "NetworkMode";
const PARAM_VPC_CIDR: &str = "VpcCidr";
const PARAM_AVAILABILITY_ZONES: &str = "AvailabilityZones";
const PARAM_VPC_ID: &str = "VpcId";
const PARAM_PUBLIC_SUBNET_IDS: &str = "PublicSubnetIds";
const PARAM_PRIVATE_SUBNET_IDS: &str = "PrivateSubnetIds";
const PARAM_SECURITY_GROUP_IDS: &str = "SecurityGroupIds";
const PARAM_DOMAIN_NAME: &str = "DomainName";
const PARAM_HOSTED_ZONE_ID: &str = "HostedZoneId";
const PARAM_CERTIFICATE_ARN: &str = "CertificateArn";
const PARAM_UPDATES_MODE: &str = "UpdatesMode";
const PARAM_TELEMETRY_MODE: &str = "TelemetryMode";
const PARAM_HEARTBEATS_MODE: &str = "HeartbeatsMode";

const CONDITION_NETWORK_CREATE_AZ2: &str = "NetworkCreateUseAz2";
const CONDITION_NETWORK_CREATE_AZ3: &str = "NetworkCreateUseAz3";
const CONDITION_NETWORK_AZ2: &str = "NetworkUseAz2";
const CONDITION_NETWORK_AZ3: &str = "NetworkUseAz3";
const CONDITION_NETWORK_MODE_CREATE: &str = "NetworkModeCreate";
const CONDITION_NETWORK_MODE_USE_EXISTING: &str = "NetworkModeUseExisting";
const CONDITION_HAS_VPC_CIDR: &str = "HasVpcCidr";
const CONDITION_HAS_DOMAIN_NAME: &str = "HasDomainName";

const OUTPUT_SOURCE_KIND: &str = "DeploymentSourceKind";
const OUTPUT_DEPLOYMENT_ID: &str = "DeploymentId";
const OUTPUT_RESOURCE_PREFIX: &str = "DeploymentResourcePrefix";
const OUTPUT_PLATFORM: &str = "DeploymentPlatform";
const OUTPUT_BASE_PLATFORM: &str = "DeploymentBasePlatform";
const OUTPUT_REGION: &str = "DeploymentRegion";
const OUTPUT_SETUP_TARGET: &str = "DeploymentSetupTarget";
const OUTPUT_SETUP_IMPORT_FORMAT_VERSION: &str = "DeploymentSetupImportFormatVersion";
const OUTPUT_SETUP_FINGERPRINT: &str = "DeploymentSetupFingerprint";
const OUTPUT_SETUP_FINGERPRINT_VERSION: &str = "DeploymentSetupFingerprintVersion";
const OUTPUT_MANAGEMENT_CONFIG: &str = "DeploymentManagementConfig";
const OUTPUT_STACK_SETTINGS: &str = "DeploymentStackSettings";
const OUTPUT_RESOURCES: &str = "DeploymentResources";
const OUTPUT_RESOURCES_CHUNK_BYTES: usize = 3_500;
const STANDARD_OUTPUT_COUNT: usize = 12;
const CLOUDFORMATION_MAX_OUTPUTS: usize = 200;
const MAPPING_REGIONAL_CUSTOM_RESOURCE_SERVICE_TOKENS: &str = "RegionalCustomResourceServiceTokens";
const MAPPING_SERVICE_TOKEN_KEY: &str = "ServiceToken";
const RULE_SUPPORTED_AWS_REGION: &str = "SupportedAwsRegion";
const RULE_CUSTOM_DOMAIN_CERTIFICATE: &str = "CustomDomainCertificate";

/// Registration behavior for the generated CloudFormation template.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistrationMode {
    /// Register through a CloudFormation custom resource.
    CustomResource {
        lambda_arn: String,
        callback_url: Option<String>,
    },
    /// Register through a same-region CloudFormation custom resource.
    RegionalCustomResource {
        lambda_arns_by_region: IndexMap<String, String>,
        callback_url: Option<String>,
    },
    /// Emit stack outputs that can be registered out of band.
    OutputsFallback,
    /// Emit both the custom resource and stack outputs.
    Both {
        lambda_arn: String,
        callback_url: Option<String>,
    },
    /// Emit both a same-region custom resource and stack outputs.
    RegionalBoth {
        lambda_arns_by_region: IndexMap<String, String>,
        callback_url: Option<String>,
    },
}

impl RegistrationMode {
    fn service_token(&self, template: &mut CfTemplate) -> Result<Option<CfExpression>> {
        match self {
            RegistrationMode::CustomResource { lambda_arn, .. }
            | RegistrationMode::Both { lambda_arn, .. } => {
                Ok(Some(CfExpression::from(lambda_arn.clone())))
            }
            RegistrationMode::RegionalCustomResource {
                lambda_arns_by_region,
                ..
            }
            | RegistrationMode::RegionalBoth {
                lambda_arns_by_region,
                ..
            } => regional_service_token(template, lambda_arns_by_region).map(Some),
            RegistrationMode::OutputsFallback => Ok(None),
        }
    }

    fn emits_outputs(&self) -> bool {
        matches!(
            self,
            RegistrationMode::OutputsFallback
                | RegistrationMode::Both { .. }
                | RegistrationMode::RegionalBoth { .. }
        )
    }

    fn callback_url(&self) -> Option<&str> {
        match self {
            RegistrationMode::CustomResource { callback_url, .. }
            | RegistrationMode::RegionalCustomResource { callback_url, .. }
            | RegistrationMode::Both { callback_url, .. }
            | RegistrationMode::RegionalBoth { callback_url, .. } => callback_url.as_deref(),
            RegistrationMode::OutputsFallback => None,
        }
    }

    pub fn supported_regions(&self) -> Vec<String> {
        match self {
            RegistrationMode::RegionalCustomResource {
                lambda_arns_by_region,
                ..
            }
            | RegistrationMode::RegionalBoth {
                lambda_arns_by_region,
                ..
            } => lambda_arns_by_region.keys().cloned().collect(),
            RegistrationMode::CustomResource { .. }
            | RegistrationMode::Both { .. }
            | RegistrationMode::OutputsFallback => Vec::new(),
        }
    }
}

/// Options for CloudFormation generation.
pub struct CloudFormationOptions<'a> {
    /// Per-`(ResourceType, Platform)` emitter dispatch. Most callers pass
    /// [`CfRegistry::built_in()`]; plugin-aware callers extend it before
    /// passing.
    pub registry: &'a CfRegistry,
    pub target: CloudFormationTarget,
    pub stack_settings: StackSettings,
    pub setup_target: String,
    pub setup_fingerprint: String,
    pub setup_fingerprint_version: u32,
    pub registration: RegistrationMode,
    pub description: Option<String>,
}

/// CloudFormation setup target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloudFormationTarget {
    Aws,
    Eks,
}

impl CloudFormationTarget {
    /// The cloud platform whose CloudFormation emitters back this target.
    pub fn cloud_platform(self) -> Platform {
        match self {
            CloudFormationTarget::Aws | CloudFormationTarget::Eks => Platform::Aws,
        }
    }

    /// Stable target name used in setup metadata and package outputs.
    pub fn name(self) -> &'static str {
        match self {
            CloudFormationTarget::Aws => "aws",
            CloudFormationTarget::Eks => "eks",
        }
    }

    fn deployment_platform(self) -> Platform {
        match self {
            CloudFormationTarget::Aws => Platform::Aws,
            CloudFormationTarget::Eks => Platform::Kubernetes,
        }
    }

    fn base_platform(self) -> Option<Platform> {
        match self {
            CloudFormationTarget::Aws => None,
            CloudFormationTarget::Eks => Some(Platform::Aws),
        }
    }

    fn is_kubernetes(self) -> bool {
        matches!(self, CloudFormationTarget::Eks)
    }
}

/// Generate a CloudFormation template for a stack.
pub fn generate_cloudformation_template(
    stack: &Stack,
    options: CloudFormationOptions<'_>,
) -> Result<CfTemplate> {
    validate_stack_for_cloudformation(stack)?;
    validate_stack_settings(&options.stack_settings)?;

    let mut stack_settings = options.stack_settings.clone();
    // CloudFormation packages always register push deployments.
    stack_settings.deployment_model = DeploymentModel::Push;
    if options.target.is_kubernetes() && stack_settings.network.is_none() {
        stack_settings.network = Some(NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        });
    }

    let names = logical_names(stack)?;
    let mut template = CfTemplate {
        aws_template_format_version: TEMPLATE_VERSION.to_string(),
        description: Some(
            options
                .description
                .clone()
                .unwrap_or_else(|| format!("Application setup stack for {}", stack.id())),
        ),
        transform: vec![LANGUAGE_EXTENSIONS_TRANSFORM.to_string()],
        metadata: IndexMap::new(),
        parameters: IndexMap::new(),
        mappings: IndexMap::new(),
        conditions: IndexMap::new(),
        rules: IndexMap::new(),
        resources: IndexMap::new(),
        outputs: IndexMap::new(),
    };

    let supports_custom_domain = stack_supports_custom_domain(stack, options.target);
    let stack_inputs = stack_inputs_for_cloudformation(stack, options.target);

    add_standard_parameters(
        &mut template,
        stack,
        &stack_settings,
        supports_custom_domain,
    )?;
    add_stack_input_parameters(&mut template, &stack_inputs);
    add_supported_region_rule(&mut template, &options.registration);
    if supports_custom_domain {
        add_custom_domain_certificate_rule(&mut template);
    }
    add_standard_conditions(
        &mut template,
        stack,
        &stack_settings,
        supports_custom_domain,
    );
    add_console_interface_metadata(
        &mut template,
        stack,
        &stack_settings,
        supports_custom_domain,
        &stack_inputs,
    );

    let mut registration_resources = Vec::new();
    let mut emitted_resource_ids: IndexMap<String, Vec<String>> = IndexMap::new();

    for (resource_id, resource) in stack.resources() {
        let resource_type = resource.config.resource_type();
        let ownership = ownership_policy_for_resource_type(resource_type.as_ref());
        if !ownership.should_emit_in_setup(resource.lifecycle) {
            continue;
        }
        let emitter = options
            .registry
            .require(&resource_type, options.target.cloud_platform())?;

        let ctx = EmitContext {
            stack,
            resource,
            resource_id,
            platform: options.target.cloud_platform(),
            stack_settings: &stack_settings,
            names: &names,
        };

        let emitted_resources = emitter.emit_resources_with_registry(&ctx, options.registry)?;
        emitted_resource_ids.insert(
            resource_id.clone(),
            emitted_resources
                .iter()
                .map(|resource| resource.logical_id.clone())
                .collect(),
        );

        for emitted in emitted_resources {
            insert_resource(&mut template, emitted)?;
        }

        let registration_data = emitter.emit_import_ref(&ctx)?;
        registration_resources.push(CfExpression::object([
            ("id", CfExpression::from(resource_id.as_str())),
            ("type", CfExpression::from(resource_type.as_ref())),
            ("importData", registration_data),
        ]));
    }

    let kubernetes_namespace = if options.target.is_kubernetes() {
        kubernetes_cluster_namespace(stack).map(CfExpression::from)
    } else {
        None
    };

    let management_config = management_config_expression(options.target);
    let stack_settings = stack_settings_expression(
        options.target,
        stack,
        &stack_settings,
        kubernetes_namespace.clone(),
        supports_custom_domain,
    );
    let resources = CfExpression::list(registration_resources);

    apply_resource_dependencies(stack, &emitted_resource_ids, &mut template);

    if let Some(service_token) = options.registration.service_token(&mut template)? {
        add_custom_resource(
            &mut template,
            service_token,
            management_config.clone(),
            stack_settings.clone(),
            &options,
            resources.clone(),
            stack_input_values_expression(&stack_inputs),
            options.registration.callback_url(),
        );
    }

    if options.registration.emits_outputs() {
        add_outputs(
            &mut template,
            management_config,
            stack_settings,
            &options,
            resources,
        )?;
    }

    Ok(template)
}

fn stack_supports_custom_domain(stack: &Stack, target: CloudFormationTarget) -> bool {
    target.is_kubernetes()
        || stack.resources().any(|(_resource_id, resource)| {
            resource
                .config
                .downcast_ref::<Worker>()
                .is_some_and(|worker| !worker.public_endpoints.is_empty())
        })
}

fn stack_inputs_for_cloudformation(
    stack: &Stack,
    target: CloudFormationTarget,
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

fn stack_input_parameter_name(input: &StackInputDefinition) -> String {
    format!("Input{}", sanitize_logical_id(&input.id))
}

fn add_stack_input_parameters(template: &mut CfTemplate, inputs: &[StackInputDefinition]) {
    for input in inputs {
        template.parameters.insert(
            stack_input_parameter_name(input),
            stack_input_parameter(input),
        );
    }
}

fn stack_input_parameter(input: &StackInputDefinition) -> CfParameter {
    let validation = input.validation.as_ref();
    let mut parameter = CfParameter {
        parameter_type: match input.kind {
            StackInputKind::Number => "Number".to_string(),
            StackInputKind::StringList => "CommaDelimitedList".to_string(),
            StackInputKind::String
            | StackInputKind::Secret
            | StackInputKind::Integer
            | StackInputKind::Boolean
            | StackInputKind::Enum => "String".to_string(),
        },
        description: Some(input.description.clone()),
        default: input.default.as_ref().map(stack_input_default_expression),
        allowed_values: validation
            .and_then(|validation| validation.values.as_ref())
            .map(|values| values.iter().cloned().map(CfExpression::from).collect()),
        allowed_pattern: validation
            .and_then(|validation| validation.pattern.clone())
            .or_else(|| {
                matches!(input.kind, StackInputKind::Integer).then(|| "^-?[0-9]+$".to_string())
            }),
        min_length: validation.and_then(|validation| validation.min_length),
        max_length: validation.and_then(|validation| validation.max_length),
        min_value: validation
            .and_then(|validation| validation.min.as_deref())
            .map(number_constraint_expression),
        max_value: validation
            .and_then(|validation| validation.max.as_deref())
            .map(number_constraint_expression),
        no_echo: matches!(input.kind, StackInputKind::Secret).then_some(true),
    };

    if matches!(input.kind, StackInputKind::Boolean) {
        parameter.allowed_values = Some(vec![
            CfExpression::from("true"),
            CfExpression::from("false"),
        ]);
    }

    parameter
}

fn number_constraint_expression(value: &str) -> CfExpression {
    value
        .parse::<i64>()
        .map(CfExpression::Integer)
        .or_else(|_| value.parse::<f64>().map(CfExpression::Number))
        .unwrap_or_else(|_| CfExpression::from(value))
}

fn stack_input_default_expression(default: &StackInputDefaultValue) -> CfExpression {
    match default {
        StackInputDefaultValue::String(value) | StackInputDefaultValue::Number(value) => {
            CfExpression::from(value.clone())
        }
        StackInputDefaultValue::Boolean(value) => CfExpression::from(value.to_string()),
        StackInputDefaultValue::StringList(values) => CfExpression::from(values.join(",")),
    }
}

fn stack_input_values_expression(inputs: &[StackInputDefinition]) -> CfExpression {
    CfExpression::object(inputs.iter().map(|input| {
        (
            input.id.clone(),
            CfExpression::ref_(stack_input_parameter_name(input)),
        )
    }))
}

fn regional_service_token(
    template: &mut CfTemplate,
    lambda_arns_by_region: &IndexMap<String, String>,
) -> Result<CfExpression> {
    if lambda_arns_by_region.is_empty() {
        return Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "generate_cloudformation_template".to_string(),
            reason: "regional custom resource registration requires at least one region"
                .to_string(),
        }));
    }

    let mut mapping = CfMapping::new();
    for (region, lambda_arn) in lambda_arns_by_region {
        mapping.insert(
            region.clone(),
            indexmap! {
                MAPPING_SERVICE_TOKEN_KEY.to_string() => CfExpression::from(lambda_arn.clone()),
            },
        );
    }
    template.mappings.insert(
        MAPPING_REGIONAL_CUSTOM_RESOURCE_SERVICE_TOKENS.to_string(),
        mapping,
    );

    Ok(CfExpression::find_in_map(
        MAPPING_REGIONAL_CUSTOM_RESOURCE_SERVICE_TOKENS,
        CfExpression::ref_("AWS::Region"),
        MAPPING_SERVICE_TOKEN_KEY,
    ))
}

/// Serialize a CloudFormation template to YAML.
pub fn to_yaml(template: &CfTemplate) -> Result<String> {
    let mut template = template.clone();
    sort_template_metadata(&mut template);

    let yaml = serde_yaml::to_string(&template).map_err(|error| {
        AlienError::new(ErrorData::TemplateSerializationFailed {
            format: "CloudFormation YAML".to_string(),
            reason: error.to_string(),
        })
    })?;

    Ok(quote_yaml_1_1_mode_scalars(&yaml))
}

fn sort_template_metadata(template: &mut CfTemplate) {
    for value in template.metadata.values_mut() {
        sort_json_value(value);
    }
    for resource in template.resources.values_mut() {
        for value in resource.metadata.values_mut() {
            sort_json_value(value);
        }
    }
}

fn sort_json_value(value: &mut Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                sort_json_value(value);
            }
        }
        Value::Object(values) => {
            let mut entries = std::mem::take(values).into_iter().collect::<Vec<_>>();
            entries.sort_by(|(left, _), (right, _)| left.cmp(right));

            for (_, value) in &mut entries {
                sort_json_value(value);
            }

            values.extend(entries);
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn quote_yaml_1_1_mode_scalars(yaml: &str) -> String {
    let mut quoted = String::with_capacity(yaml.len());
    for line in yaml.lines() {
        let trimmed = line.trim_start();
        let indent = &line[..line.len() - trimmed.len()];
        let replacement = match trimmed {
            "Default: on" => Some("Default: \"on\""),
            "Default: off" => Some("Default: \"off\""),
            "- on" => Some("- \"on\""),
            "- off" => Some("- \"off\""),
            _ => None,
        };

        if let Some(replacement) = replacement {
            quoted.push_str(indent);
            quoted.push_str(replacement);
        } else {
            quoted.push_str(line);
        }
        quoted.push('\n');
    }

    if !yaml.ends_with('\n') {
        quoted.pop();
    }

    quoted
}

/// Generate the baseline CloudFormation stack policy.
///
/// Runtime-managed resources are not part of the setup stack, so the baseline
/// policy is equivalent to CloudFormation's behavior when no policy is set.
/// An empty statement list is not valid when supplied through `StackPolicyURL`.
pub fn generate_cloudformation_stack_policy(_stack: &Stack) -> Result<serde_json::Value> {
    Ok(json!({
        "Statement": [{
            "Effect": "Allow",
            "Action": "Update:*",
            "Principal": "*",
            "Resource": "*"
        }]
    }))
}

fn validate_stack_for_cloudformation(stack: &Stack) -> Result<()> {
    for (resource_id, resource) in stack.resources() {
        if let Some(function) = resource.config.downcast_ref::<Worker>() {
            if matches!(function.code, WorkerCode::Source { .. }) {
                return Err(AlienError::new(ErrorData::OperationNotSupported {
                    operation: "generate_cloudformation_template".to_string(),
                    reason: format!(
                        "function '{resource_id}' uses source code; CloudFormation templates require a pre-built image"
                    ),
                }));
            }
        }
    }

    Ok(())
}

fn validate_stack_settings(settings: &StackSettings) -> Result<()> {
    if settings.external_bindings.is_some() {
        return Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "generate_cloudformation_template".to_string(),
            reason: "CloudFormation templates do not accept external bindings".to_string(),
        }));
    }

    if matches!(
        settings.network,
        Some(NetworkSettings::ByoVpcGcp { .. } | NetworkSettings::ByoVnetAzure { .. })
    ) {
        return Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "generate_cloudformation_template".to_string(),
            reason: "CloudFormation templates support only AWS network settings".to_string(),
        }));
    }

    Ok(())
}

fn logical_names(stack: &Stack) -> Result<IndexMap<String, String>> {
    let mut names = IndexMap::new();
    let mut used = HashSet::new();

    for (resource_id, resource) in stack.resources() {
        let mut base = sanitize_logical_id(resource.config.id());
        if base.is_empty() {
            base = sanitize_logical_id(resource_id);
        }

        let mut candidate = base.clone();
        let mut suffix = 2usize;
        while used.contains(&candidate) {
            candidate = format!("{base}{suffix}");
            suffix += 1;
        }

        used.insert(candidate.clone());
        names.insert(resource_id.clone(), candidate);
    }

    Ok(names)
}

fn sanitize_logical_id(input: &str) -> String {
    let mut out = String::new();
    let mut capitalize_next = true;

    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            if capitalize_next {
                out.push(ch.to_ascii_uppercase());
                capitalize_next = false;
            } else {
                out.push(ch);
            }
        } else {
            capitalize_next = true;
        }
    }

    if out
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_digit())
    {
        out.insert_str(0, "Resource");
    }

    out
}

fn insert_resource(template: &mut CfTemplate, resource: CfResource) -> Result<()> {
    if template.resources.contains_key(&resource.logical_id) {
        return Err(AlienError::new(ErrorData::GenericError {
            message: format!(
                "duplicate CloudFormation logical id '{}'",
                resource.logical_id
            ),
        }));
    }

    template
        .resources
        .insert(resource.logical_id.clone(), resource);
    Ok(())
}

fn apply_resource_dependencies(
    stack: &Stack,
    emitted_resource_ids: &IndexMap<String, Vec<String>>,
    template: &mut CfTemplate,
) {
    let dependency_targets: IndexMap<String, Vec<String>> = emitted_resource_ids
        .iter()
        .map(|(resource_id, logical_ids)| {
            let targets = logical_ids
                .iter()
                .filter(|logical_id| {
                    template
                        .resources
                        .get(*logical_id)
                        .is_some_and(|resource| resource.condition.is_none())
                })
                .cloned()
                .collect();
            (resource_id.clone(), targets)
        })
        .collect();

    for (resource_id, entry) in stack.resources() {
        let Some(resource_logical_ids) = emitted_resource_ids.get(resource_id) else {
            continue;
        };

        let mut depends_on = Vec::new();
        for dependency in &entry.dependencies {
            if dependency.id() == resource_id {
                continue;
            }
            if let Some(targets) = dependency_targets.get(dependency.id()) {
                for target in targets {
                    if !depends_on.contains(target) {
                        depends_on.push(target.clone());
                    }
                }
            }
        }

        if depends_on.is_empty() {
            continue;
        }

        for logical_id in resource_logical_ids {
            let Some(resource) = template.resources.get_mut(logical_id) else {
                continue;
            };
            for dependency in &depends_on {
                if !resource.depends_on.contains(dependency) {
                    resource.depends_on.push(dependency.clone());
                }
            }
        }
    }
}

fn add_console_interface_metadata(
    template: &mut CfTemplate,
    stack: &Stack,
    settings: &StackSettings,
    supports_custom_domain: bool,
    stack_inputs: &[StackInputDefinition],
) {
    let network_parameters = network_parameter_names(settings.network.as_ref());
    let compute_parameters = compute_parameter_names(stack, settings.compute.as_ref());
    let mut parameter_groups = vec![json!({
        "Label": { "default": "Registration" },
        "Parameters": [
            PARAM_TOKEN,
            PARAM_MANAGING_ROLE_ARN,
            PARAM_MANAGING_ACCOUNT_ID,
        ]
    })];
    if !network_parameters.is_empty() {
        parameter_groups.push(json!({
            "Label": { "default": "Network" },
            "Parameters": network_parameters
        }));
    }
    if supports_custom_domain {
        parameter_groups.push(json!({
            "Label": { "default": "Custom domain" },
            "Parameters": [PARAM_DOMAIN_NAME, PARAM_HOSTED_ZONE_ID, PARAM_CERTIFICATE_ARN]
        }));
    }
    if !stack_inputs.is_empty() {
        parameter_groups.push(json!({
            "Label": { "default": "Application inputs" },
            "Parameters": stack_inputs
                .iter()
                .map(stack_input_parameter_name)
                .collect::<Vec<_>>()
        }));
    }
    if !compute_parameters.is_empty() {
        parameter_groups.push(json!({
            "Label": { "default": "Runtime compute" },
            "Parameters": compute_parameters
        }));
    }
    parameter_groups.push(json!({
        "Label": { "default": "Operations" },
        "Parameters": [
            PARAM_UPDATES_MODE,
            PARAM_TELEMETRY_MODE,
            PARAM_HEARTBEATS_MODE
        ]
    }));

    let mut parameter_labels = serde_json::Map::new();
    insert_parameter_label(&mut parameter_labels, PARAM_TOKEN, "Install token");
    insert_parameter_label(
        &mut parameter_labels,
        PARAM_MANAGING_ROLE_ARN,
        "Management role ARN",
    );
    insert_parameter_label(
        &mut parameter_labels,
        PARAM_MANAGING_ACCOUNT_ID,
        "Image account ID",
    );
    for parameter in network_parameter_names(settings.network.as_ref()) {
        let label = match parameter {
            PARAM_VPC_CIDR => "VPC CIDR",
            PARAM_AVAILABILITY_ZONES => "Availability zones",
            PARAM_VPC_ID => "VPC ID",
            PARAM_PUBLIC_SUBNET_IDS => "Public subnet IDs",
            PARAM_PRIVATE_SUBNET_IDS => "Private subnet IDs",
            PARAM_SECURITY_GROUP_IDS => "Security group IDs",
            PARAM_NETWORK_MODE => "Network",
            _ => continue,
        };
        insert_parameter_label(&mut parameter_labels, parameter, label);
    }
    if supports_custom_domain {
        insert_parameter_label(&mut parameter_labels, PARAM_DOMAIN_NAME, "Domain name");
        insert_parameter_label(
            &mut parameter_labels,
            PARAM_HOSTED_ZONE_ID,
            "Hosted zone ID",
        );
        insert_parameter_label(
            &mut parameter_labels,
            PARAM_CERTIFICATE_ARN,
            "Certificate ARN",
        );
    }
    for input in stack_inputs {
        insert_parameter_label(
            &mut parameter_labels,
            &stack_input_parameter_name(input),
            &input.label,
        );
    }
    for (parameter, label) in compute_parameter_labels(stack, settings.compute.as_ref()) {
        insert_parameter_label(&mut parameter_labels, &parameter, &label);
    }
    insert_parameter_label(&mut parameter_labels, PARAM_UPDATES_MODE, "Updates");
    insert_parameter_label(&mut parameter_labels, PARAM_TELEMETRY_MODE, "Telemetry");
    insert_parameter_label(&mut parameter_labels, PARAM_HEARTBEATS_MODE, "Heartbeats");

    template.metadata.insert(
        "AWS::CloudFormation::Interface".to_string(),
        json!({
            "ParameterGroups": parameter_groups,
            "ParameterLabels": parameter_labels
        }),
    );
}

fn network_parameter_names(network: Option<&NetworkSettings>) -> Vec<&'static str> {
    match network {
        Some(
            NetworkSettings::UseDefault
            | NetworkSettings::Create { .. }
            | NetworkSettings::ByoVpcAws { .. },
        ) => vec![
            PARAM_NETWORK_MODE,
            PARAM_VPC_CIDR,
            PARAM_AVAILABILITY_ZONES,
            PARAM_VPC_ID,
            PARAM_PUBLIC_SUBNET_IDS,
            PARAM_PRIVATE_SUBNET_IDS,
            PARAM_SECURITY_GROUP_IDS,
        ],
        None | Some(NetworkSettings::ByoVpcGcp { .. } | NetworkSettings::ByoVnetAzure { .. }) => {
            Vec::new()
        }
    }
}

fn compute_capacity_groups(stack: &Stack) -> Vec<&CapacityGroup> {
    let mut groups: Vec<&CapacityGroup> = stack
        .resources()
        .filter_map(|(_resource_id, resource)| resource.config.downcast_ref::<ComputeCluster>())
        .flat_map(|cluster| cluster.capacity_groups.iter())
        .collect();
    groups.sort_by(|left, right| left.group_id.cmp(&right.group_id));
    groups
}

fn compute_parameter_names(
    stack: &Stack,
    compute: Option<&alien_core::ComputeSettings>,
) -> Vec<String> {
    let mut parameters = Vec::new();
    for group in compute_capacity_groups(stack) {
        let Some(selection) = compute.and_then(|settings| settings.pools.get(&group.group_id))
        else {
            continue;
        };
        parameters.push(compute_machine_parameter_name(&group.group_id));
        match selection {
            ComputePoolSelection::Fixed { .. } => {
                parameters.push(compute_fixed_machines_parameter_name(&group.group_id));
            }
            ComputePoolSelection::Autoscale { .. } => {
                parameters.push(compute_autoscale_min_parameter_name(&group.group_id));
                parameters.push(compute_autoscale_max_parameter_name(&group.group_id));
            }
        }
    }
    parameters
}

fn compute_parameter_labels(
    stack: &Stack,
    compute: Option<&alien_core::ComputeSettings>,
) -> Vec<(String, String)> {
    let mut labels = Vec::new();
    for group in compute_capacity_groups(stack) {
        let Some(selection) = compute.and_then(|settings| settings.pools.get(&group.group_id))
        else {
            continue;
        };
        let label_prefix = compute_pool_label(&group.group_id);
        labels.push((
            compute_machine_parameter_name(&group.group_id),
            format!("{label_prefix} machine"),
        ));
        match selection {
            ComputePoolSelection::Fixed { .. } => labels.push((
                compute_fixed_machines_parameter_name(&group.group_id),
                format!("{label_prefix} machines"),
            )),
            ComputePoolSelection::Autoscale { .. } => {
                labels.push((
                    compute_autoscale_min_parameter_name(&group.group_id),
                    format!("{label_prefix} minimum machines"),
                ));
                labels.push((
                    compute_autoscale_max_parameter_name(&group.group_id),
                    format!("{label_prefix} maximum machines"),
                ));
            }
        }
    }
    labels
}

fn compute_settings_expression(
    stack: &Stack,
    compute: Option<&alien_core::ComputeSettings>,
) -> Option<CfExpression> {
    let mut pools = Vec::new();
    for group in compute_capacity_groups(stack) {
        let Some(selection) = compute.and_then(|settings| settings.pools.get(&group.group_id))
        else {
            continue;
        };
        let machine = (
            "machine",
            CfExpression::ref_(compute_machine_parameter_name(&group.group_id)),
        );
        let mut fields = match selection {
            ComputePoolSelection::Fixed { .. } => vec![
                ("mode", CfExpression::from("fixed")),
                (
                    "machines",
                    CfExpression::ref_(compute_fixed_machines_parameter_name(&group.group_id)),
                ),
                machine,
            ],
            ComputePoolSelection::Autoscale { .. } => vec![
                ("mode", CfExpression::from("autoscale")),
                (
                    "min",
                    CfExpression::ref_(compute_autoscale_min_parameter_name(&group.group_id)),
                ),
                (
                    "max",
                    CfExpression::ref_(compute_autoscale_max_parameter_name(&group.group_id)),
                ),
                machine,
            ],
        };
        if let Some(failure_domains) = selection.failure_domains() {
            fields.push((
                "failure_domains",
                CfExpression::object([
                    (
                        "spread",
                        CfExpression::from(u32::from(failure_domains.spread)),
                    ),
                    (
                        "selectedFailureDomains",
                        CfExpression::list(
                            failure_domains
                                .selected_failure_domains
                                .iter()
                                .cloned()
                                .map(CfExpression::from),
                        ),
                    ),
                ]),
            ));
        }
        let expression = CfExpression::object(fields);
        pools.push((group.group_id.as_str(), expression));
    }
    if pools.is_empty() {
        return None;
    }
    Some(CfExpression::object([(
        "pools",
        CfExpression::object(pools),
    )]))
}

fn compute_machine_parameter_name(pool_id: &str) -> String {
    format!("Compute{}Machine", pascal_identifier(pool_id))
}

fn compute_fixed_machines_parameter_name(pool_id: &str) -> String {
    format!("Compute{}Machines", pascal_identifier(pool_id))
}

fn compute_autoscale_min_parameter_name(pool_id: &str) -> String {
    format!("Compute{}Min", pascal_identifier(pool_id))
}

fn compute_autoscale_max_parameter_name(pool_id: &str) -> String {
    format!("Compute{}Max", pascal_identifier(pool_id))
}

fn compute_pool_label(pool_id: &str) -> String {
    pool_id
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn pascal_identifier(value: &str) -> String {
    let mut output = String::new();
    let mut uppercase_next = true;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if uppercase_next {
                output.push(character.to_ascii_uppercase());
                uppercase_next = false;
            } else {
                output.push(character);
            }
        } else {
            uppercase_next = true;
        }
    }
    if output.is_empty() {
        "Pool".to_string()
    } else if matches!(output.chars().next(), Some(character) if character.is_ascii_digit()) {
        format!("Pool{output}")
    } else {
        output
    }
}

fn insert_parameter_label(
    labels: &mut serde_json::Map<String, serde_json::Value>,
    parameter: &str,
    label: &str,
) {
    labels.insert(parameter.to_string(), json!({ "default": label }));
}

fn add_custom_resource(
    template: &mut CfTemplate,
    service_token: CfExpression,
    management_config: CfExpression,
    stack_settings: CfExpression,
    options: &CloudFormationOptions<'_>,
    resources: CfExpression,
    input_values: CfExpression,
    callback_url: Option<&str>,
) {
    let depends_on = template
        .resources
        .iter()
        .filter_map(|(logical_id, resource)| {
            resource.condition.is_none().then_some(logical_id.clone())
        })
        .collect();
    let mut resource = CfResource::new(
        "DeploymentRegistration".to_string(),
        "AWS::CloudFormation::CustomResource".to_string(),
    );
    resource.depends_on = depends_on;
    // Emit an explicit name so registered deployments mirror the CFN stack list
    // verbatim; callers who want a different identity can rewire the property
    // post-rendering.
    resource.properties = indexmap! {
        "ServiceToken".to_string() => service_token,
        "Token".to_string() => CfExpression::ref_(PARAM_TOKEN),
        "DeploymentName".to_string() => CfExpression::ref_("AWS::StackName"),
        "ResourcePrefix".to_string() => CfExpression::ref_("AWS::StackName"),
        "SourceKind".to_string() => CfExpression::from("cloudformation"),
        "Platform".to_string() => CfExpression::from(options.target.deployment_platform().as_str()),
        "Region".to_string() => CfExpression::ref_("AWS::Region"),
        "SetupTarget".to_string() => CfExpression::from(options.setup_target.clone()),
        "SetupImportFormatVersion".to_string() => CfExpression::from(CURRENT_SETUP_IMPORT_FORMAT_VERSION),
        "SetupFingerprint".to_string() => CfExpression::from(options.setup_fingerprint.clone()),
        "SetupFingerprintVersion".to_string() => CfExpression::from(options.setup_fingerprint_version),
        "ManagementConfig".to_string() => management_config,
        "StackSettings".to_string() => stack_settings,
        "Resources".to_string() => resources,
    };
    if !matches!(&input_values, CfExpression::Object(values) if values.is_empty()) {
        resource
            .properties
            .insert("InputValues".to_string(), input_values);
    }
    if let Some(base_platform) = options.target.base_platform() {
        resource.properties.insert(
            "BasePlatform".to_string(),
            CfExpression::from(base_platform.as_str()),
        );
    }
    if let Some(callback_url) = callback_url.filter(|value| !value.is_empty()) {
        resource.properties.insert(
            "CallbackUrl".to_string(),
            CfExpression::from(callback_url.to_string()),
        );
    }
    template
        .resources
        .insert(resource.logical_id.clone(), resource);
}

fn add_outputs(
    template: &mut CfTemplate,
    management_config: CfExpression,
    stack_settings: CfExpression,
    options: &CloudFormationOptions<'_>,
    resources: CfExpression,
) -> Result<()> {
    template.outputs.insert(
        OUTPUT_SOURCE_KIND.to_string(),
        output("Setup source kind.", CfExpression::from("cloudformation")),
    );
    if !matches!(options.registration, RegistrationMode::OutputsFallback) {
        template.outputs.insert(
            OUTPUT_DEPLOYMENT_ID.to_string(),
            output(
                "Registered deployment ID.",
                CfExpression::get_att("DeploymentRegistration", "DeploymentId"),
            ),
        );
    }
    template.outputs.insert(
        OUTPUT_RESOURCE_PREFIX.to_string(),
        output(
            "Stable physical resource prefix.",
            CfExpression::ref_("AWS::StackName"),
        ),
    );
    template.outputs.insert(
        OUTPUT_PLATFORM.to_string(),
        output(
            "Target platform.",
            CfExpression::from(options.target.deployment_platform().as_str()),
        ),
    );
    if let Some(base_platform) = options.target.base_platform() {
        template.outputs.insert(
            OUTPUT_BASE_PLATFORM.to_string(),
            output(
                "Base cloud platform for a Kubernetes deployment.",
                CfExpression::from(base_platform.as_str()),
            ),
        );
    }
    template.outputs.insert(
        OUTPUT_REGION.to_string(),
        output("AWS region.", CfExpression::ref_("AWS::Region")),
    );
    template.outputs.insert(
        OUTPUT_SETUP_TARGET.to_string(),
        output(
            "Setup target.",
            CfExpression::from(options.setup_target.clone()),
        ),
    );
    template.outputs.insert(
        OUTPUT_SETUP_FINGERPRINT.to_string(),
        output(
            "Setup compatibility fingerprint.",
            CfExpression::from(options.setup_fingerprint.clone()),
        ),
    );
    template.outputs.insert(
        OUTPUT_SETUP_IMPORT_FORMAT_VERSION.to_string(),
        output(
            "Setup registration payload format version.",
            CfExpression::from(CURRENT_SETUP_IMPORT_FORMAT_VERSION),
        ),
    );
    template.outputs.insert(
        OUTPUT_SETUP_FINGERPRINT_VERSION.to_string(),
        output(
            "Setup fingerprint algorithm version.",
            CfExpression::from(options.setup_fingerprint_version),
        ),
    );
    template.outputs.insert(
        OUTPUT_MANAGEMENT_CONFIG.to_string(),
        output(
            "Deployment registration management configuration JSON.",
            json_output_value(management_config),
        ),
    );
    template.outputs.insert(
        OUTPUT_STACK_SETTINGS.to_string(),
        output(
            "Deployment registration settings JSON.",
            CfExpression::to_json_string(stack_settings),
        ),
    );
    add_resource_outputs(template, resources)?;
    Ok(())
}

fn json_output_value(value: CfExpression) -> CfExpression {
    match value {
        CfExpression::Null => CfExpression::from("null"),
        value => CfExpression::to_json_string(value),
    }
}

fn add_resource_outputs(template: &mut CfTemplate, resources: CfExpression) -> Result<()> {
    let chunks = chunk_resource_expression(resources)?;
    if STANDARD_OUTPUT_COUNT + chunks.len() > CLOUDFORMATION_MAX_OUTPUTS {
        return Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "generate_cloudformation_template".to_string(),
            reason: format!(
                "CloudFormation Outputs fallback needs {} resource chunks, exceeding the {} output limit",
                chunks.len(),
                CLOUDFORMATION_MAX_OUTPUTS
            ),
        }));
    }

    if chunks.len() == 1 {
        let chunk = chunks.into_iter().next().expect("one chunk");
        let value = match &chunk {
            CfExpression::List(items) if items.is_empty() => CfExpression::from("[]"),
            _ => CfExpression::to_json_string(chunk),
        };
        template.outputs.insert(
            OUTPUT_RESOURCES.to_string(),
            output("Deployment registration resources JSON.", value),
        );
        return Ok(());
    }

    for (index, chunk) in chunks.into_iter().enumerate() {
        template.outputs.insert(
            format!("{OUTPUT_RESOURCES}{index}"),
            output(
                "Deployment registration resources JSON chunk. Reassemble chunks in numeric suffix order.",
                CfExpression::to_json_string(chunk),
            ),
        );
    }

    Ok(())
}

fn empty_object() -> CfExpression {
    CfExpression::Object(IndexMap::new())
}

fn kubernetes_cluster_namespace(stack: &Stack) -> Option<String> {
    stack.resources().find_map(|(_resource_id, entry)| {
        entry
            .config
            .downcast_ref::<KubernetesCluster>()
            .map(|cluster| cluster.namespace.clone())
    })
}

fn chunk_resource_expression(resources: CfExpression) -> Result<Vec<CfExpression>> {
    let CfExpression::List(items) = resources else {
        return Ok(vec![resources]);
    };
    if items.is_empty() {
        return Ok(vec![CfExpression::list([])]);
    }

    let mut chunks = Vec::new();
    let mut current = Vec::new();
    let mut current_len = 2usize;

    for item in items {
        let item_len = serde_json::to_string(&item)
            .map_err(|error| {
                AlienError::new(ErrorData::JsonSerializationFailed {
                    reason: format!(
                        "failed to estimate CloudFormation Outputs resource chunk size: {error}"
                    ),
                })
            })?
            .len();
        let separator_len = usize::from(!current.is_empty());
        let next_len = current_len + separator_len + item_len;
        if !current.is_empty() && next_len > OUTPUT_RESOURCES_CHUNK_BYTES {
            chunks.push(CfExpression::List(current));
            current = Vec::new();
            current_len = 2;
        }

        let separator_len = usize::from(!current.is_empty());
        current_len += separator_len + item_len;
        current.push(item);
    }

    if !current.is_empty() {
        chunks.push(CfExpression::List(current));
    }

    Ok(chunks)
}

fn stack_settings_expression(
    target: CloudFormationTarget,
    stack: &Stack,
    settings: &StackSettings,
    kubernetes_namespace: Option<CfExpression>,
    supports_custom_domain: bool,
) -> CfExpression {
    let mut values = vec![
        ("deploymentModel", CfExpression::from("push")),
        ("updates", CfExpression::ref_(PARAM_UPDATES_MODE)),
        ("telemetry", CfExpression::ref_(PARAM_TELEMETRY_MODE)),
        ("heartbeats", CfExpression::ref_(PARAM_HEARTBEATS_MODE)),
        ("network", network_expression(settings.network.as_ref())),
    ];
    if supports_custom_domain {
        values.push(("domains", domains_expression()));
    }
    if target.is_kubernetes() {
        values.push((
            "kubernetes",
            kubernetes_settings_expression(settings.kubernetes.as_ref(), kubernetes_namespace),
        ));
    }
    if let Some(compute) = compute_settings_expression(stack, settings.compute.as_ref()) {
        values.push(("compute", compute));
    }
    CfExpression::object(values)
}
