//! Direct setup's dedicated subnets. Existing workload routes and the NAT stay in place.
use super::*;
use alien_aws_clients::ec2::{
    DeleteRouteRequest, Ec2Api, ReplaceRouteRequest, ReplaceRouteTableAssociationRequest, Route,
    RouteDestination,
};
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
    /// Lookups of each recorded route table a list did not return yet.
    #[serde(default)]
    table_lookups: [u32; 2],
    /// Idempotency token of each table's `CreateRouteTable` call, saved before the call, so a
    /// call repeated after a lost response returns the same table.
    #[serde(default)]
    table_tokens: [Option<String>; 2],
    /// A refresh could not verify the topology; the next setup run reconciles it.
    #[serde(default)]
    unverified: bool,
    /// Route and association changes in this setup run that the topology has not yet verified;
    /// see `MAX_TABLE_CHANGES`.
    #[serde(default)]
    table_changes: u32,
}

impl SandboxNetworkSetup {
    pub(super) fn is_unverified(&self) -> bool {
        self.unverified
    }
}

/// How many lookups a recorded route table missing from a list gets.
const LOST_CREATE_LOOKUPS: u32 = 12;

/// Route and association changes allowed before the topology verifies. Building it from scratch
/// takes at most seven; a count past this means something keeps undoing the repairs.
const MAX_TABLE_CHANGES: u32 = 16;

/// Where reconciling the dedicated sandbox network got to.
pub(super) enum SandboxStep {
    Ready,
    /// Step again after this delay.
    Again(Duration),
}

const AGAIN: SandboxStep = SandboxStep::Again(Duration::from_secs(1));

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

/// The first and last address of an IPv4 CIDR.
pub(super) fn range(cidr: &str) -> Option<(u32, u32)> {
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

/// The one NAT gateway the workload subnets' active default routes use.
fn workload_nat(id: &str, workload_subnet_ids: &[String], tables: &[RouteTable]) -> Result<String> {
    let mut nat_id = None;
    for subnet in workload_subnet_ids {
        let table = super::super::aws_sandbox::effective_table(id, subnet, tables)?;
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
            .ok_or_else(|| invalid(id, "Workload subnets require one active NAT default route"))?;
        if nat_id.as_ref().is_some_and(|current| current != &nat) {
            return Err(invalid(id, "Workload subnets must share one NAT gateway"));
        }
        nat_id = Some(nat);
    }
    nat_id.ok_or_else(|| invalid(id, "Workload network has no NAT"))
}

/// The destination that names `route` in its table, for deleting it.
fn route_destination(id: &str, table_id: &str, route: &Route) -> Result<RouteDestination> {
    if let Some(cidr) = &route.destination_cidr_block {
        return Ok(RouteDestination::Ipv4Cidr(cidr.clone()));
    }
    if let Some(cidr) = &route.destination_ipv6_cidr_block {
        return Ok(RouteDestination::Ipv6Cidr(cidr.clone()));
    }
    if let Some(prefix_list) = &route.destination_prefix_list_id {
        return Ok(RouteDestination::PrefixList(prefix_list.clone()));
    }
    Err(invalid(
        id,
        format!("Route table '{table_id}' has a route without a destination"),
    ))
}

/// Whether EC2 still has the VPC.
async fn vpc_exists(client: &dyn Ec2Api, vpc_id: &str, id: &str) -> Result<bool> {
    match client
        .describe_vpcs(
            DescribeVpcsRequest::builder()
                .vpc_ids(vec![vpc_id.to_string()])
                .build(),
        )
        .await
    {
        Ok(response) => Ok(response
            .vpc_set
            .into_iter()
            .flat_map(|set| set.items)
            .any(|vpc| vpc.vpc_id.as_deref() == Some(vpc_id))),
        Err(error) if is_not_found(&error) => Ok(false),
        Err(error) => Err(error).context(ErrorData::CloudPlatformError {
            message: format!("Failed to check whether VPC '{vpc_id}' still exists"),
            resource_id: Some(id.into()),
        }),
    }
}

fn subnet_spec<'a>(
    prefix: &str,
    id: &'a str,
    vpc: &'a str,
    index: usize,
    subnet: &'a SetupSubnet,
) -> SubnetSpec<'a> {
    SubnetSpec::builder()
        .vpc_id(vpc)
        .cidr(&subnet.cidr)
        .availability_zone(&subnet.zone)
        .name(format!("{prefix}-{id}-sandbox-{index}"))
        .subnet_type(if subnet.gateway {
            "SandboxGateway"
        } else {
            "Sandbox"
        })
        .resource_id(id)
        .build()
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
    ) -> Result<SandboxStep> {
        let config = ctx.desired_resource_config::<Network>()?;
        if !required(ctx)? {
            return Ok(SandboxStep::Ready);
        }
        let direct = ctx.initial_setup_authority == InitialSetupAuthority::DirectSetup;
        // Subnets setup does not own (an import or customer-provided subnets), and any run
        // without direct setup's authority, change nothing: a complete topology is verified
        // read-only. Only direct setup may create or repair its own subnets.
        if self.sandbox_setup.is_none() || !direct {
            if self.sandbox_network.is_some() {
                self.verify_sandbox_network(ctx).await?;
                return Ok(SandboxStep::Ready);
            }
            if !direct {
                return Err(invalid(
                    &config.id,
                    "Dedicated sandbox subnets must be created by direct setup",
                ));
            }
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
            let nat_id = workload_nat(id, &self.private_subnet_ids, &tables)?;
            self.sandbox_setup = Some(SandboxNetworkSetup {
                vpc_id: vpc,
                nat_id,
                tables: [None, None],
                table_lookups: [0, 0],
                table_tokens: [None, None],
                unverified: false,
                table_changes: 0,
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
            return Ok(AGAIN); // Persist allocation before making any cloud object.
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
                .ensure_subnet(ctx, subnet_spec(ctx.resource_prefix, id, &vpc, i, subnet))
                .await?;
            match progress {
                SubnetProgress::Created(subnet_id) => {
                    self.sandbox_setup.as_mut().expect("initialized").subnets[i].id =
                        Some(subnet_id);
                }
                SubnetProgress::HeldByAnother(held) => {
                    return Err(subnet_cidr_held(&subnet.cidr, held, id));
                }
                SubnetProgress::LookAgain(AwsNetworkHandlerAction::Stay {
                    suggested_delay: Some(delay),
                    ..
                }) => return Ok(SandboxStep::Again(delay)),
                SubnetProgress::AttemptRecorded | SubnetProgress::LookAgain(_) => {}
            }
            return Ok(AGAIN);
        }
        let tables = self.sandbox_setup_tables(ctx, &vpc, id).await?;
        // The gateway subnets' default route follows the workload subnets' NAT gateway.
        let nat_id = workload_nat(id, &self.private_subnet_ids, &tables)?;
        if nat_id != setup.nat_id {
            info!(network_id = %id, nat_id = %nat_id, "Workload NAT gateway changed; following it");
            self.sandbox_setup.as_mut().expect("initialized").nat_id = nat_id.clone();
        }
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
                let record = self.sandbox_setup.as_mut().expect("initialized");
                record.tables[index] = Some(table_id);
                record.table_tokens[index] = None;
                record.table_lookups[index] = 0;
                *table
            } else {
                let record = self.sandbox_setup.as_mut().expect("initialized");
                if setup.tables[index].is_some() {
                    // EC2 may not list a just-created table yet; look again before deciding
                    // the recorded table is missing.
                    if record.table_lookups[index] < LOST_CREATE_LOOKUPS {
                        record.table_lookups[index] += 1;
                        return Ok(AGAIN);
                    }
                    record.table_lookups[index] = 0;
                    return Err(invalid(
                        id,
                        "Recorded dedicated route table was not returned",
                    ));
                }
                // The token is saved by a step of its own. A create repeated after a lost
                // response, while the list still misses the table, returns that same table.
                let Some(token) = setup.table_tokens[index].clone() else {
                    record.table_tokens[index] = Some(new_create_attempt_token());
                    return Ok(AGAIN);
                };
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
                            .client_token(token)
                            .build(),
                    )
                    .await
                    .context(cloud("CreateRouteTable"))?;
                let table_id = response
                    .route_table
                    .and_then(|table| table.route_table_id)
                    .ok_or_else(|| invalid(id, "CreateRouteTable returned no ID"))?;
                let record = self.sandbox_setup.as_mut().expect("initialized");
                record.tables[index] = Some(table_id);
                record.table_tokens[index] = None;
                return Ok(AGAIN);
            };
            let table_id = table.route_table_id.as_ref().expect("validated");
            let routes: Vec<_> = table.route_set.iter().flat_map(|set| &set.items).collect();
            // Only local VPC routes stay, plus the gateway table's NAT default route. Anything
            // else would widen what sandboxes or the gateway reach.
            for route in &routes {
                let ipv4_default = route.destination_cidr_block.as_deref() == Some("0.0.0.0/0");
                if route.gateway_id.as_deref() == Some("local") || (index == 1 && ipv4_default) {
                    continue;
                }
                if route.origin.as_deref() == Some("EnableVgwRoutePropagation") {
                    return Err(invalid(id, format!("Route table '{table_id}' has routes propagated from a virtual private gateway; turn off route propagation on it")));
                }
                let destination = route_destination(id, table_id, route)?;
                info!(network_id = %id, route_table_id = %table_id, destination = ?destination, "Removing a route the dedicated sandbox route table must not have");
                self.count_table_change(id)?;
                match client
                    .delete_route(
                        DeleteRouteRequest::builder()
                            .route_table_id(table_id.clone())
                            .destination(destination)
                            .build(),
                    )
                    .await
                {
                    Ok(()) => {}
                    Err(error) if is_not_found(&error) => {}
                    Err(error) => return Err(error).context(cloud("DeleteRoute")),
                }
                return Ok(AGAIN);
            }
            if index == 1 {
                let nat_id = self
                    .sandbox_setup
                    .as_ref()
                    .expect("initialized")
                    .nat_id
                    .clone();
                let default = routes
                    .iter()
                    .find(|route| route.destination_cidr_block.as_deref() == Some("0.0.0.0/0"));
                match default {
                    None => {
                        self.count_table_change(id)?;
                        client
                            .create_route(
                                CreateRouteRequest::builder()
                                    .route_table_id(table_id.clone())
                                    .destination_cidr_block("0.0.0.0/0".into())
                                    .nat_gateway_id(nat_id.clone())
                                    .build(),
                            )
                            .await
                            .context(cloud("CreateRoute"))?;
                        return Ok(AGAIN);
                    }
                    Some(route)
                        if route.nat_gateway_id.as_ref() == Some(&nat_id)
                            && route.state.as_deref() == Some("active") => {}
                    Some(_) => {
                        info!(network_id = %id, route_table_id = %table_id, nat_id = %nat_id, "Pointing the gateway default route back at the workload NAT gateway");
                        self.count_table_change(id)?;
                        client
                            .replace_route(
                                ReplaceRouteRequest::builder()
                                    .route_table_id(table_id.clone())
                                    .destination_cidr_block("0.0.0.0/0".into())
                                    .nat_gateway_id(nat_id.clone())
                                    .build(),
                            )
                            .await
                            .context(cloud("ReplaceRoute"))?;
                        return Ok(AGAIN);
                    }
                }
            }
            for subnet in setup
                .subnets
                .iter()
                .filter(|subnet| subnet.gateway == (index == 1))
            {
                let subnet_id = subnet.id.as_ref().expect("created");
                // A subnet has at most one explicit association, in any table of the VPC.
                let current = tables.iter().find_map(|other| {
                    other
                        .association_set
                        .iter()
                        .flat_map(|set| &set.items)
                        .find(|association| association.subnet_id.as_ref() == Some(subnet_id))
                        .map(|association| (other.route_table_id.as_ref(), association))
                });
                if let Some((current_table, association)) = current {
                    if current_table == Some(table_id) {
                        continue;
                    }
                    let association_id = association
                        .route_table_association_id
                        .clone()
                        .ok_or_else(|| {
                            invalid(id, format!("Subnet '{subnet_id}' association has no ID"))
                        })?;
                    info!(network_id = %id, subnet_id = %subnet_id, route_table_id = %table_id, "Moving a dedicated subnet back to its route table");
                    self.count_table_change(id)?;
                    match client
                        .replace_route_table_association(
                            ReplaceRouteTableAssociationRequest::builder()
                                .association_id(association_id)
                                .route_table_id(table_id.clone())
                                .build(),
                        )
                        .await
                    {
                        Ok(_) => {}
                        // A resend after EC2 already replaced it; the next list shows the result.
                        Err(error) if is_not_found(&error) => {}
                        Err(error) => {
                            return Err(error).context(cloud("ReplaceRouteTableAssociation"))
                        }
                    }
                    return Ok(AGAIN);
                }
                self.count_table_change(id)?;
                client
                    .associate_route_table(
                        AssociateRouteTableRequest::builder()
                            .route_table_id(table_id.clone())
                            .subnet_id(subnet_id.clone())
                            .build(),
                    )
                    .await
                    .context(cloud("AssociateRouteTable"))?;
                return Ok(AGAIN);
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
        if let Some(setup) = self.sandbox_setup.as_mut() {
            setup.unverified = false;
            setup.table_changes = 0;
        }
        Ok(SandboxStep::Ready)
    }

    /// Counts one route or association change, refusing once `MAX_TABLE_CHANGES` were made
    /// without the topology verifying. Refusing ends this setup run and resets the count, so a
    /// later run, after whatever kept undoing the repairs stops, gets the full budget again.
    fn count_table_change(&mut self, id: &str) -> Result<()> {
        let setup = self.sandbox_setup.as_mut().expect("initialized");
        if setup.table_changes >= MAX_TABLE_CHANGES {
            let changes = std::mem::take(&mut setup.table_changes);
            return Err(invalid(
                id,
                format!(
                    "Direct setup changed its sandbox route tables {changes} times without them verifying; something keeps changing them back"
                ),
            ));
        }
        setup.table_changes += 1;
        Ok(())
    }

    /// Verify the recorded topology read-only. A failed check stays recorded on direct setup's
    /// subnets, so the next direct setup run repairs them.
    pub(super) async fn verify_sandbox_network(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<()> {
        let config = ctx.desired_resource_config::<Network>()?;
        let declared = self
            .sandbox_network
            .clone()
            .ok_or_else(|| invalid(&config.id, "Sandbox network has no recorded subnets"))?;
        let vpc_id = self
            .vpc_id
            .clone()
            .ok_or_else(|| invalid(&config.id, "Sandbox network setup is missing the VPC ID"))?;
        let client = ctx
            .service_provider
            .get_aws_ec2_client(ctx.get_aws_config()?)
            .await?;
        if let Some(setup) = self.sandbox_setup.as_mut() {
            setup.unverified = true;
        }
        self.sandbox_network = Some(
            super::super::validate_aws_sandbox_network(
                client.as_ref(),
                &config.id,
                &vpc_id,
                &declared,
                &self.private_subnet_ids,
            )
            .await?,
        );
        if let Some(setup) = self.sandbox_setup.as_mut() {
            setup.unverified = false;
        }
        Ok(())
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
        // A subnet create whose response was lost may have made a subnet that no read shows
        // yet, and an empty read never proves it did not. The attempt is settled the way setup
        // settles it: look by its token, then repeat the create. EC2 refuses a second subnet
        // with the CIDR, so the repeat either makes the subnet (deleted below), or conflicts
        // with the one holding it: ours, found by its token, or another, which proves this
        // attempt made nothing. Until then the attempt stays recorded.
        if let Some(attempt) = self.subnet_create_attempt.clone() {
            if matches!(attempt.subnet_type.as_str(), "Sandbox" | "SandboxGateway") {
                let (index, subnet) = setup
                    .subnets
                    .iter()
                    .enumerate()
                    .find(|(_, subnet)| subnet.cidr == attempt.cidr)
                    .ok_or_else(|| invalid(id, "Lost subnet create has no recorded allocation"))?;
                let progress = match self
                    .ensure_subnet(
                        ctx,
                        subnet_spec(ctx.resource_prefix, id, &setup.vpc_id, index, subnet),
                    )
                    .await
                {
                    Ok(progress) => progress,
                    // A VPC deleted outside Alien took every subnet in it along.
                    Err(error) => {
                        if vpc_exists(client.as_ref(), &setup.vpc_id, id).await? {
                            return Err(error);
                        }
                        info!(network_id = %id, vpc_id = %setup.vpc_id, "The VPC is gone, so the lost subnet create left nothing");
                        self.subnet_create_attempt = None;
                        return Ok(false);
                    }
                };
                match progress {
                    SubnetProgress::Created(subnet_id) => {
                        self.sandbox_setup.as_mut().expect("recorded").subnets[index].id =
                            Some(subnet_id);
                    }
                    SubnetProgress::HeldByAnother(held) => {
                        info!(network_id = %id, cidr = %attempt.cidr, holder = %held.holder, "Another subnet holds the CIDR, so the lost create made nothing");
                    }
                    SubnetProgress::AttemptRecorded | SubnetProgress::LookAgain(_) => {}
                }
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
            self.delete_removed_something = true;
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
            self.delete_removed_something = true;
            for recorded in &mut self.sandbox_setup.as_mut().expect("recorded").tables {
                if recorded.as_ref() == Some(&table_id) {
                    *recorded = None;
                }
            }
            return Ok(false);
        }
        // A missing list result is not proof of deletion for a recorded ID.
        if let Some(table_id) = setup.tables.iter().flatten().next() {
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
            self.delete_removed_something = true;
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
