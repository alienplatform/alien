//! The flow log on a created VPC must be the same under CloudFormation and Terraform, and the
//! same as the one direct setup's controller creates: all three build it from
//! [`AWS_VPC_FLOW_LOG`].

use super::helpers::{render, role_policy_documents};
use alien_cloudformation::{
    generate_cloudformation_template, CfRegistry, CloudFormationOptions, CloudFormationTarget,
    RegistrationMode,
};
use alien_core::{
    Network, NetworkSettings, ResourceLifecycle, Stack, StackSettings, AWS_VPC_FLOW_LOG,
};
use alien_terraform::{ModuleFiles, TerraformTarget};
use serde_json::Value;

/// The settings a rendered flow log, its log group and its delivery role carry.
#[derive(Debug, PartialEq)]
struct RenderedFlowLog {
    traffic_type: String,
    log_destination_type: String,
    max_aggregation_interval: u64,
    retention_in_days: u64,
    delivery_service: String,
    delivery_actions: Vec<String>,
}

fn expected() -> RenderedFlowLog {
    RenderedFlowLog {
        traffic_type: AWS_VPC_FLOW_LOG.traffic_type.to_string(),
        log_destination_type: AWS_VPC_FLOW_LOG.log_destination_type.to_string(),
        max_aggregation_interval: AWS_VPC_FLOW_LOG.max_aggregation_interval.into(),
        retention_in_days: AWS_VPC_FLOW_LOG.retention_in_days as u64,
        delivery_service: AWS_VPC_FLOW_LOG.delivery_service.to_string(),
        delivery_actions: AWS_VPC_FLOW_LOG
            .delivery_actions
            .iter()
            .map(|action| action.to_string())
            .collect(),
    }
}

fn created_network() -> (Stack, StackSettings) {
    let settings = StackSettings {
        network: Some(NetworkSettings::Create {
            cidr: Some("10.42.0.0/16".to_string()),
            availability_zones: 2,
        }),
        ..StackSettings::default()
    };
    let stack = Stack::new("flow-logs".to_string())
        .add(
            Network::new("default-network".to_string())
                .settings(settings.network.clone().expect("network"))
                .build(),
            ResourceLifecycle::Frozen,
        )
        .build();
    (stack, settings)
}

fn strings(value: &Value) -> Vec<String> {
    match value {
        Value::Array(items) => items.iter().flat_map(strings).collect(),
        Value::String(text) => vec![text.clone()],
        other => panic!("expected strings, got {other}"),
    }
}

fn number(value: &Value) -> u64 {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        .unwrap_or_else(|| panic!("expected a number, got {value}"))
}

fn cloudformation_flow_log(stack: &Stack, settings: StackSettings) -> RenderedFlowLog {
    let template = generate_cloudformation_template(
        stack,
        CloudFormationOptions {
            pins: alien_core::SetupPins::compiled_in(),
            registry: &CfRegistry::built_in(),
            source_stack: stack,
            target: CloudFormationTarget::Aws,
            stack_settings: settings,
            setup_target: "aws".to_string(),
            setup_fingerprint: "test".to_string(),
            setup_fingerprint_version: 1,
            registration: RegistrationMode::OutputsFallback,
            description: None,
            management_trust: Default::default(),
        },
    )
    .expect("template renders");
    let template = serde_json::to_value(&template).expect("template serializes");
    let only = |resource_type: &str| {
        let found: Vec<&Value> = template["Resources"]
            .as_object()
            .expect("resources")
            .values()
            .filter(|resource| resource["Type"] == resource_type)
            .collect();
        assert_eq!(found.len(), 1, "one {resource_type} in the template");
        found[0]["Properties"].clone()
    };
    let flow_log = only("AWS::EC2::FlowLog");
    let log_group = only("AWS::Logs::LogGroup");
    let role = template["Resources"]
        .as_object()
        .expect("resources")
        .values()
        .find(|resource| {
            resource["Type"] == "AWS::IAM::Role"
                && resource["Properties"]["AssumeRolePolicyDocument"]["Statement"][0]["Principal"]
                    ["Service"]
                    == AWS_VPC_FLOW_LOG.delivery_service
        })
        .expect("the template declares the delivery role")["Properties"]
        .clone();
    RenderedFlowLog {
        traffic_type: strings(&flow_log["TrafficType"]).concat(),
        log_destination_type: strings(&flow_log["LogDestinationType"]).concat(),
        max_aggregation_interval: number(&flow_log["MaxAggregationInterval"]),
        retention_in_days: number(&log_group["RetentionInDays"]),
        delivery_service: strings(
            &role["AssumeRolePolicyDocument"]["Statement"][0]["Principal"]["Service"],
        )
        .concat(),
        delivery_actions: strings(&role["Policies"][0]["PolicyDocument"]["Statement"][0]["Action"]),
    }
}

/// An HCL value as JSON; anything that is not a literal becomes its HCL text.
fn json(expression: &hcl::Expression) -> Value {
    match expression {
        hcl::Expression::String(text) => Value::String(text.clone()),
        hcl::Expression::Number(number) => Value::String(number.to_string()),
        hcl::Expression::Array(items) => Value::Array(items.iter().map(json).collect()),
        hcl::Expression::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| (key.to_string().trim_matches('"').to_string(), json(value)))
                .collect(),
        ),
        other => Value::String(other.to_string()),
    }
}

fn terraform_flow_log(module: &ModuleFiles) -> RenderedFlowLog {
    let blocks: Vec<hcl::Block> = module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .flat_map(|(path, contents)| {
            hcl::parse(contents)
                .unwrap_or_else(|error| panic!("{path} parses: {error}"))
                .into_blocks()
                .filter(|block| block.identifier() == "resource")
        })
        .collect();
    let only = |kind: &str| {
        let found: Vec<&hcl::Block> = blocks
            .iter()
            .filter(|block| block.labels()[0].as_str() == kind)
            .collect();
        assert_eq!(found.len(), 1, "one {kind} in the module");
        found[0]
    };
    let attribute = |block: &hcl::Block, key: &str| -> Value {
        let expression = block
            .body()
            .attributes()
            .find(|attribute| attribute.key() == key)
            .unwrap_or_else(|| panic!("{} has {key}", block.labels()[0].as_str()))
            .expr()
            .clone();
        // A policy is `jsonencode(<document>)`: compare the document.
        let expression = match expression {
            hcl::Expression::FuncCall(call) if call.name.name.as_str() == "jsonencode" => {
                call.args[0].clone()
            }
            other => other,
        };
        json(&expression)
    };
    let flow_log = only("aws_flow_log");
    let log_group = only("aws_cloudwatch_log_group");
    let trust = attribute(only("aws_iam_role"), "assume_role_policy");
    // Terraform moves a role's inline policy into an attached managed policy.
    let delivery = role_policy_documents(
        module,
        &format!("{}_flow_logs", flow_log.labels()[1].as_str()),
    );
    assert_eq!(delivery.len(), 1, "one delivery policy: {delivery:?}");
    let delivery = json(&delivery[0].1);
    RenderedFlowLog {
        traffic_type: strings(&attribute(flow_log, "traffic_type")).concat(),
        log_destination_type: strings(&attribute(flow_log, "log_destination_type")).concat(),
        max_aggregation_interval: number(&attribute(flow_log, "max_aggregation_interval")),
        retention_in_days: number(&attribute(log_group, "retention_in_days")),
        delivery_service: strings(&trust["Statement"][0]["Principal"]["Service"]).concat(),
        delivery_actions: strings(&delivery["Statement"][0]["Action"]),
    }
}

#[test]
fn cloudformation_and_terraform_render_the_shared_flow_log() {
    let (stack, settings) = created_network();
    let cloudformation = cloudformation_flow_log(&stack, settings.clone());
    let module = render(&stack, TerraformTarget::Aws, settings);
    let terraform = terraform_flow_log(&module);

    assert_eq!(cloudformation, expected(), "CloudFormation");
    assert_eq!(terraform, expected(), "Terraform");
}
