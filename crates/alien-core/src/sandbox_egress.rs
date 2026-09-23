//! What an AWS sandbox with `egress: deny` needs in the customer account, as concrete documents:
//! the operator role Lambda assumes to place the connector's network interfaces, and the
//! `AWS::Lambda::NetworkConnector` those interfaces belong to.
//!
//! The CloudFormation and Terraform emitters write the same objects as template expressions; the
//! generator parity tests fail if the resolved forms here ever disagree with them.

use serde_json::{json, Value};

use crate::{setup_resource_tags, Sandbox};

/// The name of the operator role's inline policy.
pub const SANDBOX_EGRESS_POLICY_NAME: &str = "sandbox-egress-connector";

/// The registry type Cloud Control creates the connector as.
pub const NETWORK_CONNECTOR_TYPE_NAME: &str = "AWS::Lambda::NetworkConnector";

/// The one destination the deny security group permits, which reaches nothing.
pub const LOOPBACK_ONLY_CIDR: &str = "127.0.0.1/32";

/// The name of both the operator role and the deny security group. Each lives in its own
/// namespace, and a retry finds either again by this name.
pub fn sandbox_egress_name(resource_prefix: &str, sandbox_id: &str) -> String {
    format!("{resource_prefix}-{sandbox_id}-egress")
}

/// The connector's name, unique per account and Region, which is how a retry finds it again.
pub fn sandbox_egress_connector_name(resource_prefix: &str, sandbox_id: &str) -> String {
    format!("{resource_prefix}-{sandbox_id}")
}

/// Lambda may assume the operator role. No `aws:SourceAccount` condition: the emitters write none.
pub fn sandbox_egress_operator_trust_policy() -> Value {
    json!({
        "Version": "2012-10-17",
        "Statement": [{
            "Effect": "Allow",
            "Principal": { "Service": "lambda.amazonaws.com" },
            "Action": "sts:AssumeRole"
        }]
    })
}

/// The contents of AWS's `AWSLambdaNetworkConnectorOperatorPolicy`, written out so it does not
/// change under the customer when AWS revises the managed policy.
pub fn sandbox_egress_operator_policy(partition: &str, account_id: &str, region: &str) -> Value {
    let arn = |kind: &str| format!("arn:{partition}:ec2:{region}:{account_id}:{kind}/*");
    json!({
        "Version": "2012-10-17",
        "Statement": [
            {
                "Sid": "CreateENI",
                "Effect": "Allow",
                "Action": "ec2:CreateNetworkInterface",
                "Resource": [arn("network-interface"), arn("subnet"), arn("security-group")]
            },
            {
                "Sid": "TagENI",
                "Effect": "Allow",
                "Action": "ec2:CreateTags",
                "Resource": arn("network-interface"),
                "Condition": {
                    "StringEquals": {
                        "ec2:ManagedResourceOperator": "network-connectors.lambda.amazonaws.com"
                    }
                }
            }
        ]
    })
}

/// The connector a deny sandbox's sessions start with, as Cloud Control's `DesiredState`.
///
/// `NetworkProtocol` is optional in the schema and refused when absent. IPv4 because the deny
/// group matches IPv4 CIDRs only, so a v6 path would sit outside it.
#[derive(Debug, Clone, bon::Builder)]
pub struct SandboxEgressConnector<'a> {
    resource_prefix: &'a str,
    sandbox_id: &'a str,
    operator_role_arn: &'a str,
    private_subnet_ids: &'a [String],
    security_group_id: &'a str,
}

impl SandboxEgressConnector<'_> {
    pub fn desired_state(&self) -> Value {
        let tags: Vec<Value> = setup_resource_tags(
            self.resource_prefix,
            self.sandbox_id,
            Sandbox::RESOURCE_TYPE.as_ref(),
        )
        .into_iter()
        .map(|(key, value)| json!({ "Key": key, "Value": value }))
        .collect();
        json!({
            "Name": sandbox_egress_connector_name(self.resource_prefix, self.sandbox_id),
            "OperatorRole": self.operator_role_arn,
            "Configuration": {
                "VpcEgressConfiguration": {
                    "AssociatedComputeResourceTypes": ["MicroVm"],
                    "NetworkProtocol": "IPv4",
                    "SubnetIds": self.private_subnet_ids,
                    "SecurityGroupIds": [self.security_group_id]
                }
            },
            "Tags": tags
        })
    }
}
