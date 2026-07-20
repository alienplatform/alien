use crate::{
    registry::CfRegistry,
    template::{
        CfExpression, CfMapping, CfOutput, CfParameter, CfResource, CfRule, CfRuleAssertion,
        CfTemplate,
    },
};
use alien_core::{
    import::{EmitContext, CURRENT_SETUP_IMPORT_FORMAT_VERSION},
    ownership_policy_for_resource_type, CapacityGroup, CapacityGroupScalePolicy, ComputeCluster,
    ComputePoolSelection, DeploymentModel, DomainSettings, ErrorData, HeartbeatsMode,
    KubernetesCluster, KubernetesSettings, Network, NetworkSettings, Platform, Result, Stack,
    StackInputDefaultValue, StackInputDefinition, StackInputKind, StackInputProvider,
    StackSettings, TelemetryMode, UpdatesMode, Worker, WorkerCode,
};
use alien_error::AlienError;
use indexmap::{indexmap, IndexMap};
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

/// Generate a CloudFormation stack policy that prevents stack updates from
/// mutating runtime-managed resources after setup registration.
pub fn generate_cloudformation_stack_policy(_stack: &Stack) -> Result<serde_json::Value> {
    Ok(json!({ "Statement": [] }))
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

fn add_standard_parameters(
    template: &mut CfTemplate,
    stack: &Stack,
    settings: &StackSettings,
    supports_custom_domain: bool,
) -> Result<()> {
    template.parameters.insert(
        PARAM_TOKEN.to_string(),
        string_parameter(
            "Install token from the application setup page.",
            None,
            None,
            true,
        ),
    );
    template.parameters.insert(
        PARAM_MANAGING_ROLE_ARN.to_string(),
        string_parameter(
            "ARN of the management identity allowed to assume setup-created roles.",
            Some(String::new()),
            None,
            false,
        ),
    );
    template.parameters.insert(
        PARAM_MANAGING_ACCOUNT_ID.to_string(),
        string_parameter(
            "AWS account ID for the management account that hosts application container images.",
            Some(String::new()),
            None,
            false,
        ),
    );

    add_network_parameters(template, settings.network.as_ref());
    add_compute_parameters(template, stack, settings.compute.as_ref())?;

    if supports_custom_domain {
        let domain_defaults = DomainParameterDefaults::from_settings(settings.domains.as_ref());
        template.parameters.insert(
            PARAM_DOMAIN_NAME.to_string(),
            string_parameter(
                "Optional custom domain for public endpoints. Leave unset to expose through the generated load balancer DNS name over HTTP.",
                Some(domain_defaults.domain_name.unwrap_or_default()),
                None,
                false,
            ),
        );
        template.parameters.insert(
            PARAM_HOSTED_ZONE_ID.to_string(),
            string_parameter(
                "Route 53 hosted zone ID for the custom domain. Not needed for the auto-generated domain.",
                Some(String::new()),
                None,
                false,
            ),
        );
        template.parameters.insert(
            PARAM_CERTIFICATE_ARN.to_string(),
            string_parameter_with_allowed_pattern(
                "ACM certificate ARN for the custom domain. Required when DomainName is set.",
                Some(domain_defaults.certificate_arn.unwrap_or_default()),
                None,
                Some("^$|^arn:aws(-[a-z]+)?:acm:[a-z0-9-]+:[0-9]{12}:certificate/.+$".to_string()),
                false,
            ),
        );
    }

    template.parameters.insert(
        PARAM_UPDATES_MODE.to_string(),
        string_parameter(
            "How updates are applied after setup registration.",
            Some(updates_mode(settings.updates).to_string()),
            Some(vec![
                CfExpression::from("auto"),
                CfExpression::from("approval-required"),
            ]),
            false,
        ),
    );
    template.parameters.insert(
        PARAM_TELEMETRY_MODE.to_string(),
        string_parameter(
            "Telemetry collection behavior.",
            Some(telemetry_mode(settings.telemetry).to_string()),
            Some(vec![CfExpression::from(telemetry_mode(settings.telemetry))]),
            false,
        ),
    );
    template.parameters.insert(
        PARAM_HEARTBEATS_MODE.to_string(),
        string_parameter(
            "Heartbeat health-check behavior.",
            Some(heartbeats_mode(settings.heartbeats).to_string()),
            Some(vec![CfExpression::from("off"), CfExpression::from("on")]),
            false,
        ),
    );
    Ok(())
}

fn add_network_parameters(template: &mut CfTemplate, network: Option<&NetworkSettings>) {
    let defaults = NetworkParameterDefaults::from_settings(network);
    template.parameters.insert(
        PARAM_NETWORK_MODE.to_string(),
        string_parameter(
            "Choose create-new for a managed VPC, use-existing for your VPC, or use-default for the account default VPC.",
            Some(network_mode_default(network).to_string()),
            Some(vec![
                CfExpression::from("create-new"),
                CfExpression::from("use-existing"),
                CfExpression::from("use-default"),
            ]),
            false,
        ),
    );
    match network {
        Some(
            NetworkSettings::Create { .. }
            | NetworkSettings::UseDefault
            | NetworkSettings::ByoVpcAws { .. },
        ) => {
            template.parameters.insert(
                PARAM_VPC_CIDR.to_string(),
                string_parameter(
                    "Only used with create-new. CIDR for the new VPC.",
                    Some(defaults.cidr.unwrap_or_else(|| "10.42.0.0/16".to_string())),
                    None,
                    false,
                ),
            );
            template.parameters.insert(
                PARAM_AVAILABILITY_ZONES.to_string(),
                number_parameter(
                    "Only used with create-new. Number of availability zones for the new VPC.",
                    u32::from(defaults.availability_zones),
                    Some(vec![
                        CfExpression::from(1u8),
                        CfExpression::from(2u8),
                        CfExpression::from(3u8),
                    ]),
                ),
            );
            template.parameters.insert(
                PARAM_VPC_ID.to_string(),
                string_parameter(
                    "Only used with use-existing. Existing VPC ID.",
                    Some(defaults.vpc_id.unwrap_or_default()),
                    None,
                    false,
                ),
            );
            template.parameters.insert(
                PARAM_PUBLIC_SUBNET_IDS.to_string(),
                comma_list_parameter(
                    "Only used with use-existing. Existing public subnet IDs.",
                    defaults.public_subnet_ids,
                ),
            );
            template.parameters.insert(
                PARAM_PRIVATE_SUBNET_IDS.to_string(),
                comma_list_parameter(
                    "Only used with use-existing. Existing private subnet IDs.",
                    defaults.private_subnet_ids,
                ),
            );
            template.parameters.insert(
                PARAM_SECURITY_GROUP_IDS.to_string(),
                comma_list_parameter(
                    "Only used with use-existing. Existing security group IDs.",
                    defaults.security_group_ids,
                ),
            );
        }
        None | Some(NetworkSettings::ByoVpcGcp { .. } | NetworkSettings::ByoVnetAzure { .. }) => {}
    }
}

fn add_compute_parameters(
    template: &mut CfTemplate,
    stack: &Stack,
    compute: Option<&alien_core::ComputeSettings>,
) -> Result<()> {
    let plan = alien_core::compute_planner::plan_compute(stack, Platform::Aws, compute)?;
    for group in compute_capacity_groups(stack) {
        let Some(selection) = compute.and_then(|settings| settings.pools.get(&group.group_id))
        else {
            continue;
        };
        let machine_parameter = compute_machine_parameter_name(&group.group_id);
        let allowed_values = plan
            .pools
            .iter()
            .find(|pool| pool.pool_id == group.group_id)
            .map(|pool| {
                pool.machines
                    .iter()
                    .map(|machine| CfExpression::from(machine.machine.as_str()))
                    .collect()
            });
        template.parameters.insert(
            machine_parameter,
            string_parameter(
                &format!(
                    "Provider machine type for runtime compute pool '{}'.",
                    group.group_id
                ),
                selection.machine().map(ToString::to_string),
                allowed_values,
                false,
            ),
        );

        let scale = group.scale_policy.as_ref().cloned().unwrap_or_else(|| {
            CapacityGroupScalePolicy::from_selected_bounds(group.min_size, group.max_size)
        });
        match (selection, scale) {
            (
                ComputePoolSelection::Fixed { machines, .. },
                CapacityGroupScalePolicy::Fixed { machines: range },
            ) => {
                let mut parameter = number_parameter(
                    &format!(
                        "Fixed machine count for runtime compute pool '{}'.",
                        group.group_id
                    ),
                    *machines,
                    None,
                );
                parameter.min_value = Some(CfExpression::from(range.min));
                parameter.max_value = Some(CfExpression::from(range.max));
                template.parameters.insert(
                    compute_fixed_machines_parameter_name(&group.group_id),
                    parameter,
                );
            }
            (
                ComputePoolSelection::Autoscale { min, max, .. },
                CapacityGroupScalePolicy::Autoscale {
                    min: min_range,
                    max: max_range,
                },
            ) => {
                let mut min_parameter = number_parameter(
                    &format!(
                        "Minimum machine count for runtime compute pool '{}'.",
                        group.group_id
                    ),
                    *min,
                    None,
                );
                min_parameter.min_value = Some(CfExpression::from(min_range.min));
                min_parameter.max_value = Some(CfExpression::from(min_range.max));
                template.parameters.insert(
                    compute_autoscale_min_parameter_name(&group.group_id),
                    min_parameter,
                );

                let mut max_parameter = number_parameter(
                    &format!(
                        "Maximum machine count for runtime compute pool '{}'.",
                        group.group_id
                    ),
                    *max,
                    None,
                );
                max_parameter.min_value = Some(CfExpression::from(max_range.min));
                max_parameter.max_value = Some(CfExpression::from(max_range.max));
                template.parameters.insert(
                    compute_autoscale_max_parameter_name(&group.group_id),
                    max_parameter,
                );
            }
            _ => {}
        }
    }
    Ok(())
}

fn add_supported_region_rule(template: &mut CfTemplate, registration: &RegistrationMode) {
    let supported_regions = registration.supported_regions();
    if supported_regions.is_empty() {
        return;
    }

    let regions = supported_regions.join(", ");
    template.rules.insert(
        RULE_SUPPORTED_AWS_REGION.to_string(),
        CfRule {
            assertions: vec![CfRuleAssertion {
                assertion: CfExpression::contains(
                    CfExpression::list(supported_regions.into_iter().map(CfExpression::from)),
                    CfExpression::ref_("AWS::Region"),
                ),
                assert_description: format!(
                    "This template can only be launched in AWS regions supported by this environment: {regions}."
                ),
            }],
        },
    );
}

fn add_custom_domain_certificate_rule(template: &mut CfTemplate) {
    template.rules.insert(
        RULE_CUSTOM_DOMAIN_CERTIFICATE.to_string(),
        CfRule {
            assertions: vec![CfRuleAssertion {
                assertion: CfExpression::or([
                    equals_ref(PARAM_DOMAIN_NAME, ""),
                    CfExpression::not(equals_ref(PARAM_CERTIFICATE_ARN, "")),
                ]),
                assert_description: "CertificateArn must be set to an AWS ACM certificate ARN when DomainName is set.".to_string(),
            }],
        },
    );
}

fn add_standard_conditions(
    template: &mut CfTemplate,
    stack: &Stack,
    settings: &StackSettings,
    supports_custom_domain: bool,
) {
    let has_created_network = stack_has_created_network(stack);
    if has_dynamic_aws_network_settings(settings.network.as_ref()) || has_created_network {
        template.conditions.insert(
            CONDITION_NETWORK_MODE_CREATE.to_string(),
            equals_ref(PARAM_NETWORK_MODE, "create-new"),
        );
        template.conditions.insert(
            CONDITION_NETWORK_MODE_USE_EXISTING.to_string(),
            equals_ref(PARAM_NETWORK_MODE, "use-existing"),
        );
    }
    if has_created_network {
        template.conditions.insert(
            CONDITION_NETWORK_AZ2.to_string(),
            CfExpression::not(CfExpression::equals(
                CfExpression::ref_(PARAM_AVAILABILITY_ZONES),
                CfExpression::from(1u8),
            )),
        );
        template.conditions.insert(
            CONDITION_NETWORK_AZ3.to_string(),
            CfExpression::equals(
                CfExpression::ref_(PARAM_AVAILABILITY_ZONES),
                CfExpression::from(3u8),
            ),
        );
        template.conditions.insert(
            CONDITION_NETWORK_CREATE_AZ2.to_string(),
            CfExpression::and([
                equals_ref(PARAM_NETWORK_MODE, "create-new"),
                condition_ref(CONDITION_NETWORK_AZ2),
            ]),
        );
        template.conditions.insert(
            CONDITION_NETWORK_CREATE_AZ3.to_string(),
            CfExpression::and([
                equals_ref(PARAM_NETWORK_MODE, "create-new"),
                condition_ref(CONDITION_NETWORK_AZ3),
            ]),
        );
    }
    if has_dynamic_aws_network_settings(settings.network.as_ref()) || has_created_network {
        template.conditions.insert(
            CONDITION_HAS_VPC_CIDR.to_string(),
            CfExpression::not(equals_ref(PARAM_VPC_CIDR, "")),
        );
    }
    if supports_custom_domain {
        template.conditions.insert(
            CONDITION_HAS_DOMAIN_NAME.to_string(),
            CfExpression::not(equals_ref(PARAM_DOMAIN_NAME, "")),
        );
    }
}

fn stack_has_created_network(stack: &Stack) -> bool {
    stack.resources().any(|(_resource_id, resource)| {
        resource
            .config
            .downcast_ref::<Network>()
            .is_some_and(|network| matches!(network.settings, NetworkSettings::Create { .. }))
    })
}

fn has_dynamic_aws_network_settings(network: Option<&NetworkSettings>) -> bool {
    matches!(
        network,
        Some(
            NetworkSettings::Create { .. }
                | NetworkSettings::UseDefault
                | NetworkSettings::ByoVpcAws { .. }
        )
    )
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
        let expression = match selection {
            ComputePoolSelection::Fixed { .. } => CfExpression::object([
                ("mode", CfExpression::from("fixed")),
                (
                    "machines",
                    CfExpression::ref_(compute_fixed_machines_parameter_name(&group.group_id)),
                ),
                machine,
            ]),
            ComputePoolSelection::Autoscale { .. } => CfExpression::object([
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
            ]),
        };
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

fn kubernetes_settings_expression(
    settings: Option<&KubernetesSettings>,
    namespace: Option<CfExpression>,
) -> CfExpression {
    let mut expression = default_kubernetes_settings_expression();

    if let Some(settings) = settings {
        let value = serde_json::to_value(settings)
            .expect("serializing Kubernetes stack settings should not fail");
        merge_cf_expression(&mut expression, cf_expression_from_json(value));
    }

    if let Some(namespace) = namespace {
        set_kubernetes_namespace_expression(&mut expression, namespace);
    }

    expression
}

fn default_kubernetes_settings_expression() -> CfExpression {
    CfExpression::object([
        (
            "cluster",
            CfExpression::object([
                ("ownership", CfExpression::from("managed")),
                ("namespace", CfExpression::from("default")),
            ]),
        ),
        (
            "exposure",
            CfExpression::if_(
                CONDITION_HAS_DOMAIN_NAME,
                custom_domain_kubernetes_exposure_expression(),
                generated_load_balancer_kubernetes_exposure_expression(),
            ),
        ),
    ])
}

fn generated_load_balancer_kubernetes_exposure_expression() -> CfExpression {
    CfExpression::object([
        ("mode", CfExpression::from("generated")),
        ("route", aws_alb_kubernetes_route_expression()),
        (
            "certificate",
            CfExpression::object([("mode", CfExpression::from("none"))]),
        ),
    ])
}

fn custom_domain_kubernetes_exposure_expression() -> CfExpression {
    CfExpression::object([
        ("mode", CfExpression::from("custom")),
        ("domain", CfExpression::ref_(PARAM_DOMAIN_NAME)),
        ("route", aws_alb_kubernetes_route_expression()),
        (
            "certificate",
            CfExpression::object([
                ("mode", CfExpression::from("awsAcmArn")),
                ("certificateArn", CfExpression::ref_(PARAM_CERTIFICATE_ARN)),
            ]),
        ),
    ])
}

fn aws_alb_kubernetes_route_expression() -> CfExpression {
    CfExpression::object([
        ("routeApi", CfExpression::from("ingress")),
        ("controller", CfExpression::from("eks.amazonaws.com/alb")),
        ("ingressClassName", CfExpression::from("alb")),
        ("labels", empty_object()),
        ("annotations", empty_object()),
        (
            "provider",
            CfExpression::object([
                ("provider", CfExpression::from("awsAlb")),
                ("scheme", CfExpression::from("internet-facing")),
                ("targetType", CfExpression::from("ip")),
                ("subnetIds", CfExpression::list([])),
            ]),
        ),
    ])
}

fn set_kubernetes_namespace_expression(expression: &mut CfExpression, namespace: CfExpression) {
    let CfExpression::Object(root) = expression else {
        return;
    };
    let cluster = root
        .entry("cluster".to_string())
        .or_insert_with(|| CfExpression::object([("ownership", CfExpression::from("managed"))]));
    let CfExpression::Object(cluster) = cluster else {
        *cluster = CfExpression::object([
            ("ownership", CfExpression::from("managed")),
            ("namespace", namespace),
        ]);
        return;
    };
    cluster.insert("namespace".to_string(), namespace);
}

fn merge_cf_expression(base: &mut CfExpression, overlay: CfExpression) {
    if is_cloudformation_intrinsic(base) || is_cloudformation_intrinsic(&overlay) {
        *base = overlay;
        return;
    }

    match (base, overlay) {
        (CfExpression::Object(base), CfExpression::Object(overlay)) => {
            for (key, value) in overlay {
                match base.get_mut(&key) {
                    Some(existing) => merge_cf_expression(existing, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, overlay) => *base = overlay,
    }
}

fn is_cloudformation_intrinsic(expression: &CfExpression) -> bool {
    let CfExpression::Object(values) = expression else {
        return false;
    };
    values
        .keys()
        .any(|key| key == "Ref" || key.starts_with("Fn::"))
}

fn cf_expression_from_json(value: Value) -> CfExpression {
    match value {
        Value::Null => CfExpression::Null,
        Value::Bool(value) => CfExpression::Bool(value),
        Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                CfExpression::Integer(value)
            } else {
                CfExpression::Number(
                    number
                        .as_f64()
                        .expect("serde_json numbers should be representable as f64"),
                )
            }
        }
        Value::String(value) => CfExpression::String(value),
        Value::Array(values) => {
            CfExpression::List(values.into_iter().map(cf_expression_from_json).collect())
        }
        Value::Object(values) => CfExpression::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, cf_expression_from_json(value)))
                .collect(),
        ),
    }
}

fn network_expression(network: Option<&NetworkSettings>) -> CfExpression {
    match network {
        None => CfExpression::no_value(),
        Some(
            NetworkSettings::UseDefault
            | NetworkSettings::Create { .. }
            | NetworkSettings::ByoVpcAws { .. },
        ) => CfExpression::if_(
            CONDITION_NETWORK_MODE_CREATE,
            CfExpression::object([
                ("type", CfExpression::from("create")),
                (
                    "cidr",
                    CfExpression::if_(
                        CONDITION_HAS_VPC_CIDR,
                        CfExpression::ref_(PARAM_VPC_CIDR),
                        CfExpression::no_value(),
                    ),
                ),
                (
                    "availability_zones",
                    CfExpression::ref_(PARAM_AVAILABILITY_ZONES),
                ),
            ]),
            CfExpression::if_(
                CONDITION_NETWORK_MODE_USE_EXISTING,
                CfExpression::object([
                    ("type", CfExpression::from("byo-vpc-aws")),
                    ("vpc_id", CfExpression::ref_(PARAM_VPC_ID)),
                    (
                        "public_subnet_ids",
                        CfExpression::ref_(PARAM_PUBLIC_SUBNET_IDS),
                    ),
                    (
                        "private_subnet_ids",
                        CfExpression::ref_(PARAM_PRIVATE_SUBNET_IDS),
                    ),
                    (
                        "security_group_ids",
                        CfExpression::ref_(PARAM_SECURITY_GROUP_IDS),
                    ),
                ]),
                CfExpression::object([("type", CfExpression::from("use-default"))]),
            ),
        ),
        Some(NetworkSettings::ByoVpcGcp { .. } | NetworkSettings::ByoVnetAzure { .. }) => {
            CfExpression::no_value()
        }
    }
}

fn domains_expression() -> CfExpression {
    CfExpression::if_(
        CONDITION_HAS_DOMAIN_NAME,
        CfExpression::object([(
            "customDomains",
            CfExpression::object([(
                "default",
                CfExpression::object([
                    ("domain", CfExpression::ref_(PARAM_DOMAIN_NAME)),
                    (
                        "certificate",
                        CfExpression::object([(
                            "aws",
                            CfExpression::object([(
                                "certificateArn",
                                CfExpression::ref_(PARAM_CERTIFICATE_ARN),
                            )]),
                        )]),
                    ),
                ]),
            )]),
        )]),
        CfExpression::no_value(),
    )
}

fn management_config_expression(target: CloudFormationTarget) -> CfExpression {
    if target.is_kubernetes() {
        return CfExpression::Null;
    }

    CfExpression::object([
        ("platform", CfExpression::from("aws")),
        (
            "managingRoleArn",
            CfExpression::ref_(PARAM_MANAGING_ROLE_ARN),
        ),
    ])
}

fn string_parameter(
    description: &str,
    default: Option<String>,
    allowed_values: Option<Vec<CfExpression>>,
    no_echo: bool,
) -> CfParameter {
    string_parameter_with_allowed_pattern(description, default, allowed_values, None, no_echo)
}

fn string_parameter_with_allowed_pattern(
    description: &str,
    default: Option<String>,
    allowed_values: Option<Vec<CfExpression>>,
    allowed_pattern: Option<String>,
    no_echo: bool,
) -> CfParameter {
    CfParameter {
        parameter_type: "String".to_string(),
        description: Some(description.to_string()),
        default: default.map(CfExpression::from),
        allowed_values,
        allowed_pattern,
        min_length: None,
        max_length: None,
        min_value: None,
        max_value: None,
        no_echo: no_echo.then_some(true),
    }
}

fn number_parameter(
    description: &str,
    default: u32,
    allowed_values: Option<Vec<CfExpression>>,
) -> CfParameter {
    CfParameter {
        parameter_type: "Number".to_string(),
        description: Some(description.to_string()),
        default: Some(CfExpression::from(default)),
        allowed_values,
        allowed_pattern: None,
        min_length: None,
        max_length: None,
        min_value: None,
        max_value: None,
        no_echo: None,
    }
}

fn comma_list_parameter(description: &str, default: Vec<String>) -> CfParameter {
    CfParameter {
        parameter_type: "CommaDelimitedList".to_string(),
        description: Some(description.to_string()),
        default: Some(CfExpression::from(default.join(","))),
        allowed_values: None,
        allowed_pattern: None,
        min_length: None,
        max_length: None,
        min_value: None,
        max_value: None,
        no_echo: None,
    }
}

fn equals_ref(parameter: &str, value: &str) -> CfExpression {
    CfExpression::equals(CfExpression::ref_(parameter), CfExpression::from(value))
}

fn condition_ref(condition: &str) -> CfExpression {
    CfExpression::object([("Condition", CfExpression::from(condition))])
}

fn output(description: &str, value: CfExpression) -> CfOutput {
    CfOutput {
        description: Some(description.to_string()),
        value,
        export: None,
    }
}

fn updates_mode(mode: UpdatesMode) -> &'static str {
    match mode {
        UpdatesMode::Auto => "auto",
        UpdatesMode::ApprovalRequired => "approval-required",
    }
}

fn telemetry_mode(mode: TelemetryMode) -> &'static str {
    match mode {
        TelemetryMode::Off => "off",
        TelemetryMode::Auto => "auto",
        TelemetryMode::ApprovalRequired => "approval-required",
    }
}

fn heartbeats_mode(mode: HeartbeatsMode) -> &'static str {
    match mode {
        HeartbeatsMode::Off => "off",
        HeartbeatsMode::On => "on",
    }
}

fn network_mode_default(network: Option<&NetworkSettings>) -> &'static str {
    match network {
        Some(NetworkSettings::ByoVpcAws { .. }) => "use-existing",
        Some(NetworkSettings::UseDefault) => "use-default",
        None | Some(NetworkSettings::Create { .. }) => "create-new",
        Some(NetworkSettings::ByoVpcGcp { .. } | NetworkSettings::ByoVnetAzure { .. }) => {
            "create-new"
        }
    }
}

#[derive(Debug)]
struct NetworkParameterDefaults {
    cidr: Option<String>,
    availability_zones: u8,
    vpc_id: Option<String>,
    public_subnet_ids: Vec<String>,
    private_subnet_ids: Vec<String>,
    security_group_ids: Vec<String>,
}

impl NetworkParameterDefaults {
    fn from_settings(network: Option<&NetworkSettings>) -> Self {
        match network {
            None => Self::auto(),
            Some(NetworkSettings::UseDefault) => Self { ..Self::auto() },
            Some(NetworkSettings::Create {
                cidr,
                availability_zones,
            }) => Self {
                cidr: cidr.clone(),
                availability_zones: *availability_zones,
                ..Self::auto()
            },
            Some(NetworkSettings::ByoVpcAws {
                vpc_id,
                public_subnet_ids,
                private_subnet_ids,
                security_group_ids,
            }) => Self {
                vpc_id: Some(vpc_id.clone()),
                public_subnet_ids: public_subnet_ids.clone(),
                private_subnet_ids: private_subnet_ids.clone(),
                security_group_ids: security_group_ids.clone(),
                ..Self::auto()
            },
            Some(NetworkSettings::ByoVpcGcp { .. } | NetworkSettings::ByoVnetAzure { .. }) => {
                Self::auto()
            }
        }
    }

    fn auto() -> Self {
        Self {
            cidr: None,
            availability_zones: 2,
            vpc_id: None,
            public_subnet_ids: vec![],
            private_subnet_ids: vec![],
            security_group_ids: vec![],
        }
    }
}

#[derive(Debug)]
struct DomainParameterDefaults {
    domain_name: Option<String>,
    certificate_arn: Option<String>,
}

impl DomainParameterDefaults {
    fn from_settings(domains: Option<&DomainSettings>) -> Self {
        let Some(domains) = domains else {
            return Self::empty();
        };
        let Some(custom_domains) = &domains.custom_domains else {
            return Self::empty();
        };
        let Some((_resource_id, domain)) = custom_domains.iter().next() else {
            return Self::empty();
        };

        Self {
            domain_name: Some(domain.domain.clone()),
            certificate_arn: domain
                .certificate
                .aws
                .as_ref()
                .map(|certificate| certificate.certificate_arn.clone()),
        }
    }

    fn empty() -> Self {
        Self {
            domain_name: None,
            certificate_arn: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_replaces_intrinsic_expression_with_structured_overlay() {
        let mut base = CfExpression::object([(
            "exposure",
            CfExpression::if_(
                CONDITION_HAS_DOMAIN_NAME,
                CfExpression::object([("mode", CfExpression::from("custom"))]),
                CfExpression::object([("mode", CfExpression::from("generated"))]),
            ),
        )]);
        let overlay = CfExpression::object([(
            "exposure",
            CfExpression::object([
                ("mode", CfExpression::from("generated")),
                (
                    "certificate",
                    CfExpression::object([("mode", CfExpression::from("none"))]),
                ),
            ]),
        )]);

        merge_cf_expression(&mut base, overlay);

        let CfExpression::Object(root) = base else {
            panic!("merged expression should remain an object");
        };
        let exposure = root
            .get("exposure")
            .expect("merged settings should keep exposure");
        let CfExpression::Object(exposure) = exposure else {
            panic!("exposure should be the structured overlay");
        };
        assert_eq!(exposure.get("mode"), Some(&CfExpression::from("generated")));
        assert!(
            !exposure.contains_key("Fn::If"),
            "intrinsic and structured object keys must not be merged"
        );
    }

    #[test]
    fn merge_replaces_structured_expression_with_intrinsic_overlay() {
        let mut base = CfExpression::object([(
            "network",
            CfExpression::object([("type", CfExpression::from("use-default"))]),
        )]);
        let overlay = CfExpression::object([(
            "network",
            CfExpression::if_(
                CONDITION_NETWORK_MODE_CREATE,
                CfExpression::object([("type", CfExpression::from("create"))]),
                CfExpression::no_value(),
            ),
        )]);

        merge_cf_expression(&mut base, overlay);

        let CfExpression::Object(root) = base else {
            panic!("merged expression should remain an object");
        };
        let network = root
            .get("network")
            .expect("merged settings should keep network");
        assert!(
            is_cloudformation_intrinsic(network),
            "intrinsic overlay should replace the structured base"
        );
    }
}
