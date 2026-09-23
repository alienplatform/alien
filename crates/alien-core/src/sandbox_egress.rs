//! What an AWS sandbox with `egress: deny` needs in the customer account, as concrete documents:
//! the operator role Lambda assumes to place the connector's network interfaces, and the
//! `AWS::Lambda::NetworkConnector` those interfaces belong to.
//!
//! The CloudFormation and Terraform emitters write the same objects as template expressions; the
//! generator parity tests fail if the resolved forms here ever disagree with them.

use std::fmt;

use serde_json::{json, Value};

use crate::{
    setup_resource_tags, Network, NetworkSettings, ResourceEntry, Sandbox, SandboxEgress, Stack,
};

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

/// Where a VPC with private subnets comes from, which is all that separates how the emitters
/// render the connector's subnets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxEgressVpc {
    /// The network is `create`: setup builds the VPC and its private subnets.
    Created,
    /// The network is `byo-vpc-aws`: the customer names the VPC and its private subnets.
    BroughtByCustomer,
}

/// The network a deny sandbox's connector attaches to.
#[derive(Debug, Clone, Copy)]
pub struct SandboxEgressNetwork<'a> {
    /// The network's resource id in the stack.
    pub id: &'a str,
    /// The network's stack entry, which carries its lifecycle.
    pub entry: &'a ResourceEntry,
    pub vpc: SandboxEgressVpc,
}

/// Why an AWS sandbox's egress cannot be built from this stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxEgressRefusal {
    /// AWS has no domain filter at the connector, so `allowDomains` has nothing to render into.
    DomainAllowList,
    /// The stack declares no network for the connector to attach to.
    NoNetwork,
    /// The network is the account's default VPC, which has no private subnets.
    DefaultVpc,
    /// The network's settings are for GCP or Azure.
    OtherCloudNetwork,
}

impl fmt::Display for SandboxEgressRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an AWS sandbox routes session traffic through a VPC egress connector")?;
        f.write_str(match self {
            Self::DomainAllowList => {
                ", which has no configuration for egress 'allowDomains'. Declare egress: deny for \
                 a connector that reaches nothing, or egress: allow for no connector at all"
            }
            Self::NoNetwork => ", and this stack declares no network for it to attach to",
            Self::DefaultVpc => {
                ", which needs private subnets, and the account's default VPC has only public \
                 subnets. Set the network to create or byo-vpc-aws"
            }
            Self::OtherCloudNetwork => ", and this stack's network settings are for another cloud",
        })
    }
}

/// The network an AWS sandbox's egress connector attaches to: none for `allow`, the stack's
/// first network for `deny`.
///
/// A connector names one to sixteen private subnets, and only a created or bring-your-own VPC has
/// any. Every other case is refused rather than rendered: a session started with no connector
/// reaches the internet, and a dropped `allowDomains` leaves the customer believing it applies.
pub fn sandbox_egress_network<'a>(
    stack: &'a Stack,
    egress: &SandboxEgress,
) -> Result<Option<SandboxEgressNetwork<'a>>, SandboxEgressRefusal> {
    match egress {
        SandboxEgress::Allow => return Ok(None),
        SandboxEgress::AllowDomains { .. } => return Err(SandboxEgressRefusal::DomainAllowList),
        SandboxEgress::Deny => {}
    }
    let (id, entry, network) = stack
        .resources()
        .find_map(|(id, entry)| Some((id, entry, entry.config.downcast_ref::<Network>()?)))
        .ok_or(SandboxEgressRefusal::NoNetwork)?;
    let vpc = match &network.settings {
        NetworkSettings::Create { .. } => SandboxEgressVpc::Created,
        NetworkSettings::ByoVpcAws { .. } => SandboxEgressVpc::BroughtByCustomer,
        NetworkSettings::UseDefault => return Err(SandboxEgressRefusal::DefaultVpc),
        NetworkSettings::ByoVpcGcp { .. } | NetworkSettings::ByoVnetAzure { .. } => {
            return Err(SandboxEgressRefusal::OtherCloudNetwork)
        }
    };
    Ok(Some(SandboxEgressNetwork { id, entry, vpc }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ResourceLifecycle, SandboxCode, SandboxLifecyclePolicy};
    use SandboxEgressRefusal::{DefaultVpc, DomainAllowList, NoNetwork, OtherCloudNetwork};
    use SandboxEgressVpc::{BroughtByCustomer, Created};

    fn outcome(
        egress: SandboxEgress,
        network: Option<NetworkSettings>,
    ) -> Result<Option<SandboxEgressVpc>, SandboxEgressRefusal> {
        let sandbox = Sandbox::new("agents".to_string())
            .code(SandboxCode::Image {
                image: "s3://acme/agents/bundle.zip".to_string(),
            })
            .egress(egress.clone())
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();
        let mut stack = Stack::new("acme".to_string()).add(sandbox, ResourceLifecycle::Frozen);
        if let Some(settings) = network {
            stack = stack.add(
                Network::new("net".to_string()).settings(settings).build(),
                ResourceLifecycle::Frozen,
            );
        }
        let stack = stack.build();
        let found = sandbox_egress_network(&stack, &egress)?;
        if let Some(found) = &found {
            assert_eq!(found.id, "net");
        }
        Ok(found.map(|found| found.vpc))
    }

    #[test]
    fn every_egress_mode_and_network_setting_resolves_to_one_outcome() {
        let create = || NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        };
        let byo_aws = || NetworkSettings::ByoVpcAws {
            vpc_id: "vpc-1".to_string(),
            public_subnet_ids: vec!["subnet-pub".to_string()],
            private_subnet_ids: vec!["subnet-priv".to_string()],
            security_group_ids: vec![],
        };
        let byo_gcp = || NetworkSettings::ByoVpcGcp {
            network_name: "vpc".to_string(),
            subnet_name: "subnet".to_string(),
            region: "us-central1".to_string(),
        };
        let domains = || SandboxEgress::AllowDomains {
            domains: vec!["example.com".to_string()],
        };

        let cases = [
            (SandboxEgress::Allow, None, Ok(None)),
            (
                SandboxEgress::Allow,
                Some(NetworkSettings::UseDefault),
                Ok(None),
            ),
            (SandboxEgress::Allow, Some(byo_gcp()), Ok(None)),
            (domains(), None, Err(DomainAllowList)),
            (domains(), Some(create()), Err(DomainAllowList)),
            (SandboxEgress::Deny, None, Err(NoNetwork)),
            (SandboxEgress::Deny, Some(create()), Ok(Some(Created))),
            (
                SandboxEgress::Deny,
                Some(byo_aws()),
                Ok(Some(BroughtByCustomer)),
            ),
            (
                SandboxEgress::Deny,
                Some(NetworkSettings::UseDefault),
                Err(DefaultVpc),
            ),
            (SandboxEgress::Deny, Some(byo_gcp()), Err(OtherCloudNetwork)),
        ];
        for (egress, network, expected) in cases {
            let label = format!("{egress:?} on {network:?}");
            assert_eq!(outcome(egress, network), expected, "{label}");
        }
    }
}
