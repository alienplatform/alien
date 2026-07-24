use super::*;

pub(super) fn add_console_interface_metadata(
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

pub(super) fn compute_capacity_groups(stack: &Stack) -> Vec<&CapacityGroup> {
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

pub(super) fn compute_settings_expression(
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

pub(super) fn compute_machine_parameter_name(pool_id: &str) -> String {
    format!("Compute{}Machine", pascal_identifier(pool_id))
}

pub(super) fn compute_fixed_machines_parameter_name(pool_id: &str) -> String {
    format!("Compute{}Machines", pascal_identifier(pool_id))
}

pub(super) fn compute_autoscale_min_parameter_name(pool_id: &str) -> String {
    format!("Compute{}Min", pascal_identifier(pool_id))
}

pub(super) fn compute_autoscale_max_parameter_name(pool_id: &str) -> String {
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
