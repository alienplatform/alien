//! Read-only verification of the setup-owned sandbox network before publishing access.

use std::collections::{BTreeMap, BTreeSet};

use alien_aws_clients::ec2::{
    DescribeRouteTablesRequest, DescribeSubnetsRequest, DescribeVpcsRequest, Ec2Api, Filter, Route,
    RouteTable, Subnet,
};
use alien_core::import::data::AwsSandboxNetwork;
use alien_error::{AlienError, Context};

use crate::{ErrorData, Result};

fn invalid(resource_id: &str, message: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        message: message.into(),
        resource_id: Some(resource_id.into()),
    })
}

/// Discover CIDRs and verify effective routes, including implicit main-table associations.
/// This does not change the Frozen network. Endpoint, peering, propagated, appliance and
/// IPv6 routes are refused: gateway tasks must share the deployment's NAT network position.
pub async fn validate_aws_sandbox_network(
    client: &dyn Ec2Api,
    resource_id: &str,
    vpc_id: &str,
    declared: &AwsSandboxNetwork,
    workload_subnet_ids: &[String],
) -> Result<AwsSandboxNetwork> {
    let groups = [
        declared.sandbox_subnet_ids.as_slice(),
        declared.gateway_subnet_ids.as_slice(),
        workload_subnet_ids,
    ];
    let mut ids = BTreeSet::new();
    for group in groups {
        if group.is_empty() || group.iter().any(|id| !ids.insert(id.clone())) {
            return Err(invalid(resource_id, "Sandbox connector, gateway and workload subnets must be nonempty, distinct and dedicated"));
        }
    }
    if !(2..=3).contains(&declared.gateway_subnet_ids.len()) {
        return Err(invalid(
            resource_id,
            "Provide two or three dedicated gateway subnets",
        ));
    }
    let cloud_error = |operation: &str| ErrorData::CloudPlatformError {
        message: format!("{operation} failed while verifying sandbox routes in VPC '{vpc_id}'"),
        resource_id: Some(resource_id.into()),
    };
    let vpcs = client
        .describe_vpcs(DescribeVpcsRequest {
            vpc_ids: Some(vec![vpc_id.into()]),
            ..Default::default()
        })
        .await
        .context(cloud_error("DescribeVpcs"))?;
    let vpc = vpcs
        .vpc_set
        .and_then(|set| {
            set.items
                .into_iter()
                .find(|vpc| vpc.vpc_id.as_deref() == Some(vpc_id))
        })
        .ok_or_else(|| {
            invalid(
                resource_id,
                format!("Deployment VPC '{vpc_id}' was not returned"),
            )
        })?;
    let mut vpc_cidrs: BTreeSet<String> = vpc.cidr_block.into_iter().collect();
    if let Some(associations) = vpc.cidr_block_association_set {
        // Denying a range that is still associating or disassociating is conservative.
        vpc_cidrs.extend(
            associations
                .items
                .into_iter()
                .map(|association| association.cidr_block),
        );
    }
    if vpc_cidrs.is_empty() {
        return Err(invalid(
            resource_id,
            format!("VPC '{vpc_id}' has no reported IPv4 CIDRs"),
        ));
    }
    let mut subnets = BTreeMap::new();
    let mut next_token = None;
    loop {
        let response = client
            .describe_subnets(DescribeSubnetsRequest {
                subnet_ids: Some(ids.iter().cloned().collect()),
                next_token,
                ..Default::default()
            })
            .await
            .context(cloud_error("DescribeSubnets"))?;
        for subnet in response.subnet_set.into_iter().flat_map(|set| set.items) {
            let id = subnet.subnet_id.clone().ok_or_else(|| {
                invalid(
                    resource_id,
                    "DescribeSubnets returned a subnet without an ID",
                )
            })?;
            if !ids.contains(&id)
                || subnet.vpc_id.as_deref() != Some(vpc_id)
                || subnet.state.as_deref() != Some("available")
            {
                return Err(invalid(
                    resource_id,
                    format!("Subnet '{id}' must be available in VPC '{vpc_id}'"),
                ));
            }
            subnets.insert(id, subnet);
        }
        next_token = response.next_token.filter(|token| !token.is_empty());
        if next_token.is_none() {
            break;
        }
    }
    for id in &ids {
        if !subnets.contains_key(id) {
            return Err(invalid(
                resource_id,
                format!("Subnet '{id}' was not returned by DescribeSubnets"),
            ));
        }
    }
    for group in &groups[..2] {
        let mut zones = BTreeSet::new();
        for id in *group {
            let zone = subnets[id].availability_zone_id.as_ref().ok_or_else(|| {
                invalid(
                    resource_id,
                    format!("Subnet '{id}' has no reported availability zone ID"),
                )
            })?;
            zones.insert(zone);
        }
        if zones.len() < 2 {
            return Err(invalid(
                resource_id,
                format!(
                    "Subnets {} must span at least two availability zones",
                    group.join(", ")
                ),
            ));
        }
    }
    let mut tables = Vec::new();
    let mut next_token = None;
    loop {
        let response = client
            .describe_route_tables(DescribeRouteTablesRequest {
                filters: Some(vec![Filter {
                    name: "vpc-id".into(),
                    values: vec![vpc_id.into()],
                }]),
                next_token,
                ..Default::default()
            })
            .await
            .context(cloud_error("DescribeRouteTables"))?;
        tables.extend(
            response
                .route_table_set
                .into_iter()
                .flat_map(|set| set.items),
        );
        next_token = response.next_token.filter(|token| !token.is_empty());
        if next_token.is_none() {
            break;
        }
    }
    let mut nat_id = None;
    for (index, group) in groups.iter().enumerate() {
        for id in *group {
            let subnet = &subnets[id];
            if subnet
                .ipv6_cidr_block_association_set
                .as_ref()
                .is_some_and(|set| !set.items.is_empty())
            {
                return Err(invalid(
                    resource_id,
                    format!("Subnet '{id}' must not have IPv6 CIDRs for internet sandbox egress"),
                ));
            }
            let table = effective_table(resource_id, id, &tables)?;
            let routes = table
                .route_set
                .as_ref()
                .filter(|set| !set.items.is_empty())
                .ok_or_else(|| {
                    invalid(resource_id, format!("Subnet '{id}' has no reported routes"))
                })?;
            let mut defaults = 0;
            for route in &routes.items {
                let local = route.gateway_id.as_deref() == Some("local")
                    && route
                        .destination_cidr_block
                        .as_ref()
                        .is_some_and(|cidr| vpc_cidrs.contains(cidr))
                    && route.destination_ipv6_cidr_block.is_none()
                    && route.destination_prefix_list_id.is_none()
                    && route.state.as_deref() == Some("active");
                if local {
                    continue;
                }
                let nat_default = index != 0
                    && route.destination_cidr_block.as_deref() == Some("0.0.0.0/0")
                    && route.destination_ipv6_cidr_block.is_none()
                    && route.destination_prefix_list_id.is_none()
                    && route
                        .nat_gateway_id
                        .as_ref()
                        .is_some_and(|nat| nat.starts_with("nat-"))
                    && route.state.as_deref() == Some("active");
                if !nat_default {
                    return Err(invalid(resource_id, format!("Subnet '{id}', route table '{}': route {} is not allowed; connector subnets require only local VPC routes, gateway and workload subnets require only local VPC routes and an active NAT default route (no VPC endpoints)", table.route_table_id.as_deref().unwrap_or("unknown"), describe_route(route))));
                }
                defaults += 1;
                let current = route.nat_gateway_id.as_ref().expect("checked above");
                if nat_id.is_some_and(|previous| previous != current) {
                    return Err(invalid(resource_id, format!("Subnet '{id}' route 0.0.0.0/0 uses NAT '{current}'; gateway and workload subnets must share one deployment NAT gateway")));
                }
                nat_id = Some(current);
            }
            if index != 0 && defaults != 1 {
                return Err(invalid(resource_id, format!("Subnet '{id}' must have exactly one active 0.0.0.0/0 route to the deployment NAT gateway")));
            }
        }
    }
    let cidrs = |group: &[String]| -> Result<Vec<String>> {
        group
            .iter()
            .map(|id| subnet_cidr(resource_id, id, &subnets[id]))
            .collect()
    };
    let actual = AwsSandboxNetwork {
        sandbox_subnet_ids: declared.sandbox_subnet_ids.clone(),
        gateway_subnet_ids: declared.gateway_subnet_ids.clone(),
        sandbox_cidrs: cidrs(groups[0])?,
        gateway_cidrs: cidrs(groups[1])?,
        workload_cidrs: cidrs(groups[2])?,
        vpc_cidrs: vpc_cidrs.into_iter().collect(),
    };
    // CloudFormation BYO setup uses explicit CIDRs for its Frozen connector SG. Never
    // accept a broader caller-supplied range merely because the selected subnet exists.
    if !declared.gateway_cidrs.is_empty() && declared.gateway_cidrs != actual.gateway_cidrs {
        return Err(invalid(resource_id, format!("Gateway subnet CIDRs {:?} do not match setup's connector security-group destinations {:?}; correct setup for subnets {}", actual.gateway_cidrs, declared.gateway_cidrs, declared.gateway_subnet_ids.join(", "))));
    }
    Ok(actual)
}

fn subnet_cidr(resource_id: &str, id: &str, subnet: &Subnet) -> Result<String> {
    subnet
        .cidr_block
        .clone()
        .filter(|cidr| !cidr.is_empty())
        .ok_or_else(|| {
            invalid(
                resource_id,
                format!("Subnet '{id}' has no reported IPv4 CIDR"),
            )
        })
}

fn effective_table<'a>(
    resource_id: &str,
    subnet_id: &str,
    tables: &'a [RouteTable],
) -> Result<&'a RouteTable> {
    let associated = |table: &&RouteTable, main: bool| {
        table.association_set.as_ref().is_some_and(|set| {
            set.items.iter().any(|association| {
                if main {
                    association.main == Some(true)
                } else {
                    association.subnet_id.as_deref() == Some(subnet_id)
                }
            })
        })
    };
    let explicit: Vec<_> = tables
        .iter()
        .filter(|table| associated(table, false))
        .collect();
    if explicit.len() == 1 {
        return Ok(explicit[0]);
    }
    if explicit.is_empty() {
        let main: Vec<_> = tables
            .iter()
            .filter(|table| associated(table, true))
            .collect();
        if main.len() == 1 {
            return Ok(main[0]);
        }
    }
    Err(invalid(
        resource_id,
        format!("Subnet '{subnet_id}' has an ambiguous or missing effective route table"),
    ))
}

fn describe_route(route: &Route) -> String {
    let destination = route
        .destination_cidr_block
        .as_ref()
        .or(route.destination_ipv6_cidr_block.as_ref())
        .or(route.destination_prefix_list_id.as_ref())
        .map(String::as_str)
        .unwrap_or("unknown destination");
    let target = route
        .gateway_id
        .as_ref()
        .or(route.nat_gateway_id.as_ref())
        .or(route.network_interface_id.as_ref())
        .or(route.instance_id.as_ref())
        .or(route.vpc_peering_connection_id.as_ref())
        .or(route.transit_gateway_id.as_ref())
        .map(String::as_str)
        .unwrap_or("unsupported target");
    format!(
        "{destination} -> {target} ({})",
        route.state.as_deref().unwrap_or("unknown state")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_aws_clients::ec2::MockEc2Api;
    use serde_json::{json, Value};

    fn fixture(extra: Option<Value>, nat: &str) -> (MockEc2Api, AwsSandboxNetwork) {
        let mut client = MockEc2Api::new();
        client.expect_describe_vpcs().times(1).return_once(|_| Ok(serde_json::from_value(json!({
            "vpcSet":{"item":[{"vpcId":"vpc-test","cidrBlock":"10.0.0.0/16","cidrBlockAssociationSet":{"item":[{"cidrBlock":"198.18.0.0/16"}]}}]}
        })).unwrap()));
        client.expect_describe_subnets().times(1).return_once(|request| {
            assert_eq!(request.subnet_ids.unwrap().len(), 5);
            let items: Vec<_> = ["connector-a", "connector-b", "gateway-a", "gateway-b", "workload"].iter().enumerate().map(|(i,id)| json!({
                "subnetId":id,"vpcId":"vpc-test","state":"available","cidrBlock":format!("10.0.{i}.0/24"),"availabilityZoneId":format!("zone-{}",i%2)
            })).collect();
            Ok(serde_json::from_value(json!({"subnetSet":{"item":items}})).unwrap())
        });
        let local =
            json!({"destinationCidrBlock":"10.0.0.0/16","gatewayId":"local","state":"active"});
        let mut workload_routes = vec![
            local.clone(),
            json!({"destinationCidrBlock":"0.0.0.0/0","natGatewayId":nat,"state":"active"}),
        ];
        if let Some(extra) = extra {
            workload_routes.push(extra);
        }
        let pages = [
            json!({"routeTableSet":{"item":[{"routeTableId":"main","vpcId":"vpc-test","routeSet":{"item":[local]},"associationSet":{"item":[{"main":true}]}}]},"nextToken":"page-two"}),
            json!({"routeTableSet":{"item":[{"routeTableId":"nat","vpcId":"vpc-test","routeSet":{"item":workload_routes},"associationSet":{"item":[{"subnetId":"gateway-a"},{"subnetId":"gateway-b"},{"subnetId":"workload"}]}}]}}),
        ];
        let mut pages = pages.into_iter();
        client
            .expect_describe_route_tables()
            .times(2)
            .returning(move |request| {
                assert_eq!(request.filters.unwrap()[0].values, ["vpc-test"]);
                let page = pages.next().unwrap();
                assert_eq!(
                    request.next_token.is_none(),
                    page.get("nextToken").is_some()
                );
                Ok(serde_json::from_value(page).unwrap())
            });
        let declared = AwsSandboxNetwork {
            sandbox_subnet_ids: vec!["connector-a".into(), "connector-b".into()],
            gateway_subnet_ids: vec!["gateway-a".into(), "gateway-b".into()],
            sandbox_cidrs: vec![],
            gateway_cidrs: vec![],
            workload_cidrs: vec![],
            vpc_cidrs: vec![],
        };
        (client, declared)
    }

    #[tokio::test]
    async fn discovers_actual_cidrs_and_implicit_main_routes_across_pages() {
        let (client, declared) = fixture(None, "nat-shared");
        let actual = validate_aws_sandbox_network(
            &client,
            "network",
            "vpc-test",
            &declared,
            &["workload".into()],
        )
        .await
        .unwrap();
        assert_eq!(actual.sandbox_cidrs, ["10.0.0.0/24", "10.0.1.0/24"]);
        assert_eq!(actual.gateway_cidrs, ["10.0.2.0/24", "10.0.3.0/24"]);
        assert_eq!(actual.workload_cidrs, ["10.0.4.0/24"]);
        assert_eq!(actual.vpc_cidrs, ["10.0.0.0/16", "198.18.0.0/16"]);
    }

    #[tokio::test]
    async fn refuses_endpoint_peering_ipv6_and_blackhole_routes_with_coordinates() {
        for extra in [
            json!({"destinationPrefixListId":"pl-s3","gatewayId":"vpce-s3","state":"active"}),
            json!({"destinationCidrBlock":"10.9.0.0/16","vpcPeeringConnectionId":"pcx-test","state":"active"}),
            json!({"destinationIpv6CidrBlock":"::/0","gatewayId":"igw-test","state":"active"}),
            json!({"destinationCidrBlock":"192.168.0.0/16","networkInterfaceId":"eni-test","state":"blackhole"}),
        ] {
            let (client, declared) = fixture(Some(extra), "nat-shared");
            let error = validate_aws_sandbox_network(
                &client,
                "network",
                "vpc-test",
                &declared,
                &["workload".into()],
            )
            .await
            .unwrap_err();
            assert_eq!(error.code, "RESOURCE_CONFIG_INVALID");
            assert!(error.message.contains("gateway-a"), "{error}");
            assert!(error.message.contains("route table 'nat'"), "{error}");
            assert!(error.message.contains("route "), "{error}");
        }
    }

    #[tokio::test]
    async fn refuses_broad_setup_security_group_destinations() {
        let (client, mut declared) = fixture(None, "nat-shared");
        declared.gateway_cidrs = vec!["10.0.0.0/16".into()];
        let error = validate_aws_sandbox_network(
            &client,
            "network",
            "vpc-test",
            &declared,
            &["workload".into()],
        )
        .await
        .unwrap_err();
        assert!(
            error
                .message
                .contains("connector security-group destinations"),
            "{error}"
        );
    }
    #[tokio::test]
    async fn failed_route_refresh_withdraws_a_previously_verified_binding() {
        use crate::controller_test::SingleControllerExecutor;
        use crate::{
            AwsNetworkController, AwsSandboxController, AwsSandboxState,
            MockPlatformServiceProvider, ResourceController,
        };
        use alien_core::{Network, NetworkSettings, Platform, Sandbox, SandboxCode, SandboxEgress};
        use std::sync::Arc;

        let (client, declared) = fixture(
            Some(json!({"destinationPrefixListId":"pl-s3","gatewayId":"vpce-s3","state":"active"})),
            "nat-shared",
        );
        let client = Arc::new(client);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_aws_ec2_client()
            .returning(move |_| Ok(client.clone()));
        let controller = AwsSandboxController {
            state: AwsSandboxState::Ready,
            egress: Some(SandboxEgress::Internet.into()),
            egress_network_verified: true,
            image_arn: Some("image-test".into()),
            active_version: Some("1.0".into()),
            region: Some("us-east-1".into()),
            ..Default::default()
        };
        assert!(controller.get_binding_params().unwrap().is_some());
        let mut executor = SingleControllerExecutor::builder()
            .resource(
                Sandbox::new("subject".into())
                    .code(SandboxCode::Image {
                        image: "s3://test/bundle.zip".into(),
                    })
                    .egress(SandboxEgress::Internet)
                    .lifecycle(alien_core::SandboxLifecyclePolicy {
                        max_lifetime_seconds: None,
                        idle_pause_seconds: None,
                    })
                    .build(),
            )
            .controller(controller)
            .platform(Platform::Aws)
            .service_provider(Arc::new(provider))
            .with_dependency(
                Network::new("network".into())
                    .settings(NetworkSettings::Create {
                        cidr: Some("10.0.0.0/16".into()),
                        availability_zones: 2,
                    })
                    .build(),
                AwsNetworkController {
                    vpc_id: Some("vpc-test".into()),
                    sandbox_network: Some(declared),
                    private_subnet_ids: vec!["workload".into()],
                    ..Default::default()
                },
            )
            .build()
            .await
            .unwrap();
        let error = executor.step().await.unwrap_err();
        assert!(error.message.contains("gateway-a"), "{error}");
        let controller = executor.internal_state::<AwsSandboxController>().unwrap();
        assert!(!controller.egress_network_verified);
        assert!(controller.get_binding_params().unwrap().is_none());
    }
}
