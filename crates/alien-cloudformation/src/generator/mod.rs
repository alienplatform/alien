mod expressions;
mod metadata;
mod parameters;
mod registration;

use metadata::{
    add_console_interface_metadata, compute_autoscale_max_parameter_name,
    compute_autoscale_min_parameter_name, compute_capacity_groups,
    compute_fixed_machines_parameter_name, compute_machine_parameter_name,
    compute_settings_expression,
};
use registration::{
    add_custom_resource, add_outputs, empty_object, kubernetes_cluster_namespace,
    stack_settings_expression, RegistrationEntry,
};

#[cfg(test)]
mod tests;

use crate::{
    emitters::enabled,
    registry::CfRegistry,
    template::{CfExpression, CfMapping, CfParameter, CfResource, CfTemplate},
};
use alien_core::{
    import::{EmitContext, CURRENT_SETUP_IMPORT_FORMAT_VERSION},
    ownership_policy_for_resource_type, CapacityGroup, ComputeCluster, ComputePoolSelection,
    DeploymentModel, ErrorData, KubernetesCluster, NetworkSettings, Platform,
    RemoteStackManagement, Result, Stack, StackInputDefaultValue, StackInputDefinition,
    StackInputKind, StackInputProvider, StackSettings, Worker, WorkerCode,
};
use alien_error::AlienError;
use expressions::{
    domains_expression, equals_ref, kubernetes_settings_expression, management_config_expression,
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
/// `Fn::Sub` variable carrying one registration entry's JSON text.
const ENTRY_JSON_SUB_VARIABLE: &str = "entry";
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

    let mut registration_resources: Vec<RegistrationEntry> = Vec::new();
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

        let enabled_when = resource.enabled_when.as_deref();
        let declared_condition = if let Some(input_id) = enabled_when {
            if !emitter.supports_enabled_when() {
                return Err(AlienError::new(ErrorData::OperationNotSupported {
                    operation: format!("enabled() on resource type '{resource_type}'"),
                    reason: format!(
                        "the CloudFormation emitter for '{resource_type}' does not render \
                         conditionally yet, so resource '{resource_id}' would be created \
                         regardless of the deployer's answer"
                    ),
                }));
            }
            Some(declare_enabled_condition(
                &mut template,
                &stack_inputs,
                input_id,
                resource_id,
            )?)
        } else {
            None
        };

        let mut emitted_resources = emitter.emit_resources_with_registry(&ctx, options.registry)?;
        if let Some(condition) = declared_condition {
            for emitted in &mut emitted_resources {
                // A resource carries at most one Condition, so an emitter that
                // already set its own leaves nowhere to put the deployer's gate.
                // Expressing both would need an `Fn::And` over the two, which
                // nothing needs yet — until it does, refuse rather than pick one
                // and create the resource the deployer declined.
                //
                // No shipped emitter reaches this: the ones that set conditions
                // (aws/network.rs, aws/kubernetes_cluster.rs) return false from
                // `supports_enabled_when`, so they fail the check above first.
                if let Some(existing) = &emitted.condition {
                    return Err(AlienError::new(ErrorData::OperationNotSupported {
                        operation: format!("enabled() on resource type '{resource_type}'"),
                        reason: format!(
                            "the CloudFormation emitter for '{resource_type}' already puts \
                             condition '{existing}' on '{}', and a resource can carry only one \
                             condition",
                            emitted.logical_id
                        ),
                    }));
                }
                emitted.condition = Some(condition.clone());
            }
        }
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
        registration_resources.push(RegistrationEntry {
            enabled_when: enabled_when.map(str::to_string),
            entry: CfExpression::object([
                ("id", CfExpression::from(resource_id.as_str())),
                ("type", CfExpression::from(resource_type.as_ref())),
                ("importData", registration_data),
            ]),
        });
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
    apply_resource_dependencies(stack, &emitted_resource_ids, &mut template);

    if let Some(service_token) = options.registration.service_token(&mut template)? {
        add_custom_resource(
            &mut template,
            service_token,
            management_config.clone(),
            stack_settings.clone(),
            &options,
            CfExpression::list(
                registration_resources
                    .iter()
                    .map(RegistrationEntry::custom_resource_element),
            ),
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
            &registration_resources,
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
    stack_input_parameter_name_for_id(&input.id)
}

/// Parameter name for a stack input id. Shared with the gating helpers, which
/// only know the id.
pub(crate) fn stack_input_parameter_name_for_id(input_id: &str) -> String {
    format!("Input{}", sanitize_logical_id(input_id))
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

/// Declares the condition a gated resource renders under, once per gating input.
///
/// Boolean stack inputs reach CloudFormation as string parameters constrained to
/// `"true"` / `"false"`, so the gate is an equality test against `"true"`.
///
/// The "input is declared" and "input is boolean" rules below are also enforced
/// by `ResourceEnabledValidCheck`, repeated here on purpose: a caller that
/// renders without running preflights must not get a template that silently
/// drops the gate.
fn declare_enabled_condition(
    template: &mut CfTemplate,
    stack_inputs: &[StackInputDefinition],
    input_id: &str,
    resource_id: &str,
) -> Result<String> {
    let condition_name = enabled::condition_name(input_id);
    if template.conditions.contains_key(&condition_name) {
        // A sibling resource on the same gate already validated the input
        // and declared the condition.
        return Ok(condition_name);
    }

    let input = alien_core::find_boolean_gate_input(stack_inputs, input_id).map_err(|issue| {
        AlienError::new(ErrorData::OperationNotSupported {
            operation: format!("enabled('{input_id}')"),
            reason: match issue {
                alien_core::GateInputIssue::Undeclared => format!(
                    "resource '{resource_id}' is gated on stack input '{input_id}', which this \
                     template never asks the deployer for"
                ),
                alien_core::GateInputIssue::NotBoolean(kind) => format!(
                    "resource '{resource_id}' is gated on stack input '{input_id}', which is a \
                     {kind:?} input rather than a boolean"
                ),
            },
        })
    })?;

    template.conditions.insert(
        condition_name.clone(),
        equals_ref(&stack_input_parameter_name(input), "true"),
    );
    Ok(condition_name)
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
    // Remote Storage grants refer back to the management role. For the
    // management -> storage bootstrap edge, wait for the bucket resources but
    // not the grants that cannot exist until management does.
    let remote_storage_prerequisite_targets: IndexMap<String, Vec<String>> = emitted_resource_ids
        .iter()
        .filter(|(resource_id, _)| {
            stack
                .resources
                .get(*resource_id)
                .is_some_and(alien_core::ResourceEntry::is_remote_frozen_storage)
        })
        .map(|(resource_id, logical_ids)| {
            let targets = logical_ids
                .iter()
                .filter(|logical_id| {
                    template.resources.get(*logical_id).is_some_and(|resource| {
                        resource.condition.is_none()
                            && !is_remote_storage_permission_support_resource(resource)
                    })
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
            let targets = if entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE {
                remote_storage_prerequisite_targets
                    .get(dependency.id())
                    .or_else(|| dependency_targets.get(dependency.id()))
            } else {
                dependency_targets.get(dependency.id())
            };
            if let Some(targets) = targets {
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

fn is_remote_storage_permission_support_resource(resource: &CfResource) -> bool {
    resource.resource_type == "AWS::IAM::Policy"
}
