//! CloudFormation expression builders and parameter/output constructors.
//!
//! Owns the deployment-settings expressions (kubernetes / network / domains /
//! management), the JSON <-> `CfExpression` bridge used to overlay serialized
//! settings, and the small `CfParameter` / `CfOutput` constructors shared by
//! the parameter and output builders.

use super::{
    empty_object, CloudFormationTarget, CONDITION_HAS_DOMAIN_NAME, CONDITION_HAS_VPC_CIDR,
    CONDITION_NETWORK_MODE_CREATE, CONDITION_NETWORK_MODE_USE_EXISTING, PARAM_AVAILABILITY_ZONES,
    PARAM_CERTIFICATE_ARN, PARAM_DOMAIN_NAME, PARAM_MANAGING_ROLE_ARN, PARAM_PRIVATE_SUBNET_IDS,
    PARAM_PUBLIC_SUBNET_IDS, PARAM_SECURITY_GROUP_IDS, PARAM_VPC_CIDR, PARAM_VPC_ID,
};
use crate::template::{CfExpression, CfOutput, CfParameter};
use alien_core::{HeartbeatsMode, KubernetesSettings, NetworkSettings, TelemetryMode, UpdatesMode};
use serde_json::Value;

pub(super) fn kubernetes_settings_expression(
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

pub(super) fn merge_cf_expression(base: &mut CfExpression, overlay: CfExpression) {
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

pub(super) fn is_cloudformation_intrinsic(expression: &CfExpression) -> bool {
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

pub(super) fn network_expression(network: Option<&NetworkSettings>) -> CfExpression {
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

pub(super) fn domains_expression() -> CfExpression {
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

pub(super) fn management_config_expression(target: CloudFormationTarget) -> CfExpression {
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

pub(super) fn string_parameter(
    description: &str,
    default: Option<String>,
    allowed_values: Option<Vec<CfExpression>>,
    no_echo: bool,
) -> CfParameter {
    string_parameter_with_allowed_pattern(description, default, allowed_values, None, no_echo)
}

pub(super) fn string_parameter_with_allowed_pattern(
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

pub(super) fn number_parameter(
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

pub(super) fn comma_list_parameter(description: &str, default: Vec<String>) -> CfParameter {
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

pub(super) fn equals_ref(parameter: &str, value: &str) -> CfExpression {
    CfExpression::equals(CfExpression::ref_(parameter), CfExpression::from(value))
}

pub(super) fn condition_ref(condition: &str) -> CfExpression {
    CfExpression::object([("Condition", CfExpression::from(condition))])
}

pub(super) fn output(description: &str, value: CfExpression) -> CfOutput {
    CfOutput {
        description: Some(description.to_string()),
        value,
        export: None,
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

pub(super) fn network_mode_default(network: Option<&NetworkSettings>) -> &'static str {
    match network {
        Some(NetworkSettings::ByoVpcAws { .. }) => "use-existing",
        Some(NetworkSettings::UseDefault) => "use-default",
        None | Some(NetworkSettings::Create { .. }) => "create-new",
        Some(NetworkSettings::ByoVpcGcp { .. } | NetworkSettings::ByoVnetAzure { .. }) => {
            "create-new"
        }
    }
}
