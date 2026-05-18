use crate::{
    registry::CfRegistry,
    template::{CfExpression, CfMapping, CfOutput, CfParameter, CfResource, CfTemplate},
};
use alien_core::{
    import::EmitContext, ownership_policy_for_resource_type, DomainSettings, ErrorData,
    HeartbeatsMode, Network, NetworkSettings, Platform, Result, Stack, StackSettings,
    TelemetryMode, UpdatesMode, Worker, WorkerCode,
};
use alien_error::AlienError;
use indexmap::{indexmap, IndexMap};
use serde_json::json;
use std::collections::HashSet;

const TEMPLATE_VERSION: &str = "2010-09-09";
const LANGUAGE_EXTENSIONS_TRANSFORM: &str = "AWS::LanguageExtensions";

const PARAM_DEPLOYMENT_GROUP_TOKEN: &str = "DeploymentGroupToken";
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
const OUTPUT_STACK_PREFIX: &str = "DeploymentStackPrefix";
const OUTPUT_PLATFORM: &str = "DeploymentPlatform";
const OUTPUT_REGION: &str = "DeploymentRegion";
const OUTPUT_SETUP_TARGET: &str = "DeploymentSetupTarget";
const OUTPUT_SETUP_FINGERPRINT: &str = "DeploymentSetupFingerprint";
const OUTPUT_SETUP_FINGERPRINT_VERSION: &str = "DeploymentSetupFingerprintVersion";
const OUTPUT_MANAGEMENT_CONFIG: &str = "DeploymentManagementConfig";
const OUTPUT_STACK_SETTINGS: &str = "DeploymentStackSettings";
const OUTPUT_RESOURCES: &str = "DeploymentResources";
const OUTPUT_RESOURCES_CHUNK_BYTES: usize = 3_500;
const STANDARD_OUTPUT_COUNT: usize = 9;
const CLOUDFORMATION_MAX_OUTPUTS: usize = 200;
const MAPPING_REGIONAL_CUSTOM_RESOURCE_SERVICE_TOKENS: &str = "RegionalCustomResourceServiceTokens";
const MAPPING_SERVICE_TOKEN_KEY: &str = "ServiceToken";

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
}

/// Options for CloudFormation generation.
pub struct CloudFormationOptions<'a> {
    /// Per-`(ResourceType, Platform)` emitter dispatch. Most callers pass
    /// [`CfRegistry::built_in()`]; plugin-aware callers extend it before
    /// passing.
    pub registry: &'a CfRegistry,
    pub stack_settings: StackSettings,
    pub setup_target: String,
    pub setup_fingerprint: String,
    pub setup_fingerprint_version: u32,
    pub registration: RegistrationMode,
    pub description: Option<String>,
}

/// Generate a CloudFormation template for a stack.
pub fn generate_cloudformation_template(
    stack: &Stack,
    options: CloudFormationOptions<'_>,
) -> Result<CfTemplate> {
    validate_stack_for_cloudformation(stack)?;
    validate_stack_settings(&options.stack_settings)?;

    let names = logical_names(stack)?;
    let mut template = CfTemplate {
        aws_template_format_version: TEMPLATE_VERSION.to_string(),
        description: Some(
            options
                .description
                .clone()
                .unwrap_or_else(|| format!("Deployment stack for {}", stack.id())),
        ),
        transform: vec![LANGUAGE_EXTENSIONS_TRANSFORM.to_string()],
        metadata: IndexMap::new(),
        parameters: IndexMap::new(),
        mappings: IndexMap::new(),
        conditions: IndexMap::new(),
        resources: IndexMap::new(),
        outputs: IndexMap::new(),
    };

    add_standard_parameters(&mut template, &options.stack_settings);
    add_standard_conditions(&mut template, stack, &options.stack_settings);
    add_console_interface_metadata(&mut template, &options.stack_settings);

    let mut imported_resources = Vec::new();
    let mut emitted_resource_ids: IndexMap<String, Vec<String>> = IndexMap::new();

    for (resource_id, resource) in stack.resources() {
        let resource_type = resource.config.resource_type();
        let ownership = ownership_policy_for_resource_type(resource_type.as_ref());
        if !ownership.should_emit_in_setup(resource.lifecycle) {
            continue;
        }
        let emitter = options.registry.require(&resource_type, Platform::Aws)?;

        let ctx = EmitContext {
            stack,
            resource,
            resource_id,
            platform: Platform::Aws,
            stack_settings: &options.stack_settings,
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

        let import_data = emitter.emit_import_ref(&ctx)?;
        imported_resources.push(CfExpression::object([
            ("id", CfExpression::from(resource_id.as_str())),
            ("type", CfExpression::from(resource_type.as_ref())),
            ("importData", import_data),
        ]));
    }

    let management_config = management_config_expression();
    let stack_settings = stack_settings_expression(&options.stack_settings);
    let resources = CfExpression::list(imported_resources);

    apply_resource_dependencies(stack, &emitted_resource_ids, &mut template);

    if let Some(service_token) = options.registration.service_token(&mut template)? {
        add_custom_resource(
            &mut template,
            service_token,
            management_config.clone(),
            stack_settings.clone(),
            &options,
            resources.clone(),
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
    let yaml = serde_yaml::to_string(template).map_err(|error| {
        AlienError::new(ErrorData::TemplateSerializationFailed {
            format: "CloudFormation YAML".to_string(),
            reason: error.to_string(),
        })
    })?;

    Ok(quote_yaml_1_1_mode_scalars(&yaml))
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
/// mutating resources Alien manages after import.
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

fn add_standard_parameters(template: &mut CfTemplate, settings: &StackSettings) {
    template.parameters.insert(
        PARAM_DEPLOYMENT_GROUP_TOKEN.to_string(),
        string_parameter(
            "Deployment-group token used when registering the resolved stack import.",
            None,
            None,
            true,
        ),
    );
    template.parameters.insert(
        PARAM_MANAGING_ROLE_ARN.to_string(),
        string_parameter(
            "Manager IAM role ARN allowed to assume generated management roles.",
            None,
            None,
            false,
        ),
    );
    template.parameters.insert(
        PARAM_MANAGING_ACCOUNT_ID.to_string(),
        string_parameter(
            "Account ID hosting the manager. Referenced by stack-side IAM policies that scope cross-account image pulls. Empty disables those grants.",
            Some(String::new()),
            None,
            false,
        ),
    );

    add_network_parameters(template, settings.network.as_ref());

    let domain_defaults = DomainParameterDefaults::from_settings(settings.domains.as_ref());
    template.parameters.insert(
        PARAM_DOMAIN_NAME.to_string(),
        string_parameter(
            "Optional domain name for public endpoints. Empty disables custom domains.",
            Some(domain_defaults.domain_name.unwrap_or_default()),
            None,
            false,
        ),
    );
    template.parameters.insert(
        PARAM_HOSTED_ZONE_ID.to_string(),
        string_parameter(
            "Optional Route 53 hosted zone ID for the domain.",
            Some(String::new()),
            None,
            false,
        ),
    );
    template.parameters.insert(
        PARAM_CERTIFICATE_ARN.to_string(),
        string_parameter(
            "Optional ACM certificate ARN for the domain.",
            Some(domain_defaults.certificate_arn.unwrap_or_default()),
            None,
            false,
        ),
    );

    template.parameters.insert(
        PARAM_UPDATES_MODE.to_string(),
        string_parameter(
            "How updates are applied after import.",
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
            Some(vec![
                CfExpression::from("off"),
                CfExpression::from("auto"),
                CfExpression::from("approval-required"),
            ]),
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
}

fn add_network_parameters(template: &mut CfTemplate, network: Option<&NetworkSettings>) {
    let defaults = NetworkParameterDefaults::from_settings(network);
    template.parameters.insert(
        PARAM_NETWORK_MODE.to_string(),
        string_parameter(
            "Choose whether this setup creates a new network, uses an existing network, or uses the default network.",
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
                    "CIDR for created VPCs. Empty uses the generated default.",
                    Some(defaults.cidr.unwrap_or_default()),
                    None,
                    false,
                ),
            );
            template.parameters.insert(
                PARAM_AVAILABILITY_ZONES.to_string(),
                number_parameter(
                    "Number of availability zones to use when creating a VPC.",
                    defaults.availability_zones,
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
                    "Existing VPC ID. Required when Network is use-existing.",
                    Some(defaults.vpc_id.unwrap_or_default()),
                    None,
                    false,
                ),
            );
            template.parameters.insert(
                PARAM_PUBLIC_SUBNET_IDS.to_string(),
                comma_list_parameter("Existing public subnet IDs.", defaults.public_subnet_ids),
            );
            template.parameters.insert(
                PARAM_PRIVATE_SUBNET_IDS.to_string(),
                comma_list_parameter("Existing private subnet IDs.", defaults.private_subnet_ids),
            );
            template.parameters.insert(
                PARAM_SECURITY_GROUP_IDS.to_string(),
                comma_list_parameter("Existing security group IDs.", defaults.security_group_ids),
            );
        }
        None | Some(NetworkSettings::ByoVpcGcp { .. } | NetworkSettings::ByoVnetAzure { .. }) => {}
    }
}

fn add_standard_conditions(template: &mut CfTemplate, stack: &Stack, settings: &StackSettings) {
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
    template.conditions.insert(
        CONDITION_HAS_DOMAIN_NAME.to_string(),
        CfExpression::not(equals_ref(PARAM_DOMAIN_NAME, "")),
    );
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

fn add_console_interface_metadata(template: &mut CfTemplate, settings: &StackSettings) {
    let network_parameters = network_parameter_names(settings.network.as_ref());
    let mut parameter_groups = vec![json!({
        "Label": { "default": "Registration" },
        "Parameters": [
            PARAM_DEPLOYMENT_GROUP_TOKEN,
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
    parameter_groups.push(json!({
        "Label": { "default": "Domains" },
        "Parameters": [PARAM_DOMAIN_NAME, PARAM_HOSTED_ZONE_ID, PARAM_CERTIFICATE_ARN]
    }));
    parameter_groups.push(json!({
        "Label": { "default": "Operations" },
        "Parameters": [PARAM_UPDATES_MODE, PARAM_TELEMETRY_MODE, PARAM_HEARTBEATS_MODE]
    }));

    let mut parameter_labels = serde_json::Map::new();
    insert_parameter_label(
        &mut parameter_labels,
        PARAM_DEPLOYMENT_GROUP_TOKEN,
        "Deployment group token",
    );
    insert_parameter_label(
        &mut parameter_labels,
        PARAM_MANAGING_ROLE_ARN,
        "Manager role ARN",
    );
    insert_parameter_label(
        &mut parameter_labels,
        PARAM_MANAGING_ACCOUNT_ID,
        "Manager account ID",
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
        "DeploymentStackImport".to_string(),
        "AWS::CloudFormation::CustomResource".to_string(),
    );
    resource.depends_on = depends_on;
    // The Custom Resource forwards `DeploymentName` to the Platform's
    // `/v1/deployments/import` endpoint, which defaults to the CFN stack name
    // (`!Ref AWS::StackName`) when the property is absent. We always emit it
    // so the manager-side import row mirrors the CFN stack list verbatim;
    // customers who want a different identity can rewire the property
    // post-rendering.
    resource.properties = indexmap! {
        "ServiceToken".to_string() => service_token,
        "DeploymentGroupToken".to_string() => CfExpression::ref_(PARAM_DEPLOYMENT_GROUP_TOKEN),
        "DeploymentName".to_string() => CfExpression::ref_("AWS::StackName"),
        "StackPrefix".to_string() => CfExpression::ref_("AWS::StackName"),
        "SourceKind".to_string() => CfExpression::from("cloudformation"),
        "Platform".to_string() => CfExpression::from(Platform::Aws.as_str()),
        "Region".to_string() => CfExpression::ref_("AWS::Region"),
        "SetupTarget".to_string() => CfExpression::from(options.setup_target.clone()),
        "SetupFingerprint".to_string() => CfExpression::from(options.setup_fingerprint.clone()),
        "SetupFingerprintVersion".to_string() => CfExpression::from(options.setup_fingerprint_version),
        "ManagementConfig".to_string() => management_config,
        "StackSettings".to_string() => stack_settings,
        "Resources".to_string() => resources,
    };
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
    template.outputs.insert(
        OUTPUT_STACK_PREFIX.to_string(),
        output(
            "Physical stack prefix.",
            CfExpression::ref_("AWS::StackName"),
        ),
    );
    template.outputs.insert(
        OUTPUT_PLATFORM.to_string(),
        output(
            "Target platform.",
            CfExpression::from(Platform::Aws.as_str()),
        ),
    );
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
        OUTPUT_SETUP_FINGERPRINT_VERSION.to_string(),
        output(
            "Setup fingerprint algorithm version.",
            CfExpression::from(options.setup_fingerprint_version),
        ),
    );
    template.outputs.insert(
        OUTPUT_MANAGEMENT_CONFIG.to_string(),
        output(
            "Manager import ManagementConfig JSON.",
            CfExpression::to_json_string(management_config),
        ),
    );
    template.outputs.insert(
        OUTPUT_STACK_SETTINGS.to_string(),
        output(
            "Manager import StackSettings JSON.",
            CfExpression::to_json_string(stack_settings),
        ),
    );
    add_resource_outputs(template, resources)?;
    Ok(())
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
            output("Manager import resources JSON.", value),
        );
        return Ok(());
    }

    for (index, chunk) in chunks.into_iter().enumerate() {
        template.outputs.insert(
            format!("{OUTPUT_RESOURCES}{index}"),
            output(
                "Manager import resources JSON chunk. Reassemble chunks in numeric suffix order.",
                CfExpression::to_json_string(chunk),
            ),
        );
    }

    Ok(())
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

fn stack_settings_expression(settings: &StackSettings) -> CfExpression {
    CfExpression::object([
        ("deploymentModel", CfExpression::from("push")),
        ("updates", CfExpression::ref_(PARAM_UPDATES_MODE)),
        ("telemetry", CfExpression::ref_(PARAM_TELEMETRY_MODE)),
        ("heartbeats", CfExpression::ref_(PARAM_HEARTBEATS_MODE)),
        ("network", network_expression(settings.network.as_ref())),
        ("domains", domains_expression()),
    ])
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
                    "availabilityZones",
                    CfExpression::ref_(PARAM_AVAILABILITY_ZONES),
                ),
            ]),
            CfExpression::if_(
                CONDITION_NETWORK_MODE_USE_EXISTING,
                CfExpression::object([
                    ("type", CfExpression::from("byo-vpc-aws")),
                    ("vpcId", CfExpression::ref_(PARAM_VPC_ID)),
                    (
                        "publicSubnetIds",
                        CfExpression::ref_(PARAM_PUBLIC_SUBNET_IDS),
                    ),
                    (
                        "privateSubnetIds",
                        CfExpression::ref_(PARAM_PRIVATE_SUBNET_IDS),
                    ),
                    (
                        "securityGroupIds",
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

fn management_config_expression() -> CfExpression {
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
    CfParameter {
        parameter_type: "String".to_string(),
        description: Some(description.to_string()),
        default: default.map(CfExpression::from),
        allowed_values,
        no_echo: no_echo.then_some(true),
    }
}

fn number_parameter(
    description: &str,
    default: u8,
    allowed_values: Option<Vec<CfExpression>>,
) -> CfParameter {
    CfParameter {
        parameter_type: "Number".to_string(),
        description: Some(description.to_string()),
        default: Some(CfExpression::from(default)),
        allowed_values,
        no_echo: None,
    }
}

fn comma_list_parameter(description: &str, default: Vec<String>) -> CfParameter {
    CfParameter {
        parameter_type: "CommaDelimitedList".to_string(),
        description: Some(description.to_string()),
        default: Some(CfExpression::from(default.join(","))),
        allowed_values: None,
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
