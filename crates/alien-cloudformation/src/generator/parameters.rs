//! CloudFormation parameter, rule, and condition builders.
//!
//! Owns the standard/network/compute parameter blocks, the supported-region
//! and custom-domain rules, the standard conditions, and the parameter-default
//! helpers that read those values from stack settings.

use super::expressions::{
    comma_list_parameter, condition_ref, equals_ref, heartbeats_mode, network_mode_default,
    number_parameter, string_parameter, string_parameter_with_allowed_pattern, telemetry_mode,
    updates_mode,
};
use super::{
    compute_autoscale_max_parameter_name, compute_autoscale_min_parameter_name,
    compute_capacity_groups, compute_fixed_machines_parameter_name, compute_machine_parameter_name,
    RegistrationMode, CONDITION_HAS_DOMAIN_NAME, CONDITION_HAS_VPC_CIDR, CONDITION_NETWORK_AZ2,
    CONDITION_NETWORK_AZ3, CONDITION_NETWORK_CREATE_AZ2, CONDITION_NETWORK_CREATE_AZ3,
    CONDITION_NETWORK_MODE_CREATE, CONDITION_NETWORK_MODE_USE_EXISTING, PARAM_AVAILABILITY_ZONES,
    PARAM_CERTIFICATE_ARN, PARAM_DOMAIN_NAME, PARAM_HEARTBEATS_MODE, PARAM_HOSTED_ZONE_ID,
    PARAM_MANAGING_ACCOUNT_ID, PARAM_MANAGING_ROLE_ARN, PARAM_NETWORK_MODE,
    PARAM_PRIVATE_SUBNET_IDS, PARAM_PUBLIC_SUBNET_IDS, PARAM_SECURITY_GROUP_IDS,
    PARAM_TELEMETRY_MODE, PARAM_TOKEN, PARAM_UPDATES_MODE, PARAM_VPC_CIDR, PARAM_VPC_ID,
    RULE_CUSTOM_DOMAIN_CERTIFICATE, RULE_SUPPORTED_AWS_REGION,
};
use crate::template::{CfExpression, CfRule, CfRuleAssertion, CfTemplate};
use alien_core::{
    CapacityGroupScalePolicy, ComputePoolSelection, DomainSettings, Network, NetworkSettings,
    Platform, Result, Stack, StackSettings,
};

pub(super) fn add_standard_parameters(
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

pub(super) fn add_supported_region_rule(
    template: &mut CfTemplate,
    registration: &RegistrationMode,
) {
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

pub(super) fn add_custom_domain_certificate_rule(template: &mut CfTemplate) {
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

pub(super) fn add_standard_conditions(
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
