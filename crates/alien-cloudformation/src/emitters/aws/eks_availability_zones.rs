//! Setup-owned account-local AZ selection. No runtime cloud discovery is required.
use crate::{
    emitters::aws::helpers::{service_trust_policy, tags},
    template::{CfExpression, CfResource},
};
use alien_core::{
    import::EmitContext, KubernetesCluster, KubernetesClusterOwnership, KubernetesClusterProvider,
};

pub(crate) const LOOKUP_ID: &str = "EksAvailabilityZones";
const HANDLER: &str = include_str!("eks_availability_zones.py");

/// Custom-resource attributes are strings; split the CSV response before selection.
pub(crate) fn zones(attribute: &str) -> CfExpression {
    CfExpression::object([(
        "Fn::Split",
        CfExpression::list([
            CfExpression::from(","),
            CfExpression::get_att(LOOKUP_ID, attribute),
        ]),
    )])
}

pub(crate) fn required(ctx: &EmitContext<'_>) -> bool {
    ctx.targets_kubernetes
        && ctx.stack.resources().any(|(_, entry)| {
            entry
                .config
                .downcast_ref::<KubernetesCluster>()
                .is_some_and(|cluster| {
                    cluster.provider == KubernetesClusterProvider::Eks
                        && cluster.ownership == KubernetesClusterOwnership::Managed
                })
        })
}

pub(crate) fn resources(ctx: &EmitContext<'_>, count: CfExpression) -> Vec<CfResource> {
    let mut logs = CfResource::new(
        "EksAvailabilityZonesLogs".to_string(),
        "AWS::Logs::LogGroup".to_string(),
    );
    logs.properties.insert(
        "LogGroupName".to_string(),
        CfExpression::sub("/aws/lambda/${AWS::StackName}-eks-az"),
    );
    logs.properties
        .insert("RetentionInDays".to_string(), CfExpression::from(14u8));
    let mut role = CfResource::new(
        "EksAvailabilityZonesRole".to_string(),
        "AWS::IAM::Role".to_string(),
    );
    role.properties.insert(
        "AssumeRolePolicyDocument".to_string(),
        service_trust_policy(["lambda.amazonaws.com"]),
    );
    role.properties.insert(
        "Policies".to_string(),
        CfExpression::list([CfExpression::object([
            ("PolicyName", CfExpression::from("availability-zone-lookup")),
            (
                "PolicyDocument",
                CfExpression::object([
                    ("Version", CfExpression::from("2012-10-17")),
                    (
                        "Statement",
                        CfExpression::list([
                            CfExpression::object([
                                ("Effect", CfExpression::from("Allow")),
                                (
                                    "Action",
                                    CfExpression::list([CfExpression::from(
                                        "ec2:DescribeAvailabilityZones",
                                    )]),
                                ),
                                ("Resource", CfExpression::from("*")),
                            ]),
                            CfExpression::object([
                                ("Effect", CfExpression::from("Allow")),
                                (
                                    "Action",
                                    CfExpression::list([
                                        CfExpression::from("logs:CreateLogStream"),
                                        CfExpression::from("logs:PutLogEvents"),
                                    ]),
                                ),
                                (
                                    "Resource",
                                    CfExpression::get_att("EksAvailabilityZonesLogs", "Arn"),
                                ),
                            ]),
                        ]),
                    ),
                ]),
            ),
        ])]),
    );
    let mut function = CfResource::new(
        "EksAvailabilityZonesFunction".to_string(),
        "AWS::Lambda::Function".to_string(),
    );
    function.properties.insert(
        "FunctionName".to_string(),
        CfExpression::sub("${AWS::StackName}-eks-az"),
    );
    function.properties.insert(
        "Role".to_string(),
        CfExpression::get_att("EksAvailabilityZonesRole", "Arn"),
    );
    function
        .properties
        .insert("Runtime".to_string(), CfExpression::from("python3.13"));
    function
        .properties
        .insert("Handler".to_string(), CfExpression::from("index.handler"));
    function
        .properties
        .insert("Timeout".to_string(), CfExpression::from(60u8));
    function.properties.insert(
        "Code".to_string(),
        CfExpression::object([("ZipFile", CfExpression::from(HANDLER))]),
    );
    let mut lookup = CfResource::new(
        LOOKUP_ID.to_string(),
        "Custom::EksAvailabilityZones".to_string(),
    );
    lookup.properties.insert(
        "ServiceToken".to_string(),
        CfExpression::get_att("EksAvailabilityZonesFunction", "Arn"),
    );
    lookup
        .properties
        .insert("RequestedCount".to_string(), count);
    lookup.properties.insert(
        "ExcludedZoneIds".to_string(),
        CfExpression::list(["use1-az3", "usw1-az2", "cac1-az3"].map(CfExpression::from)),
    );
    let mut resources = vec![logs, role, function, lookup];
    for resource in &mut resources {
        resource.condition = Some("NetworkModeCreate".to_string());
        if resource.resource_type != "Custom::EksAvailabilityZones" {
            resource.properties.insert("Tags".to_string(), tags(ctx));
        }
    }
    resources
}

#[cfg(test)]
mod tests {
    #[test]
    fn actual_lookup_handler_responds_through_create_update_delete_and_failures() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let output = std::process::Command::new("python3")
            .arg(root.join("tests/handlers/eks_availability_zones_test.py"))
            .arg(root.join("src/emitters/aws/eks_availability_zones.py"))
            .output()
            .expect("Python is required for the embedded setup handler test");
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
