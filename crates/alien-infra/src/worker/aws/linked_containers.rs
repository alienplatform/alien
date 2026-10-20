//! A linked container's empty security group is an identity attached only to its
//! consumers. The container owns the ingress grant and the group's lifecycle.
use crate::{
    core::ResourceControllerContext,
    error::{ErrorData, Result},
    network::AwsNetworkController,
};
use alien_aws_clients::{
    ec2::{DescribeSecurityGroupsRequest, Ec2Api, Filter},
    lambda::VpcConfig,
};
use alien_core::{
    Container, Network, ResourceRef, Worker, ALIEN_MANAGED_BY_TAG_KEY, ALIEN_RESOURCE_TAG_KEY,
    ALIEN_STACK_TAG_KEY,
};
use alien_error::{AlienError, Context};

fn invalid(worker: &str, message: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        resource_id: Some(worker.into()),
        message: message.into(),
    })
}

pub(super) async fn vpc_config(
    ctx: &ResourceControllerContext<'_>,
    base: Option<VpcConfig>,
) -> Result<Option<VpcConfig>> {
    let worker = ctx.desired_resource_config::<Worker>()?;
    let mut targets = Vec::new();
    for link in &worker.links {
        if link.resource_type != Container::RESOURCE_TYPE {
            continue;
        }
        let target = ctx
            .desired_stack
            .resources
            .get(link.id())
            .and_then(|entry| entry.config.downcast_ref::<Container>())
            .ok_or_else(|| {
                invalid(
                    &worker.id,
                    format!("Linked Container '{}' is missing", link.id()),
                )
            })?;
        if !target.ports.is_empty() {
            if !ctx
                .state
                .resources
                .get(link.id())
                .is_some_and(|state| state.status == alien_core::ResourceStatus::Running)
            {
                return Err(AlienError::new(ErrorData::DependencyNotReady {
                    resource_id: worker.id.clone(),
                    dependency_id: link.id().to_owned(),
                }));
            }
            targets.push(link.id().to_owned());
        }
    }
    targets.sort();
    targets.dedup();
    if targets.is_empty() {
        return Ok(base);
    }
    let mut base = base.ok_or_else(|| {
        invalid(
            &worker.id,
            "Container links require private Lambda subnets and a network security group",
        )
    })?;
    let mut groups = base.security_group_ids.take().unwrap_or_default();
    if groups.len() + targets.len() > 5 {
        return Err(invalid(
            &worker.id,
            "AWS Lambda supports at most five security groups, including the network group and one per linked Container",
        ));
    }
    let network = ctx.require_dependency::<AwsNetworkController>(&ResourceRef::new(
        Network::RESOURCE_TYPE,
        "default-network",
    ))?;
    let vpc = network
        .vpc_id
        .as_deref()
        .ok_or_else(|| invalid(&worker.id, "Container links require an imported VPC ID"))?;
    let ec2 = ctx
        .service_provider
        .get_aws_ec2_client(ctx.get_aws_config()?)
        .await?;
    for target in targets {
        groups.push(client_group(&*ec2, ctx.resource_prefix, &target, &worker.id, vpc).await?);
    }
    groups.sort();
    groups.dedup();
    base.security_group_ids = Some(groups);
    Ok(Some(base))
}

async fn client_group(
    ec2: &dyn Ec2Api,
    prefix: &str,
    target: &str,
    worker: &str,
    vpc: &str,
) -> Result<String> {
    let tags = [
        (ALIEN_STACK_TAG_KEY, prefix),
        (ALIEN_RESOURCE_TAG_KEY, target),
        (ALIEN_MANAGED_BY_TAG_KEY, "runtime"),
        ("purpose", "container-internal-client"),
    ];
    let mut filters = vec![Filter {
        name: "vpc-id".into(),
        values: vec![vpc.into()],
    }];
    filters.extend(tags.iter().map(|(key, value)| Filter {
        name: format!("tag:{key}"),
        values: vec![(*value).into()],
    }));
    let response = ec2
        .describe_security_groups(
            DescribeSecurityGroupsRequest::builder()
                .filters(filters)
                .build(),
        )
        .await
        .context(ErrorData::CloudPlatformError {
            message: format!("Resolve Container '{target}' client identity for Worker '{worker}'"),
            resource_id: Some(worker.into()),
        })?;
    let mut groups = response
        .security_group_info
        .map(|set| set.items)
        .unwrap_or_default();
    if groups.len() != 1 {
        return Err(invalid(
            worker,
            format!(
                "Linked Container '{target}' must have exactly one ready client security group in VPC '{vpc}'"
            ),
        ));
    }
    let group = groups.pop().expect("one group");
    if group.vpc_id.as_deref() != Some(vpc)
        || !tags.iter().all(|(key, value)| {
            group.tag_set.as_ref().is_some_and(|set| {
                set.items
                    .iter()
                    .any(|tag| tag.key == *key && tag.value == *value)
            })
        })
        || group
            .ip_permissions
            .as_ref()
            .is_some_and(|set| !set.items.is_empty())
        || group
            .ip_permissions_egress
            .as_ref()
            .is_some_and(|set| !set.items.is_empty())
    {
        return Err(invalid(
            worker,
            format!(
                "Linked Container '{target}' client group must be owned by that container, in the same VPC, and grant no network permissions"
            ),
        ));
    }
    group.group_id.filter(|id| !id.is_empty()).ok_or_else(|| {
        invalid(
            worker,
            format!("Linked Container '{target}' client group has no ID"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_aws_clients::ec2::MockEc2Api;
    use serde_json::json;

    #[tokio::test]
    async fn resolve_only_one_owned_empty_group_in_the_target_vpc() {
        let good = json!({"groupId":"sg-client", "vpcId":"vpc-test", "ipPermissions":{"item":[]}, "ipPermissionsEgress":{"item":[]}, "tagSet":{"item":[
            {"key":ALIEN_STACK_TAG_KEY,"value":"deployment-test"}, {"key":ALIEN_RESOURCE_TAG_KEY,"value":"api"},
            {"key":ALIEN_MANAGED_BY_TAG_KEY,"value":"runtime"}, {"key":"purpose","value":"container-internal-client"}
        ]}});
        let mut foreign_vpc = good.clone();
        foreign_vpc["vpcId"] = json!("vpc-foreign");
        let mut foreign_owner = good.clone();
        foreign_owner["tagSet"]["item"][1]["value"] = json!("other-api");
        let mut outbound = good.clone();
        outbound["ipPermissionsEgress"]["item"] =
            json!([{"ipProtocol":"-1","ipRanges":{"item":[{"cidrIp":"0.0.0.0/0"}]}}]);
        let mut inbound = good.clone();
        inbound["ipPermissions"]["item"] =
            json!([{"ipProtocol":"-1","ipRanges":{"item":[{"cidrIp":"0.0.0.0/0"}]}}]);
        for (groups, accepted) in [
            (vec![good.clone()], true),
            (vec![], false),
            (vec![good.clone(), good], false),
            (vec![foreign_vpc], false),
            (vec![foreign_owner], false),
            (vec![outbound], false),
            (vec![inbound], false),
        ] {
            let mut ec2 = MockEc2Api::new();
            ec2.expect_describe_security_groups()
                .times(1)
                .withf(|request| {
                    let filters = request.filters.as_ref().unwrap();
                    filters.len() == 5
                        && [
                            ("vpc-id", "vpc-test"),
                            ("tag:deployment", "deployment-test"),
                            ("tag:resource", "api"),
                            ("tag:managed-by", "runtime"),
                            ("tag:purpose", "container-internal-client"),
                        ]
                        .iter()
                        .all(|(name, value)| {
                            filters
                                .iter()
                                .any(|filter| filter.name == *name && filter.values == [*value])
                        })
                })
                .returning(move |_| {
                    Ok(
                        serde_json::from_value(json!({"securityGroupInfo":{"item":groups}}))
                            .unwrap(),
                    )
                });
            let result = client_group(&ec2, "deployment-test", "api", "caller", "vpc-test").await;
            assert_eq!(result.is_ok(), accepted, "{result:?}");
            if accepted {
                assert_eq!(result.unwrap(), "sg-client");
            }
        }
    }
}
