//! Direct setup's dedicated subnets. Existing workload routes and the NAT stay in place.
use super::*;
use alien_core::import::data::AwsSandboxNetwork;
use alien_core::InitialSetupAuthority;
use std::net::Ipv4Addr;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SandboxNetworkSetup {
    vpc_id: String,
    nat_id: String,
    subnets: Vec<SetupSubnet>,
    /// One route-less table and one NAT table, in that order.
    tables: [Option<String>; 2],
    /// Teardown lookups for a subnet whose create response was lost.
    #[serde(default)]
    lost_create_lookups: u32,
}

/// How many teardown lookups a lost subnet create gets before it is taken as never created.
const LOST_CREATE_LOOKUPS: u32 = 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetupSubnet {
    cidr: String,
    zone: String,
    gateway: bool,
    id: Option<String>,
}

fn invalid(id: &str, message: impl Into<String>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        resource_id: Some(id.into()),
        message: message.into(),
    })
}

fn range(cidr: &str) -> Option<(u32, u32)> {
    let (ip, bits) = cidr.split_once('/')?;
    let ip = u32::from(ip.parse::<Ipv4Addr>().ok()?);
    let bits: u32 = bits.parse().ok()?;
    if bits > 32 {
        return None;
    }
    let mask = u32::MAX.checked_shl(32 - bits).unwrap_or(0);
    Some((ip & mask, ip | !mask))
}

/// Allocate only unused /24s; the existing direct-network /20s are never subdivided.
fn unused_cidrs(vpc: &str, occupied: &[String], count: usize, id: &str) -> Result<Vec<String>> {
    let (start, end) = range(vpc)
        .filter(|(a, b)| b - a >= 255 && b - a < 1 << 20)
        .ok_or_else(|| {
            invalid(
                id,
                "Internet sandbox setup requires free /24 blocks in an IPv4 VPC CIDR",
            )
        })?;
    let used = occupied
        .iter()
        .map(|cidr| {
            range(cidr).ok_or_else(|| invalid(id, format!("Invalid existing subnet CIDR '{cidr}'")))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut selected = Vec::new();
    for candidate in (u64::from(start)..=u64::from(end) - 255).step_by(256) {
        let first = candidate as u32;
        if used.iter().any(|(a, b)| first <= *b && first + 255 >= *a) {
            continue;
        }
        selected.push(format!("{}/24", Ipv4Addr::from(first)));
        if selected.len() == count {
            return Ok(selected);
        }
    }
    Err(invalid(
        id,
        format!("Internet sandbox setup needs {count} unused /24 subnets in VPC CIDR '{vpc}'"),
    ))
}

fn table_name(prefix: &str, id: &str, gateway: bool) -> String {
    format!(
        "{prefix}-{id}-sandbox-{}",
        if gateway { "gateway-rt" } else { "isolated-rt" }
    )
}

/// Existing imported coordinates already satisfy this requirement; only direct setup
/// may create missing children. The declaration stays out of persisted Network config.
pub(super) fn required(ctx: &ResourceControllerContext<'_>) -> Result<bool> {
    if !alien_core::sandbox_egress::has_sandbox_gateway(ctx.desired_stack) {
        return Ok(false);
    }
    let network = alien_core::sandbox_egress::sandbox_egress_network(ctx.desired_stack)
        .map_err(|error| invalid(ctx.desired_config.id(), error.to_string()))?;
    Ok(network.id == ctx.desired_config.id())
}

impl AwsNetworkController {
    pub(super) async fn reconcile_sandbox_network(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<bool> {
        let config = ctx.desired_resource_config::<Network>()?;
        if !required(ctx)? {
            return Ok(true);
        }
        // An imported network already has setup's coordinates; Ready verifies them read-only.
        if self.sandbox_network.is_some() && self.sandbox_setup.is_none() {
            return Ok(true);
        }
        if ctx.initial_setup_authority != InitialSetupAuthority::DirectSetup {
            return Err(invalid(
                &config.id,
                "Dedicated sandbox subnets must be created by direct setup",
            ));
        }
        let id = config.id.as_str();
        let vpc = self
            .vpc_id
            .clone()
            .ok_or_else(|| invalid(id, "Sandbox setup has no VPC"))?;
        let client = ctx
            .service_provider
            .get_aws_ec2_client(ctx.get_aws_config()?)
            .await?;
        let cloud = |operation: &str| ErrorData::CloudPlatformError {
            message: format!("{operation} failed during dedicated sandbox network setup"),
            resource_id: Some(id.into()),
        };
        if self.sandbox_setup.is_none() {
            let vpcs = client
                .describe_vpcs(
                    DescribeVpcsRequest::builder()
                        .vpc_ids(vec![vpc.clone()])
                        .build(),
                )
                .await
                .context(cloud("DescribeVpcs"))?;
            let cidr = vpcs
                .vpc_set
                .into_iter()
                .flat_map(|set| set.items)
                .find(|item| item.vpc_id.as_deref() == Some(&vpc))
                .and_then(|item| item.cidr_block)
                .ok_or_else(|| invalid(id, "VPC has no IPv4 CIDR"))?;
            let mut subnets = Vec::new();
            let mut next_token = None;
            loop {
                let response = client
                    .describe_subnets(DescribeSubnetsRequest {
                        filters: Some(vec![Filter {
                            name: "vpc-id".into(),
                            values: vec![vpc.clone()],
                        }]),
                        next_token,
                        ..Default::default()
                    })
                    .await
                    .context(cloud("DescribeSubnets"))?;
                subnets.extend(response.subnet_set.into_iter().flat_map(|set| set.items));
                next_token = response.next_token.filter(|token| !token.is_empty());
                if next_token.is_none() {
                    break;
                }
            }
            let mut zones = std::collections::BTreeSet::new();
            for subnet_id in &self.private_subnet_ids {
                let subnet = subnets
                    .iter()
                    .find(|subnet| subnet.subnet_id.as_ref() == Some(subnet_id))
                    .ok_or_else(|| {
                        invalid(id, format!("Workload subnet '{subnet_id}' was not found"))
                    })?;
                zones.insert(
                    subnet
                        .availability_zone
                        .clone()
                        .ok_or_else(|| invalid(id, "Workload subnet has no availability zone"))?,
                );
            }
            if zones.len() < 2 {
                return Err(invalid(id,"Internet sandbox setup requires workload subnets in at least two availability zones"));
            }
            let zones: Vec<_> = zones.into_iter().take(3).collect();
            let occupied = subnets
                .into_iter()
                .map(|subnet| {
                    subnet
                        .cidr_block
                        .ok_or_else(|| invalid(id, "Existing subnet has no IPv4 CIDR"))
                })
                .collect::<Result<Vec<_>>>()?;
            let blocks = unused_cidrs(&cidr, &occupied, zones.len() * 2, id)?;
            let tables = self.sandbox_setup_tables(ctx, &vpc, id).await?;
            let mut nat_id = None;
            for subnet in &self.private_subnet_ids {
                let table = super::super::aws_sandbox::effective_table(id, subnet, &tables)?;
                let defaults: Vec<_> = table
                    .route_set
                    .iter()
                    .flat_map(|set| &set.items)
                    .filter(|route| route.destination_cidr_block.as_deref() == Some("0.0.0.0/0"))
                    .collect();
                let nat = defaults
                    .first()
                    .filter(|_| defaults.len() == 1)
                    .filter(|route| route.state.as_deref() == Some("active"))
                    .and_then(|route| route.nat_gateway_id.clone())
                    .ok_or_else(|| {
                        invalid(id, "Workload subnets require one active NAT default route")
                    })?;
                if nat_id.as_ref().is_some_and(|current| current != &nat) {
                    return Err(invalid(id, "Workload subnets must share one NAT gateway"));
                }
                nat_id = Some(nat);
            }
            self.sandbox_setup = Some(SandboxNetworkSetup {
                vpc_id: vpc,
                nat_id: nat_id.ok_or_else(|| invalid(id, "Workload network has no NAT"))?,
                tables: [None, None],
                lost_create_lookups: 0,
                subnets: blocks
                    .into_iter()
                    .enumerate()
                    .map(|(i, cidr)| SetupSubnet {
                        cidr,
                        zone: zones[i % zones.len()].clone(),
                        gateway: i >= zones.len(),
                        id: None,
                    })
                    .collect(),
            });
            return Ok(false); // Persist allocation before making any cloud object.
        }
        let setup = self.sandbox_setup.as_ref().expect("initialized").clone();
        if setup.vpc_id != vpc {
            return Err(invalid(
                id,
                "Dedicated sandbox subnets cannot move to another VPC",
            ));
        }
        for (i, subnet) in setup.subnets.iter().enumerate() {
            if subnet.id.is_some() {
                continue;
            }
            let progress = self
                .ensure_subnet(
                    ctx,
                    SubnetSpec::builder()
                        .vpc_id(&vpc)
                        .cidr(&subnet.cidr)
                        .availability_zone(&subnet.zone)
                        .name(format!("{}-{id}-sandbox-{i}", ctx.resource_prefix))
                        .subnet_type(if subnet.gateway {
                            "SandboxGateway"
                        } else {
                            "Sandbox"
                        })
                        .resource_id(id)
                        .build(),
                )
                .await?;
            if let SubnetProgress::Created(subnet_id) = progress {
                self.sandbox_setup.as_mut().expect("initialized").subnets[i].id = Some(subnet_id);
            }
            return Ok(false);
        }
        let tables = self.sandbox_setup_tables(ctx, &vpc, id).await?;
        for index in 0..2 {
            let name = table_name(ctx.resource_prefix, id, index == 1);
            let matching: Vec<_> = tables
                .iter()
                .filter(|table| {
                    table.tag_set.as_ref().is_some_and(|set| {
                        set.items
                            .iter()
                            .any(|tag| tag.key == "Name" && tag.value == name)
                    })
                })
                .collect();
            if matching.len() > 1 {
                return Err(invalid(id,format!("Multiple dedicated route tables named '{name}'; retry cleanup before setup")));
            }
            let table = if let Some(table) = matching.first() {
                if !has_owned_tags(table.tag_set.as_ref(), ctx.resource_prefix, id) {
                    return Err(invalid(
                        id,
                        format!("Route table '{name}' belongs to another owner"),
                    ));
                }
                let table_id = table
                    .route_table_id
                    .clone()
                    .ok_or_else(|| invalid(id, "Dedicated route table has no ID"))?;
                if setup.tables[index]
                    .as_ref()
                    .is_some_and(|recorded| recorded != &table_id)
                {
                    return Err(invalid(id, "Dedicated route table identity changed"));
                }
                self.sandbox_setup.as_mut().expect("initialized").tables[index] = Some(table_id);
                *table
            } else {
                if setup.tables[index].is_some() {
                    return Err(invalid(
                        id,
                        "Recorded dedicated route table was not returned",
                    ));
                }
                let response = client
                    .create_route_table(
                        CreateRouteTableRequest::builder()
                            .vpc_id(vpc.clone())
                            .tag_specifications(vec![self.create_tag_specification(
                                ctx.resource_prefix,
                                id,
                                "route-table",
                                name,
                                [],
                            )])
                            .build(),
                    )
                    .await
                    .context(cloud("CreateRouteTable"))?;
                self.sandbox_setup.as_mut().expect("initialized").tables[index] = Some(
                    response
                        .route_table
                        .and_then(|table| table.route_table_id)
                        .ok_or_else(|| invalid(id, "CreateRouteTable returned no ID"))?,
                );
                return Ok(false);
            };
            let table_id = table.route_table_id.as_ref().expect("validated");
            if index == 1
                && !table
                    .route_set
                    .iter()
                    .flat_map(|set| &set.items)
                    .any(|route| route.destination_cidr_block.as_deref() == Some("0.0.0.0/0"))
            {
                client
                    .create_route(
                        CreateRouteRequest::builder()
                            .route_table_id(table_id.clone())
                            .destination_cidr_block("0.0.0.0/0".into())
                            .nat_gateway_id(setup.nat_id.clone())
                            .build(),
                    )
                    .await
                    .context(cloud("CreateRoute"))?;
                return Ok(false);
            }
            for subnet in setup
                .subnets
                .iter()
                .filter(|subnet| subnet.gateway == (index == 1))
            {
                let subnet_id = subnet.id.as_ref().expect("created");
                if table
                    .association_set
                    .iter()
                    .flat_map(|set| &set.items)
                    .any(|association| association.subnet_id.as_ref() == Some(subnet_id))
                {
                    continue;
                }
                client
                    .associate_route_table(
                        AssociateRouteTableRequest::builder()
                            .route_table_id(table_id.clone())
                            .subnet_id(subnet_id.clone())
                            .build(),
                    )
                    .await
                    .context(cloud("AssociateRouteTable"))?;
                return Ok(false);
            }
        }
        let ids = |gateway| {
            setup
                .subnets
                .iter()
                .filter(|subnet| subnet.gateway == gateway)
                .filter_map(|subnet| subnet.id.clone())
                .collect()
        };
        let declared = AwsSandboxNetwork {
            sandbox_subnet_ids: ids(false),
            gateway_subnet_ids: ids(true),
            sandbox_cidrs: vec![],
            gateway_cidrs: vec![],
            workload_cidrs: vec![],
            vpc_cidrs: vec![],
        };
        self.sandbox_network = Some(
            super::super::validate_aws_sandbox_network(
                client.as_ref(),
                id,
                &vpc,
                &declared,
                &self.private_subnet_ids,
            )
            .await?,
        );
        Ok(true)
    }

    async fn sandbox_setup_tables(
        &self,
        ctx: &ResourceControllerContext<'_>,
        vpc: &str,
        id: &str,
    ) -> Result<Vec<RouteTable>> {
        let client = ctx
            .service_provider
            .get_aws_ec2_client(ctx.get_aws_config()?)
            .await?;
        let mut tables = Vec::new();
        let mut next_token = None;
        loop {
            let response = client
                .describe_route_tables(DescribeRouteTablesRequest {
                    filters: Some(vec![Filter {
                        name: "vpc-id".into(),
                        values: vec![vpc.into()],
                    }]),
                    next_token,
                    ..Default::default()
                })
                .await
                .context(ErrorData::CloudPlatformError {
                    message: "Failed to discover sandbox route tables".into(),
                    resource_id: Some(id.into()),
                })?;
            tables.extend(
                response
                    .route_table_set
                    .into_iter()
                    .flat_map(|set| set.items),
            );
            next_token = response.next_token.filter(|token| !token.is_empty());
            if next_token.is_none() {
                return Ok(tables);
            }
        }
    }

    pub(super) async fn delete_sandbox_network(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<bool> {
        let Some(setup) = self.sandbox_setup.clone() else {
            return Ok(true);
        };
        // Only direct setup records these children, so the record alone proves ownership. Setup
        // teardown runs with setup's credentials but without the direct-setup authority marker.
        let config = ctx.desired_resource_config::<Network>()?;
        let id = config.id.as_str();
        let client = ctx
            .service_provider
            .get_aws_ec2_client(ctx.get_aws_config()?)
            .await?;
        // Recover the only in-flight subnet by its exact persisted create token before deletion.
        if let Some(attempt) = self.subnet_create_attempt.clone() {
            if matches!(attempt.subnet_type.as_str(), "Sandbox" | "SandboxGateway") {
                let found = self
                    .find_subnet_by_create_attempt(ctx, &setup.vpc_id, &attempt, id)
                    .await?;
                let record = self.sandbox_setup.as_mut().expect("recorded");
                match found {
                    Some(found) => {
                        record
                            .subnets
                            .iter_mut()
                            .find(|subnet| subnet.cidr == attempt.cidr)
                            .ok_or_else(|| {
                                invalid(id, "Lost subnet create has no recorded allocation")
                            })?
                            .id = Some(found);
                    }
                    // EC2 may not list a just-accepted subnet yet; look again before
                    // concluding the create never happened.
                    None if record.lost_create_lookups < LOST_CREATE_LOOKUPS => {
                        record.lost_create_lookups += 1;
                        return Ok(false);
                    }
                    None => {}
                }
                record.lost_create_lookups = 0;
                self.subnet_create_attempt = None;
                return Ok(false);
            }
        }
        if let Some((index, subnet)) = setup
            .subnets
            .iter()
            .enumerate()
            .find(|(_, subnet)| subnet.id.is_some())
        {
            let subnet_id = subnet.id.as_ref().expect("selected");
            match client.delete_subnet(subnet_id).await {
                Ok(()) => {}
                Err(error) if is_not_found(&error) => {}
                Err(error) if is_conflict(&error) => {
                    self.wait_for_delete_dependencies(
                        id,
                        "dedicated sandbox network".into(),
                        DeleteBlockers::default(),
                        NETWORK_INTERFACE_DRAIN_MAX_POLLS,
                    )?;
                    return Ok(false);
                }
                Err(error) => {
                    return Err(error).context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete dedicated subnet '{subnet_id}'"),
                        resource_id: Some(id.into()),
                    })
                }
            }
            self.wait_for_delete_dependencies_iterations = 0;
            self.sandbox_setup.as_mut().expect("recorded").subnets[index].id = None;
            return Ok(false);
        }
        // Tags also find a route table whose create response was lost, including duplicates.
        let tables = self.sandbox_setup_tables(ctx, &setup.vpc_id, id).await?;
        for table in tables {
            let owned = has_owned_tags(table.tag_set.as_ref(), ctx.resource_prefix, id);
            let named = table.tag_set.as_ref().is_some_and(|set| {
                set.items.iter().any(|tag| {
                    tag.key == "Name"
                        && [
                            table_name(ctx.resource_prefix, id, false),
                            table_name(ctx.resource_prefix, id, true),
                        ]
                        .contains(&tag.value)
                })
            });
            if !owned || !named {
                continue;
            }
            let table_id = table
                .route_table_id
                .ok_or_else(|| invalid(id, "Dedicated route table has no ID"))?;
            match client.delete_route_table(&table_id).await {
                Ok(()) => {}
                Err(error) if is_not_found(&error) => {}
                Err(error) if is_conflict(&error) => {
                    self.wait_for_delete_dependencies(
                        id,
                        "dedicated sandbox network".into(),
                        DeleteBlockers::default(),
                        NETWORK_INTERFACE_DRAIN_MAX_POLLS,
                    )?;
                    return Ok(false);
                }
                Err(error) => {
                    return Err(error).context(ErrorData::CloudPlatformError {
                        message: format!("Failed to delete dedicated route table '{table_id}'"),
                        resource_id: Some(id.into()),
                    })
                }
            }
            for recorded in &mut self.sandbox_setup.as_mut().expect("recorded").tables {
                if recorded.as_ref() == Some(&table_id) {
                    *recorded = None;
                }
            }
            return Ok(false);
        }
        // A missing list result is not proof of deletion for a recorded ID.
        for table_id in setup.tables.iter().flatten() {
            match client.delete_route_table(table_id).await {
                Ok(()) => {}
                Err(error) if is_not_found(&error) => {}
                Err(error) if is_conflict(&error) => {
                    self.wait_for_delete_dependencies(
                        id,
                        "dedicated sandbox network".into(),
                        DeleteBlockers::default(),
                        NETWORK_INTERFACE_DRAIN_MAX_POLLS,
                    )?;
                    return Ok(false);
                }
                Err(error) => {
                    return Err(error).context(ErrorData::CloudPlatformError {
                        message: format!(
                            "Failed to delete recorded dedicated route table '{table_id}'"
                        ),
                        resource_id: Some(id.into()),
                    })
                }
            }
            for recorded in &mut self.sandbox_setup.as_mut().expect("recorded").tables {
                if recorded.as_ref() == Some(table_id) {
                    *recorded = None;
                }
            }
            return Ok(false);
        }
        self.wait_for_delete_dependencies_iterations = 0;
        self.sandbox_setup = None;
        self.sandbox_network = None;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn allocation_preserves_existing_direct_subnets_and_refuses_exhaustion() {
        let used = vec![
            "10.54.0.0/20".into(),
            "10.54.16.0/20".into(),
            "10.54.32.0/20".into(),
            "10.54.128.0/20".into(),
            "10.54.144.0/20".into(),
            "10.54.160.0/20".into(),
        ];
        assert_eq!(
            unused_cidrs("10.54.0.0/16", &used, 6, "network").unwrap(),
            [
                "10.54.48.0/24",
                "10.54.49.0/24",
                "10.54.50.0/24",
                "10.54.51.0/24",
                "10.54.52.0/24",
                "10.54.53.0/24"
            ]
        );
        assert!(unused_cidrs("10.54.0.0/24", &["10.54.0.0/28".into()], 1, "network").is_err());
        assert!(unused_cidrs("10.54.0.0/16", &["invalid".into()], 4, "network").is_err());
    }
}

#[cfg(test)]
#[path = "aws_sandbox_setup_tests.rs"]
mod lifecycle_tests;
