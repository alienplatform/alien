use crate::{
    emitters::enabled,
    inline_policy::{consolidate_role_inline_policies, expression_references},
    registry::CfRegistry,
    template::{
        CfExpression, CfMapping, CfOutput, CfParameter, CfResource, CfRule, CfRuleAssertion,
        CfTemplate,
    },
};
use alien_core::{
    import::{EmitContext, CURRENT_SETUP_IMPORT_FORMAT_VERSION},
    ownership_policy_for_resource_type, CapacityGroup, CapacityGroupScalePolicy, ComputeCluster,
    ComputePoolSelection, Container, Daemon, DeploymentModel, DomainSettings, ErrorData,
    ExposeProtocol, HeartbeatsMode, KubernetesCluster, KubernetesSettings, Network,
    NetworkSettings, Platform, RemoteBindings, ResourceLifecycle, Result, Sandbox, Stack,
    StackInputDefaultValue, StackInputDefinition, StackInputKind, StackInputProvider,
    StackSettings, Storage, TelemetryMode, UpdatesMode, Worker, WorkerCode,
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
const PARAM_ENDPOINT_ACCESS: &str = "EndpointAccess";
const PARAM_NETWORK_MODE: &str = "NetworkMode";
const PARAM_VPC_CIDR: &str = "VpcCidr";
const PARAM_AVAILABILITY_ZONES: &str = "AvailabilityZones";
const PARAM_VPC_ID: &str = "VpcId";
const PARAM_PUBLIC_SUBNET_IDS: &str = "PublicSubnetIds";
const PARAM_PRIVATE_SUBNET_IDS: &str = "PrivateSubnetIds";
const PARAM_SECURITY_GROUP_IDS: &str = "SecurityGroupIds";
const PARAM_DOMAIN_NAME: &str = "DomainName";
const PARAM_DOMAIN_RESOURCE: &str = "DomainResource";
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

/// Setup resources that require subnet IDs cannot use implicit default networking.
/// EKS also requires explicit subnets: this package does not discover the default VPC.
fn restricts_network_mode(stack: &Stack, target: CloudFormationTarget) -> bool {
    target.is_kubernetes() || alien_core::restricts_network_mode(stack, false)
}

/// Generate a CloudFormation template for a stack.
pub fn generate_cloudformation_template(
    stack: &Stack,
    options: CloudFormationOptions<'_>,
) -> Result<CfTemplate> {
    validate_stack_for_cloudformation(stack)?;
    validate_stack_settings(&options.stack_settings)?;
    if options.target.is_kubernetes()
        && (matches!(
            options.stack_settings.network,
            Some(NetworkSettings::Create {
                availability_zones: 1,
                ..
            })
        ) || stack.resources().any(|(_, entry)| {
            entry
                .config
                .downcast_ref::<Network>()
                .is_some_and(|network| {
                    matches!(
                        network.settings,
                        NetworkSettings::Create {
                            availability_zones: 1,
                            ..
                        }
                    )
                })
        }))
    {
        return Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "generate EKS CloudFormation package".to_string(),
            reason: "EKS requires at least two distinct Availability Zones".to_string(),
        }));
    }

    if options.target.is_kubernetes()
        && (matches!(
            options.stack_settings.network,
            Some(NetworkSettings::UseDefault)
        ) || stack.resources().any(|(_, entry)| {
            entry
                .config
                .downcast_ref::<Network>()
                .is_some_and(|network| matches!(network.settings, NetworkSettings::UseDefault))
        }))
    {
        return Err(AlienError::new(ErrorData::OperationNotSupported {
            operation: "generate EKS CloudFormation package".to_string(),
            reason: "CloudFormation EKS requires create-new networking or an existing VPC with explicit subnet IDs; automatic default-VPC discovery is not supported".to_string(),
        }));
    }

    let mut stack_settings = options.stack_settings.clone();
    if options.target.is_kubernetes() {
        // Kubernetes compute pools use existing nodes, not cloud fleet choices.
        if let Some(compute) = stack_settings.compute.as_mut() {
            compute.pools.clear();
            if compute.containers.is_empty() {
                stack_settings.compute = None;
            }
        }
    }
    // CloudFormation packages always register push deployments.
    stack_settings.deployment_model = DeploymentModel::Push;
    if options.target.is_kubernetes() && stack_settings.network.is_none() {
        stack_settings.network = Some(NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        });
    }

    let names = logical_names(stack)?;
    // CREATE_COMPLETE reads as "the package is installed". For a runtime-provisioned sandbox the
    // image does not exist yet, and the stack description is the one line every console shows.
    // Not on a Kubernetes target: the sandbox is skipped there, so no build follows registration.
    let runtime_sandbox = !options.target.is_kubernetes()
        && stack.resources().any(|(_, entry)| {
            entry.config.resource_type().0.as_ref() == Sandbox::RESOURCE_TYPE.as_ref()
                && entry.lifecycle == ResourceLifecycle::Live
        });
    let mut template = CfTemplate {
        aws_template_format_version: TEMPLATE_VERSION.to_string(),
        description: Some({
            let base = options
                .description
                .clone()
                .unwrap_or_else(|| format!("Application setup stack for {}", stack.id()));
            if runtime_sandbox {
                format!(
                    "{base}. The sandbox image is built after the deployment registers, so a \
                     completed stack does not mean the sandbox can accept sessions yet."
                )
            } else {
                base
            }
        }),
        transform: vec![LANGUAGE_EXTENSIONS_TRANSFORM.to_string()],
        metadata: IndexMap::new(),
        parameters: IndexMap::new(),
        mappings: IndexMap::new(),
        conditions: IndexMap::new(),
        rules: IndexMap::new(),
        resources: IndexMap::new(),
        outputs: IndexMap::new(),
    };

    // CloudFormation exposes one DomainName/CertificateArn pair and registers it under the
    // selected public resource. Reject settings it cannot represent instead of moving a
    // configured hostname onto a different workload.
    if let Some(custom_domains) = stack_settings
        .domains
        .as_ref()
        .and_then(|domains| domains.custom_domains.as_ref())
        .filter(|_| !options.target.is_kubernetes())
    {
        if custom_domains.len() > 1 {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "generate CloudFormation custom domain settings".to_string(),
                reason: "CloudFormation exposes one DomainName/CertificateArn pair. Configure one custom-domain resource or use Terraform for multiple domains.".to_string(),
            }));
        }
        let public_resources = public_http_resource_ids(stack);
        if let Some(id) = custom_domains
            .keys()
            .find(|id| !public_resources.contains(id))
        {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "generate CloudFormation custom domain settings".to_string(),
                reason: format!(
                    "Custom domain resource '{id}' must name a public HTTP resource in this stack."
                ),
            }));
        }
    }
    let supports_custom_domain = stack_supports_custom_domain(stack, options.target);
    let access_only = stack
        .resources
        .values()
        .any(|entry| alien_core::remote_bindings::remote_binding_for_entry(entry).is_some())
        && !stack
            .resources
            .values()
            .any(|entry| entry.lifecycle == ResourceLifecycle::Live);
    let stack_inputs = stack_inputs_for_cloudformation(stack, options.target);

    add_standard_parameters(
        &mut template,
        stack,
        &stack_settings,
        supports_custom_domain,
        access_only,
        !matches!(options.registration, RegistrationMode::OutputsFallback),
        options.target,
    )?;
    add_stack_input_parameters(&mut template, &stack_inputs)?;
    add_supported_region_rule(&mut template, &options.registration);
    if supports_custom_domain {
        add_custom_domain_certificate_rule(&mut template);
    }
    if !access_only {
        add_standard_conditions(
            &mut template,
            stack,
            &stack_settings,
            supports_custom_domain,
            options.target,
        );
    }
    add_console_interface_metadata(
        &mut template,
        stack,
        &stack_settings,
        supports_custom_domain,
        &stack_inputs,
        access_only,
        !matches!(options.registration, RegistrationMode::OutputsFallback),
    );

    let mut registration_resources: Vec<RegistrationEntry> = Vec::new();
    let mut emitted_resource_ids: IndexMap<String, Vec<String>> = IndexMap::new();
    let mut secrets_vault_binding: Option<CfExpression> = None;

    for (resource_id, resource) in stack.resources() {
        let resource_type = resource.config.resource_type();
        let ownership = ownership_policy_for_resource_type(resource_type.as_ref());
        if !ownership.emits_setup_scaffolding(resource.lifecycle) {
            continue;
        }
        // A sandbox on a Kubernetes target is a pod bounded by the chart's NetworkPolicy, not
        // cloud infrastructure. Emitter lookup keys off the cloud the cluster runs in, so without
        // this an EKS install would provision the MicroVM image, connector, security group and
        // roles of the AWS backend it never uses, and register import data naming it. The
        // Terraform generator refuses the same way; the two formats have to install the same
        // thing from one declaration.
        // Logical ComputeCluster pools belong to the Kubernetes operator too;
        // they must not emit a second cloud fleet or cloud import payload.
        if options.target.is_kubernetes()
            && (resource_type == Sandbox::RESOURCE_TYPE
                || resource_type == ComputeCluster::RESOURCE_TYPE)
        {
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
            targets_kubernetes: options.target.is_kubernetes(),
            stack_settings: &stack_settings,
            names: &names,
        };

        let enabled_when = resource.enabled_when.as_deref();
        let declared_condition = if let Some(input_id) = enabled_when {
            // The same policy the compile-time check enforces, re-checked at
            // render time so a caller that skips preflights cannot gate what
            // the policy refuses.
            if let Some(refusal) =
                alien_core::gate_refusal(resource_type.as_ref(), resource_id.as_str())
            {
                return Err(AlienError::new(ErrorData::OperationNotSupported {
                    operation: format!(
                        "enabled() on resource '{resource_id}' of type '{resource_type}'"
                    ),
                    reason: refusal.reason().to_string(),
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
                // (aws/network.rs, aws/kubernetes_cluster.rs) belong to types
                // the gateability policy refuses, so they fail the check above
                // first.
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

        if resource_id.as_str() == alien_core::SECRETS_VAULT_ID {
            secrets_vault_binding = emitter.emit_binding_ref(&ctx)?;
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
        access_only,
    );
    apply_resource_dependencies(stack, &emitted_resource_ids, &mut template);
    apply_network_iam_dependencies(stack, &emitted_resource_ids, &mut template);
    consolidate_role_inline_policies(&mut template)?;

    add_deployer_secret_outputs(
        &mut template,
        stack,
        options.target,
        secrets_vault_binding.as_ref(),
    )?;

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

fn public_http_resource_ids(stack: &Stack) -> Vec<String> {
    let mut ids: Vec<_> = stack
        .resources()
        .filter_map(|(id, entry)| {
            let public = entry
                .config
                .downcast_ref::<Worker>()
                .is_some_and(|worker| !worker.public_endpoints.is_empty())
                || entry
                    .config
                    .downcast_ref::<Container>()
                    .is_some_and(|container| {
                        container
                            .public_endpoints
                            .iter()
                            .any(|endpoint| endpoint.protocol == ExposeProtocol::Http)
                    })
                || entry.config.downcast_ref::<Daemon>().is_some_and(|daemon| {
                    daemon
                        .public_endpoints
                        .iter()
                        .any(|endpoint| endpoint.protocol == ExposeProtocol::Http)
                });
            public.then(|| id.to_string())
        })
        .collect();
    ids.sort();
    ids
}

fn stack_supports_custom_domain(stack: &Stack, target: CloudFormationTarget) -> bool {
    target.is_kubernetes() || !public_http_resource_ids(stack).is_empty()
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
                // Deployer secrets are written into the customer's own secret
                // store, never passed through the template.
                && !alien_core::is_deployer_secret_input(input)
                && input
                    .platforms
                    .as_ref()
                    .is_none_or(|platforms| platforms.contains(&platform))
        })
        .cloned()
        .collect()
}

/// One output per deployer secret, naming where the deployer writes its value.
///
/// On AWS that is the SSM parameter in the stack's `secrets` vault. On a
/// Kubernetes target the value is a Secret in the deployment's namespace whose
/// name the operator derives, so the output carries the vault key and the
/// deployment status shows the full name.
fn add_deployer_secret_outputs(
    template: &mut CfTemplate,
    stack: &Stack,
    target: CloudFormationTarget,
    secrets_vault_binding: Option<&CfExpression>,
) -> Result<()> {
    let platform = target.deployment_platform();
    let inputs: Vec<&StackInputDefinition> = stack
        .inputs()
        .iter()
        .filter(|input| {
            alien_core::is_deployer_secret_input(input)
                && input
                    .platforms
                    .as_ref()
                    .is_none_or(|platforms| platforms.contains(&platform))
        })
        .collect();
    if inputs.is_empty() {
        return Ok(());
    }

    let vault_prefix = if target.is_kubernetes() {
        None
    } else {
        let prefix = match secrets_vault_binding {
            Some(CfExpression::Object(binding)) => binding.get("vaultPrefix").cloned(),
            _ => None,
        };
        Some(prefix.ok_or_else(|| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: "generate_cloudformation_template".to_string(),
                reason: format!(
                    "deployer secret inputs live in the '{}' vault, which this stack does not \
                     emit; run the stack through preflights so the vault is added",
                    alien_core::SECRETS_VAULT_ID
                ),
            })
        })?)
    };

    for input in inputs {
        let vault_key = alien_core::deployer_secret_vault_key(&input.id);
        let output_name = format!("DeployerSecret{}", sanitize_logical_id(&input.id));
        if template.outputs.contains_key(&output_name) {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "generate_cloudformation_template".to_string(),
                reason: format!(
                    "stack input '{}' normalizes to CloudFormation output '{output_name}', \
                     which another input already claimed; rename one",
                    input.id
                ),
            }));
        }
        let (description, value) = match &vault_prefix {
            Some(prefix) => (
                format!(
                    "SSM SecureString parameter to write the '{}' value into.",
                    input.label
                ),
                CfExpression::join(
                    "-",
                    CfExpression::list([prefix.clone(), CfExpression::from(vault_key)]),
                ),
            ),
            None => (
                format!(
                    "Secrets vault key for '{}'; the deployment status shows the Kubernetes \
                     Secret to create.",
                    input.label
                ),
                CfExpression::from(vault_key),
            ),
        };
        template
            .outputs
            .insert(output_name, output(&description, value));
    }
    Ok(())
}

fn stack_input_parameter_name(input: &StackInputDefinition) -> String {
    stack_input_parameter_name_for_id(&input.id)
}

/// Parameter name for a stack input id. Shared with the gating helpers, which
/// only know the id.
pub(crate) fn stack_input_parameter_name_for_id(input_id: &str) -> String {
    format!("Input{}", sanitize_logical_id(input_id))
}

fn add_stack_input_parameters(
    template: &mut CfTemplate,
    inputs: &[StackInputDefinition],
) -> Result<()> {
    for input in inputs {
        let parameter_name = stack_input_parameter_name(input);
        // Distinct ids can sanitize to the same logical id (`fooBar` and
        // `foo_bar` both become `InputFooBar`); a silent overwrite would make
        // both inputs — gates and their conditions included — read one
        // parameter.
        if template.parameters.contains_key(&parameter_name) {
            return Err(AlienError::new(ErrorData::OperationNotSupported {
                operation: "generate_cloudformation_template".to_string(),
                reason: format!(
                    "stack input '{}' normalizes to CloudFormation parameter \
                     '{parameter_name}', which another input already claimed; rename one so \
                     every input keeps its own parameter",
                    input.id
                ),
            }));
        }
        template
            .parameters
            .insert(parameter_name, stack_input_parameter(input));
    }
    Ok(())
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
        let replacement = trimmed
            .strip_suffix(": on")
            .map(|key| format!("{key}: \"on\""))
            .or_else(|| {
                trimmed
                    .strip_suffix(": off")
                    .map(|key| format!("{key}: \"off\""))
            })
            .or_else(|| match trimmed {
                "- on" => Some("- \"on\"".to_string()),
                "- off" => Some("- \"off\"".to_string()),
                _ => None,
            });

        if let Some(replacement) = replacement {
            quoted.push_str(indent);
            quoted.push_str(&replacement);
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

/// Lambda deletes a worker's Hyperplane ENI with the function's execution
/// role and its permissions. If those are deleted first, the ENI stays and the
/// private subnets and security group it sits in can never be deleted. The
/// created network's private subnets and security group therefore depend on
/// every IAM role and policy, so CloudFormation deletes IAM last. Skipped:
/// conditional IAM resources (`DependsOn` cannot name a resource whose
/// condition is false) and IAM resources that already depend on the network,
/// such as an IRSA role trusting a cluster placed in it.
fn apply_network_iam_dependencies(
    stack: &Stack,
    emitted_resource_ids: &IndexMap<String, Vec<String>>,
    template: &mut CfTemplate,
) {
    const IAM_TYPES: [&str; 3] = [
        "AWS::IAM::Role",
        "AWS::IAM::Policy",
        "AWS::IAM::ManagedPolicy",
    ];
    let Some(network_id) = stack
        .resources()
        .find_map(|(id, entry)| entry.config.downcast_ref::<Network>().map(|_| id))
    else {
        return;
    };
    let targets: Vec<String> = emitted_resource_ids
        .get(network_id)
        .into_iter()
        .flatten()
        .filter(|logical_id| {
            template.resources.get(*logical_id).is_some_and(|resource| {
                match resource.resource_type.as_str() {
                    "AWS::EC2::Subnet" => logical_id.contains("PrivateSubnet"),
                    "AWS::EC2::SecurityGroup" => true,
                    _ => false,
                }
            })
        })
        .cloned()
        .collect();
    if targets.is_empty() {
        return;
    }

    let logical_ids: Vec<&String> = template.resources.keys().collect();
    let direct_dependencies: IndexMap<&String, Vec<&String>> = template
        .resources
        .iter()
        .map(|(logical_id, resource)| {
            let dependencies = logical_ids
                .iter()
                .copied()
                .filter(|other| {
                    *other != logical_id
                        && (resource.depends_on.contains(other)
                            || resource
                                .properties
                                .values()
                                .any(|expression| expression_references(expression, other)))
                })
                .collect();
            (logical_id, dependencies)
        })
        .collect();
    let reaches_target = |start: &String| -> bool {
        let mut pending = vec![start];
        let mut seen = std::collections::HashSet::new();
        while let Some(logical_id) = pending.pop() {
            if !seen.insert(logical_id) {
                continue;
            }
            if targets.contains(logical_id) {
                return true;
            }
            pending.extend(
                direct_dependencies
                    .get(logical_id)
                    .into_iter()
                    .flatten()
                    .copied(),
            );
        }
        false
    };
    let iam_logical_ids: Vec<String> = template
        .resources
        .iter()
        .filter(|(logical_id, resource)| {
            IAM_TYPES.contains(&resource.resource_type.as_str())
                && resource.condition.is_none()
                && !reaches_target(logical_id)
        })
        .map(|(logical_id, _)| logical_id.clone())
        .collect();

    for target in &targets {
        let Some(resource) = template.resources.get_mut(target) else {
            continue;
        };
        for logical_id in &iam_logical_ids {
            if !resource.depends_on.contains(logical_id) {
                resource.depends_on.push(logical_id.clone());
            }
        }
    }
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
            // The resource-specific grant already depends on both the bindings identity and the
            // physical resource. The executor edge exists so direct setup attaches that grant only
            // after identity creation; it should not make generated physical resources wait on the
            // identity itself.
            if stack
                .resources
                .get(dependency.id())
                .is_some_and(|entry| entry.config.downcast_ref::<RemoteBindings>().is_some())
            {
                continue;
            }
            // Node grants reference both the role and Storage in their own policy. The
            // executor edge orders cluster completion, not creation of every fragment:
            // stamping it onto the role can cycle through a workload identity. Explicit
            // edges to the same target have no separate origin and retain this ordering
            // through the policy's actual references.
            if dependency.resource_type() == &Storage::RESOURCE_TYPE
                && stack.resources.get(dependency.id()).is_some_and(|target| {
                    target
                        .config
                        .downcast_ref::<Storage>()
                        .is_some_and(|storage| storage.id == dependency.id())
                })
                && entry
                    .config
                    .downcast_ref::<ComputeCluster>()
                    .and_then(|cluster| cluster.node_permissions.as_ref())
                    .and_then(|profile| profile.0.get(dependency.id()))
                    .is_some_and(|references| !references.is_empty())
            {
                continue;
            }
            let targets = dependency_targets.get(dependency.id());
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

fn add_standard_parameters(
    template: &mut CfTemplate,
    stack: &Stack,
    settings: &StackSettings,
    supports_custom_domain: bool,
    bindings_only: bool,
    uses_custom_registration: bool,
    target: CloudFormationTarget,
) -> Result<()> {
    if uses_custom_registration {
        template.parameters.insert(
            PARAM_TOKEN.to_string(),
            string_parameter(
                "Install token from the application setup page.",
                None,
                None,
                true,
            ),
        );
    }
    template.parameters.insert(
        PARAM_MANAGING_ROLE_ARN.to_string(),
        string_parameter(
            "ARN of the management identity allowed to assume setup-created roles.",
            (!bindings_only).then(String::new),
            None,
            false,
        ),
    );
    if bindings_only {
        return Ok(());
    }
    template.parameters.insert(
        PARAM_MANAGING_ACCOUNT_ID.to_string(),
        string_parameter(
            "AWS account ID for the management account that hosts application container images.",
            Some(String::new()),
            None,
            false,
        ),
    );

    add_network_parameters(template, stack, settings.network.as_ref(), target);
    add_container_resource_parameters(template, stack, settings.compute.as_ref())?;
    if !target.is_kubernetes() {
        add_compute_parameters(template, stack, settings.compute.as_ref())?;
    }

    if supports_custom_domain {
        let domain_defaults = DomainParameterDefaults::from_settings(settings.domains.as_ref());
        if !target.is_kubernetes() && public_http_resource_ids(stack).len() > 1 {
            let ids = public_http_resource_ids(stack);
            let default = domain_defaults
                .resource_id
                .as_ref()
                .cloned()
                .unwrap_or_else(|| ids[0].clone());
            template.parameters.insert(
                PARAM_DOMAIN_RESOURCE.to_string(),
                string_parameter(
                    "Public HTTP resource that will use DomainName and CertificateArn.",
                    Some(default),
                    Some(ids.into_iter().map(CfExpression::from).collect()),
                    false,
                ),
            );
        }
        template.parameters.insert(
            PARAM_DOMAIN_NAME.to_string(),
            string_parameter(
                "Optional custom domain. Leave unset to use the deployment-managed endpoint.",
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
        PARAM_ENDPOINT_ACCESS.to_string(),
        string_parameter(
            "Who can reach this deployment's endpoints. Private access requires AWS managed containers and cannot change after setup.",
            Some(settings.endpoint_access.as_str().to_string()),
            Some(vec![CfExpression::from("internet"), CfExpression::from("private")]),
            false,
        ),
    );
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

fn add_network_parameters(
    template: &mut CfTemplate,
    stack: &Stack,
    network: Option<&NetworkSettings>,
    target: CloudFormationTarget,
) {
    let defaults = NetworkParameterDefaults::from_settings(network);
    let mut modes = vec![
        CfExpression::from("create-new"),
        CfExpression::from("use-existing"),
    ];
    // A sandbox's egress connector must name subnets, and CloudFormation cannot resolve the
    // account default VPC's. Withholding the value is what makes the answer unpickable: the
    // parameter is checked before the transform runs, where a template Rule is not yet reached.
    let restricted = restricts_network_mode(stack, target);
    let description = if restricted {
        "Choose create-new for a managed VPC, or use-existing for your VPC. This application's \
         private resources require explicit subnet IDs, which setup cannot discover from the \
         account default VPC."
    } else {
        modes.push(CfExpression::from("use-default"));
        "Choose create-new for a managed VPC, use-existing for your VPC, or use-default for the account default VPC."
    };
    // The declared network and the sandbox's connector are read from different places, so a stack
    // can ask for the account default while its sandbox forbids it. A default outside the allowed
    // values is rejected by CreateStack as a malformed template, which reads as a broken vendor
    // artifact rather than as the answer being unavailable.
    let default_mode = match network_mode_default(network) {
        "use-default" if restricted => "create-new",
        mode => mode,
    };
    template.parameters.insert(
        PARAM_NETWORK_MODE.to_string(),
        string_parameter(
            description,
            Some(default_mode.to_string()),
            Some(modes),
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
                    "Only used with create-new. CIDR for the new VPC; leave unset for the generated default.",
                    Some(defaults.cidr.unwrap_or_default()),
                    None,
                    false,
                ),
            );
            template.parameters.insert(
                PARAM_AVAILABILITY_ZONES.to_string(),
                number_parameter(
                    "Only used with create-new. Number of availability zones for the new VPC.",
                    u32::from(defaults.availability_zones),
                    Some(if target.is_kubernetes() {
                        vec![CfExpression::from(2u8), CfExpression::from(3u8)]
                    } else {
                        vec![
                            CfExpression::from(1u8),
                            CfExpression::from(2u8),
                            CfExpression::from(3u8),
                        ]
                    }),
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
        // Resource parameters can lower the release defaults, so a machine list
        // filtered against those defaults would reject valid smaller machines.
        // Preflight validates the selected machine against the selected resources.
        let has_resource_choices = stack.resources.values().any(|entry| {
            entry
                .config
                .downcast_ref::<alien_core::Container>()
                .is_some_and(|container| container.resource_choices.is_some())
        });
        let allowed_values = (!has_resource_choices)
            .then(|| &plan)
            .into_iter()
            .flat_map(|plan| &plan.pools)
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

fn add_container_resource_parameters(
    template: &mut CfTemplate,
    stack: &Stack,
    compute: Option<&alien_core::ComputeSettings>,
) -> Result<()> {
    // Planning validates both the declared ranges and the supplied defaults first.
    let plan = alien_core::compute_planner::plan_compute(stack, Platform::Aws, compute)?;
    for container in plan.containers {
        let prefix = format!("Container{}", pascal_identifier(&container.container_id));
        if let Some(range) = container.choices.cpu {
            let mut parameter =
                number_parameter("CPU allocation per container replica, in vCPUs.", 1, None);
            parameter.default = Some(CfExpression::Number(
                alien_core::instance_catalog::parse_cpu(&container.cpu.desired)
                    .expect("planner validated CPU"),
            ));
            parameter.min_value = Some(CfExpression::Number(
                alien_core::instance_catalog::parse_cpu(&range.min).expect("planner validated CPU"),
            ));
            parameter.max_value = Some(CfExpression::Number(
                alien_core::instance_catalog::parse_cpu(&range.max).expect("planner validated CPU"),
            ));
            template
                .parameters
                .insert(format!("{prefix}Cpu"), parameter);
        }
        if container.choices.memory.is_some() {
            template.parameters.insert(
                format!("{prefix}Memory"),
                string_parameter(
                    "Memory allocation per container replica, using Ki, Mi, Gi, or Ti.",
                    Some(container.memory.desired),
                    None,
                    false,
                ),
            );
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
    target: CloudFormationTarget,
) {
    let has_created_network = stack_has_created_network(stack);
    if has_dynamic_aws_network_settings(settings.network.as_ref()) || has_created_network {
        template.conditions.insert(
            CONDITION_NETWORK_MODE_CREATE.to_string(),
            equals_ref(PARAM_NETWORK_MODE, "create-new"),
        );
        // Without use-default there is nothing left for this to distinguish, and every branch it
        // guarded has collapsed, so defining it would leave an unused condition behind.
        //
        // That this stays in step with whoever still references the condition is not left to
        // reading: cfn-lint runs on every rendered template in these tests and fails both ways —
        // W8001 for a condition defined and unused, an error for one referenced and undefined.
        if !restricts_network_mode(stack, target) {
            template.conditions.insert(
                CONDITION_NETWORK_MODE_USE_EXISTING.to_string(),
                equals_ref(PARAM_NETWORK_MODE, "use-existing"),
            );
        }
        if !restricts_network_mode(stack, target)
            && has_created_network
            && stack.resources().any(|(_id, entry)| {
                entry.lifecycle == alien_core::ResourceLifecycle::Frozen
                    && entry
                        .config
                        .downcast_ref::<alien_core::Postgres>()
                        .is_some()
            })
        {
            template.conditions.insert(
                crate::emitters::aws::helpers::CONDITION_NETWORK_MODE_HAS_NAMED_SUBNETS.to_string(),
                CfExpression::or([
                    equals_ref(PARAM_NETWORK_MODE, "create-new"),
                    equals_ref(PARAM_NETWORK_MODE, "use-existing"),
                ]),
            );
        }
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
        if !target.is_kubernetes() && public_http_resource_ids(stack).len() > 1 {
            let ids = public_http_resource_ids(stack);
            for (index, id) in ids.iter().enumerate().take(ids.len() - 1) {
                template.conditions.insert(
                    format!("CustomDomainResource{index}"),
                    equals_ref(PARAM_DOMAIN_RESOURCE, id),
                );
            }
        }
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
    bindings_only: bool,
    uses_custom_registration: bool,
) {
    let network_parameters = network_parameter_names(settings.network.as_ref());
    let compute_parameters = compute_parameter_names(stack, settings.compute.as_ref());
    let registration_parameters = if uses_custom_registration {
        vec![PARAM_TOKEN, PARAM_MANAGING_ROLE_ARN]
    } else {
        vec![PARAM_MANAGING_ROLE_ARN]
    };
    let mut parameter_groups = vec![json!({
        "Label": { "default": "Registration" },
        "Parameters": registration_parameters
    })];
    if !bindings_only {
        parameter_groups[0]["Parameters"]
            .as_array_mut()
            .expect("registration parameter group is an array")
            .push(json!(PARAM_MANAGING_ACCOUNT_ID));
    }
    if !network_parameters.is_empty() {
        parameter_groups.push(json!({
            "Label": { "default": "Network" },
            "Parameters": network_parameters
        }));
    }
    if supports_custom_domain {
        let mut parameters = vec![
            PARAM_DOMAIN_NAME,
            PARAM_HOSTED_ZONE_ID,
            PARAM_CERTIFICATE_ARN,
        ];
        if template.parameters.contains_key(PARAM_DOMAIN_RESOURCE) {
            parameters.insert(0, PARAM_DOMAIN_RESOURCE);
        }
        parameter_groups.push(json!({
            "Label": { "default": "Custom domain" },
            "Parameters": parameters
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
    if !bindings_only {
        parameter_groups.push(json!({
            "Label": { "default": "Operations" },
            "Parameters": [
                PARAM_UPDATES_MODE,
                PARAM_TELEMETRY_MODE,
                PARAM_HEARTBEATS_MODE
            ]
        }));
    }

    let mut parameter_labels = serde_json::Map::new();
    if uses_custom_registration {
        insert_parameter_label(&mut parameter_labels, PARAM_TOKEN, "Install token");
    }
    insert_parameter_label(
        &mut parameter_labels,
        PARAM_MANAGING_ROLE_ARN,
        "Management role ARN",
    );
    if !bindings_only {
        insert_parameter_label(
            &mut parameter_labels,
            PARAM_MANAGING_ACCOUNT_ID,
            "Image account ID",
        );
    }
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
    if !bindings_only {
        insert_parameter_label(&mut parameter_labels, PARAM_UPDATES_MODE, "Updates");
        insert_parameter_label(&mut parameter_labels, PARAM_TELEMETRY_MODE, "Telemetry");
        insert_parameter_label(&mut parameter_labels, PARAM_HEARTBEATS_MODE, "Heartbeats");
    }

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
    let mut fields = Vec::new();
    if !pools.is_empty() {
        fields.push(("pools", CfExpression::object(pools)));
    }
    let mut containers = Vec::new();
    let mut ids: Vec<_> = stack.resources.keys().collect();
    ids.sort();
    for id in ids {
        let Some(container) = stack.resources[id]
            .config
            .downcast_ref::<alien_core::Container>()
        else {
            continue;
        };
        let Some(choices) = &container.resource_choices else {
            continue;
        };
        let prefix = format!("Container{}", pascal_identifier(id));
        let mut allocation = Vec::new();
        if choices.cpu.is_some() {
            allocation.push(("cpu", CfExpression::ref_(format!("{prefix}Cpu"))));
        }
        if choices.memory.is_some() {
            allocation.push(("memory", CfExpression::ref_(format!("{prefix}Memory"))));
        }
        if !allocation.is_empty() {
            containers.push((id.as_str(), CfExpression::object(allocation)));
        }
    }
    if !containers.is_empty() {
        fields.push(("containers", CfExpression::object(containers)));
    }
    (!fields.is_empty()).then(|| CfExpression::object(fields))
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
    resources: &[RegistrationEntry],
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

/// One resource's registration entry, kept beside the gate that decides whether
/// the deployer wanted it.
///
/// The two registration paths need different forms of the same two pieces, so
/// the gate stays separate until each one renders it: the custom resource gates
/// the entry object, the Outputs fallback gates the entry's JSON text.
struct RegistrationEntry {
    /// Stack input id the deployer answers, or `None` when the resource is
    /// created unconditionally.
    enabled_when: Option<String>,
    entry: CfExpression,
}

impl RegistrationEntry {
    /// Gates a rendered value on this entry's input: a declined entry
    /// resolves to `AWS::NoValue`, which removes the element from its list.
    fn gated(&self, value: CfExpression) -> CfExpression {
        enabled::when_enabled(
            self.enabled_when.as_deref(),
            value,
            CfExpression::no_value(),
        )
    }

    /// Element for the custom resource's `Resources` property.
    fn custom_resource_element(&self) -> CfExpression {
        self.gated(self.entry.clone())
    }

    /// The entry's JSON text for the Outputs fallback, gated so a declined entry
    /// resolves to `AWS::NoValue`.
    ///
    /// `Fn::ToJsonString` sits *inside* the gate on purpose. Wrapping the gate
    /// instead — `ToJsonString(Fn::If(..))` — serializes a declined entry as the
    /// literal `null`, which is the bug this construction exists to avoid.
    ///
    /// The `Fn::Sub` around it only launders the type. CloudFormation resolves a
    /// bare `Fn::ToJsonString` inside `Fn::Join` correctly, but cfn-lint does not
    /// infer that it returns a string and fails the template with E6101, and the
    /// only way to silence that is a template-wide suppression that would also
    /// mask real E6101s. Substitution is a single pass, so JSON containing a
    /// literal `${...}` passes through untouched.
    fn outputs_element(&self) -> CfExpression {
        let json_text = CfExpression::sub_with(
            format!("${{{ENTRY_JSON_SUB_VARIABLE}}}"),
            [(
                ENTRY_JSON_SUB_VARIABLE,
                CfExpression::to_json_string(self.entry.clone()),
            )],
        );
        self.gated(json_text)
    }
}

/// Renders a chunk of entries as the JSON array text a stack output carries.
///
/// With no gated entry this is `Fn::ToJsonString` over the whole list, which
/// keeps an ungated stack's output unchanged on re-apply.
///
/// Once any entry is gated that no longer works. `Fn::ToJsonString` does not
/// honour `AWS::NoValue`: a declined entry survives as a literal `null`, and
/// registration runs the typed importer over every element it receives, so the
/// null fails deserialization rather than being skipped. `Fn::Join` does drop
/// `AWS::NoValue` elements — without leaving a stray delimiter, and collapsing
/// to `[]` when every entry is declined — so gated chunks convert each entry to
/// JSON text on its own and join the array together.
fn resources_json(entries: &[RegistrationEntry]) -> CfExpression {
    if entries.iter().all(|entry| entry.enabled_when.is_none()) {
        return CfExpression::to_json_string(CfExpression::list(
            entries.iter().map(|entry| entry.entry.clone()),
        ));
    }

    CfExpression::join(
        "",
        CfExpression::list([
            CfExpression::from("["),
            CfExpression::join(
                ",",
                CfExpression::list(entries.iter().map(RegistrationEntry::outputs_element)),
            ),
            CfExpression::from("]"),
        ]),
    )
}

fn add_resource_outputs(template: &mut CfTemplate, entries: &[RegistrationEntry]) -> Result<()> {
    let chunks = chunk_registration_entries(entries)?;
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
        let value = if chunk.is_empty() {
            CfExpression::from("[]")
        } else {
            resources_json(chunk)
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
                resources_json(chunk),
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

/// Splits entries into contiguous groups that each stay under the per-output
/// byte budget, sized against the JSON text an entry renders to.
fn chunk_registration_entries(entries: &[RegistrationEntry]) -> Result<Vec<&[RegistrationEntry]>> {
    if entries.is_empty() {
        return Ok(vec![&[]]);
    }

    let mut chunks = Vec::new();
    let mut start = 0usize;
    let mut current_len = 2usize;

    for (index, entry) in entries.iter().enumerate() {
        let item_len = serde_json::to_string(&entry.entry)
            .map_err(|error| {
                AlienError::new(ErrorData::JsonSerializationFailed {
                    reason: format!(
                        "failed to estimate CloudFormation Outputs resource chunk size: {error}"
                    ),
                })
            })?
            .len();
        let is_first_in_chunk = index == start;
        let separator_len = usize::from(!is_first_in_chunk);
        if !is_first_in_chunk
            && current_len + separator_len + item_len > OUTPUT_RESOURCES_CHUNK_BYTES
        {
            chunks.push(&entries[start..index]);
            start = index;
            current_len = 2 + item_len;
            continue;
        }

        current_len += separator_len + item_len;
    }

    chunks.push(&entries[start..]);

    Ok(chunks)
}

fn stack_settings_expression(
    target: CloudFormationTarget,
    stack: &Stack,
    settings: &StackSettings,
    kubernetes_namespace: Option<CfExpression>,
    supports_custom_domain: bool,
    bindings_only: bool,
) -> CfExpression {
    if bindings_only {
        return CfExpression::object([
            ("deploymentModel", CfExpression::from("push")),
            ("updates", CfExpression::from("approval-required")),
            ("telemetry", CfExpression::from("off")),
            ("heartbeats", CfExpression::from("on")),
        ]);
    }
    let mut values = vec![
        ("deploymentModel", CfExpression::from("push")),
        ("endpointAccess", CfExpression::ref_(PARAM_ENDPOINT_ACCESS)),
        ("updates", CfExpression::ref_(PARAM_UPDATES_MODE)),
        ("telemetry", CfExpression::ref_(PARAM_TELEMETRY_MODE)),
        ("heartbeats", CfExpression::ref_(PARAM_HEARTBEATS_MODE)),
        (
            "network",
            network_expression(stack, settings.network.as_ref(), target),
        ),
    ];
    if supports_custom_domain {
        values.push(("domains", domains_expression(stack, target)));
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

fn network_expression(
    stack: &Stack,
    network: Option<&NetworkSettings>,
    target: CloudFormationTarget,
) -> CfExpression {
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
            {
                let byo = CfExpression::object([
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
                ]);
                if restricts_network_mode(stack, target) {
                    byo
                } else {
                    CfExpression::if_(
                        CONDITION_NETWORK_MODE_USE_EXISTING,
                        byo,
                        CfExpression::object([("type", CfExpression::from("use-default"))]),
                    )
                }
            },
        ),
        Some(NetworkSettings::ByoVpcGcp { .. } | NetworkSettings::ByoVnetAzure { .. }) => {
            CfExpression::no_value()
        }
    }
}

fn domains_expression(stack: &Stack, target: CloudFormationTarget) -> CfExpression {
    let domain = CfExpression::object([
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
    ]);
    let custom_domains = if target.is_kubernetes() {
        CfExpression::object([("default", domain)])
    } else {
        let ids = public_http_resource_ids(stack);
        let mut selected =
            CfExpression::object([(ids.last().expect("public resource").clone(), domain.clone())]);
        for (index, id) in ids.iter().enumerate().rev().skip(1) {
            selected = CfExpression::if_(
                format!("CustomDomainResource{index}"),
                CfExpression::object([(id.clone(), domain.clone())]),
                selected,
            );
        }
        selected
    };
    CfExpression::if_(
        CONDITION_HAS_DOMAIN_NAME,
        CfExpression::object([("customDomains", custom_domains)]),
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
    resource_id: Option<String>,
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
        let Some((resource_id, domain)) = custom_domains.iter().min_by_key(|(id, _)| *id) else {
            return Self::empty();
        };

        Self {
            resource_id: Some(resource_id.clone()),
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
            resource_id: None,
            domain_name: None,
            certificate_arn: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{PermissionProfile, Resource, ResourceRef};

    fn node_dependency_fixture() -> (Stack, IndexMap<String, Vec<String>>, CfTemplate) {
        let mut stack = Stack::new("example".to_string())
            .add(
                ComputeCluster::new("compute".to_string())
                    .node_permissions(
                        PermissionProfile::new().resource("objects", ["storage/data-read"]),
                    )
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Storage::new("objects".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Storage::new("ordinary".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        stack.resources.get_mut("compute").unwrap().dependencies = vec![
            ResourceRef::new(Storage::RESOURCE_TYPE, "objects"),
            ResourceRef::new(Storage::RESOURCE_TYPE, "ordinary"),
        ];
        let ids = indexmap! {
            "compute".to_string() => vec!["NodeRole".to_string(), "NodePolicy".to_string()],
            "objects".to_string() => vec!["Bucket".to_string()],
            "ordinary".to_string() => vec!["OtherBucket".to_string()],
        };
        let mut template = CfTemplate::default();
        for (id, kind) in [
            ("NodeRole", "AWS::IAM::Role"),
            ("NodePolicy", "AWS::IAM::Policy"),
            ("Bucket", "AWS::S3::Bucket"),
            ("OtherBucket", "AWS::S3::Bucket"),
        ] {
            template.resources.insert(
                id.to_string(),
                CfResource::new(id.to_string(), kind.to_string()),
            );
        }
        let policy = template.resources.get_mut("NodePolicy").unwrap();
        policy.properties.insert(
            "Roles".to_string(),
            CfExpression::list([CfExpression::ref_("NodeRole")]),
        );
        policy.properties.insert(
            "PolicyDocument".to_string(),
            CfExpression::object([
                ("Version", CfExpression::from("2012-10-17")),
                (
                    "Statement",
                    CfExpression::list([CfExpression::object([
                        ("Effect", CfExpression::from("Allow")),
                        ("Action", CfExpression::from("s3:GetObject")),
                        ("Resource", CfExpression::get_att("Bucket", "Arn")),
                    ])]),
                ),
            ]),
        );
        (stack, ids, template)
    }

    #[test]
    fn node_storage_order_is_projected_without_changing_policy_or_executor_edges() {
        for location in ["managed", "gated", "external"] {
            let (stack, mut ids, mut template) = node_dependency_fixture();
            let target = if location == "gated" {
                template.resources.get_mut("Bucket").unwrap().condition =
                    Some("Enabled".to_string());
                template.resources.get_mut("NodePolicy").unwrap().condition =
                    Some("Enabled".to_string());
                CfExpression::if_(
                    "Enabled",
                    CfExpression::get_att("Bucket", "Arn"),
                    CfExpression::no_value(),
                )
            } else if location == "external" {
                template.resources.shift_remove("Bucket");
                ids.shift_remove("objects");
                CfExpression::from("arn:aws:s3:::existing-objects/*")
            } else {
                CfExpression::get_att("Bucket", "Arn")
            };
            let CfExpression::Object(document) = template
                .resources
                .get_mut("NodePolicy")
                .unwrap()
                .properties
                .get_mut("PolicyDocument")
                .unwrap()
            else {
                panic!("policy document")
            };
            let CfExpression::List(statements) = document.get_mut("Statement").unwrap() else {
                panic!("statements")
            };
            let CfExpression::Object(statement) = &mut statements[0] else {
                panic!("statement")
            };
            statement.insert("Resource".to_string(), target);
            let policy = template.resources["NodePolicy"].clone();
            let dependencies = stack.resources["compute"].dependencies.clone();
            apply_resource_dependencies(&stack, &ids, &mut template);
            for id in ["NodeRole", "NodePolicy"] {
                assert_eq!(
                    template.resources[id].depends_on,
                    vec!["OtherBucket".to_string()]
                );
            }
            assert_eq!(
                template.resources["NodePolicy"].properties,
                policy.properties
            );
            assert_eq!(template.resources["NodePolicy"].condition, policy.condition);
            assert_eq!(stack.resources["compute"].dependencies, dependencies);
            let once = template.clone();
            apply_resource_dependencies(&stack, &ids, &mut template);
            assert_eq!(template, once);
        }
    }

    #[test]
    fn ordinary_or_invalid_node_targets_keep_physical_dependencies() {
        for case in [
            "no-profile",
            "empty",
            "other-target",
            "wrong-ref-type",
            "wrong-target-type",
            "missing",
            "mismatched-id",
            "non-node",
        ] {
            let (mut stack, ids, mut template) = node_dependency_fixture();
            match case {
                "no-profile" => {
                    stack
                        .resources
                        .get_mut("compute")
                        .unwrap()
                        .config
                        .downcast_mut::<ComputeCluster>()
                        .unwrap()
                        .node_permissions = None
                }
                "empty" => stack
                    .resources
                    .get_mut("compute")
                    .unwrap()
                    .config
                    .downcast_mut::<ComputeCluster>()
                    .unwrap()
                    .node_permissions
                    .as_mut()
                    .unwrap()
                    .0
                    .get_mut("objects")
                    .unwrap()
                    .clear(),
                "other-target" => {
                    stack
                        .resources
                        .get_mut("compute")
                        .unwrap()
                        .config
                        .downcast_mut::<ComputeCluster>()
                        .unwrap()
                        .node_permissions =
                        Some(PermissionProfile::new().resource("elsewhere", ["storage/data-read"]))
                }
                "wrong-ref-type" => {
                    stack.resources.get_mut("compute").unwrap().dependencies[0] =
                        ResourceRef::new(ComputeCluster::RESOURCE_TYPE, "objects")
                }
                "wrong-target-type" => {
                    stack.resources.get_mut("objects").unwrap().config =
                        Resource::new(ComputeCluster::new("objects".to_string()).build())
                }
                "missing" => {
                    stack.resources.shift_remove("objects");
                }
                "mismatched-id" => {
                    stack.resources.get_mut("objects").unwrap().config =
                        Resource::new(Storage::new("different".to_string()).build())
                }
                "non-node" => {
                    stack.resources.get_mut("compute").unwrap().config =
                        Resource::new(Storage::new("compute".to_string()).build())
                }
                _ => unreachable!(),
            }
            apply_resource_dependencies(&stack, &ids, &mut template);
            for id in ["NodeRole", "NodePolicy"] {
                assert_eq!(
                    template.resources[id].depends_on,
                    vec!["Bucket".to_string(), "OtherBucket".to_string()],
                    "{case}"
                );
            }
        }
    }

    #[test]
    fn quotes_yaml_1_1_boolean_like_strings_in_object_values() {
        let yaml = "Properties:\n  StackSettings:\n    telemetry: off\n    heartbeats: on\n  AllowedValues:\n  - off\n  - on\n  Label: only-on-request\n";

        assert_eq!(
            quote_yaml_1_1_mode_scalars(yaml),
            "Properties:\n  StackSettings:\n    telemetry: \"off\"\n    heartbeats: \"on\"\n  AllowedValues:\n  - \"off\"\n  - \"on\"\n  Label: only-on-request\n"
        );
    }

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
