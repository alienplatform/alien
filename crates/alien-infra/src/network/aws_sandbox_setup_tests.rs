use super::*;
use crate::core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider};
use crate::import::ResourceImporter;
use alien_aws_clients::ec2::{MockEc2Api, RouteDestination};
use alien_core::import::{data::AwsNetworkImportData, ImportContext};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Cloud {
    subnets: Vec<Value>,
    tables: Vec<Value>,
    mutations: usize,
    /// Lookups by CIDR that do not see a just-created subnet yet.
    unlisted_lookups: usize,
    /// Route table lists after each create that do not see the new table yet.
    table_lag: usize,
    unlisted_table_lookups: usize,
    last_table: Option<String>,
    /// Route tables by the client token of the call that created them.
    table_tokens: Vec<(String, Value)>,
    /// Subnet deletes allowed before EC2 denies them; `None` allows all.
    subnet_deletes_allowed: Option<usize>,
    /// The CIDR of every `CreateSubnet` call, including refused ones.
    subnet_creates: Vec<String>,
    /// The latest subnet created, which lagging reads leave out.
    last_subnet: Option<String>,
    /// The VPC was deleted outside Alien.
    vpc_deleted: bool,
}

fn overlaps(a: &str, b: &str) -> bool {
    let (a, b) = (range(a).unwrap(), range(b).unwrap());
    a.0 <= b.1 && b.0 <= a.1
}

fn vpc_not_found() -> AlienError<CloudClientErrorData> {
    AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
        resource_type: "VPC".into(),
        resource_name: "vpc-existing".into(),
    })
}
fn parse<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).unwrap()
}
fn local() -> Value {
    json!({"destinationCidrBlock":"10.54.0.0/16","gatewayId":"local","state":"active"})
}
fn tags(specs: Option<Vec<TagSpecification>>) -> Value {
    json!({"item":specs.into_iter().flatten().flat_map(|spec|spec.tags).map(|tag|json!({"key":tag.key,"value":tag.value})).collect::<Vec<_>>()})
}
fn matches_filters(value: &Value, filters: Option<&Vec<Filter>>) -> bool {
    filters.into_iter().flatten().all(|filter| {
        let actual = match filter.name.as_str() {
            "vpc-id" => value["vpcId"].as_str(),
            "cidr-block" => value["cidrBlock"].as_str(),
            name if name.starts_with("tag:") => value["tagSet"]["item"]
                .as_array()
                .and_then(|tags| {
                    tags.iter()
                        .find(|tag| tag["key"] == name.trim_start_matches("tag:"))
                })
                .and_then(|tag| tag["value"].as_str()),
            _ => panic!("unexpected filter"),
        };
        actual.is_some_and(|actual| filter.values.iter().any(|expected| expected == actual))
    })
}
fn provider(cloud: &Arc<Mutex<Cloud>>) -> Arc<MockPlatformServiceProvider> {
    let mut ec2 = MockEc2Api::new();
    let c = cloud.clone();
    ec2.expect_describe_vpcs().returning(move |_| {
        if c.lock().unwrap().vpc_deleted {
            return Err(vpc_not_found());
        }
        Ok(parse(json!({"vpcSet":{"item":[{"vpcId":"vpc-existing","cidrBlock":"10.54.0.0/16","state":"available"}]}})))
    });
    let c = cloud.clone();
    ec2.expect_describe_subnets().returning(move |request| {
        let mut c = c.lock().unwrap();
        // A lagging list read leaves out the newest subnet; a read by ID sees it.
        let hidden = if request.subnet_ids.is_none() && c.unlisted_lookups > 0 {
            c.unlisted_lookups -= 1;
            c.last_subnet.clone()
        } else {
            None
        };
        let subnets: Vec<_> = c
            .subnets
            .iter()
            .filter(|subnet| hidden.as_ref().is_none_or(|id| subnet["subnetId"] != *id))
            .filter(|subnet| {
                matches_filters(subnet, request.filters.as_ref())
                    && request
                        .subnet_ids
                        .as_ref()
                        .is_none_or(|ids| ids.iter().any(|id| subnet["subnetId"] == *id))
            })
            .cloned()
            .collect();
        Ok(parse(json!({"subnetSet":{"item":subnets}})))
    });
    let c = cloud.clone();
    ec2.expect_create_subnet().returning(move|request|{
        let mut c=c.lock().unwrap();
        c.subnet_creates.push(request.cidr_block.clone());
        if c.vpc_deleted {
            return Err(vpc_not_found());
        }
        // EC2 never lets subnets of a VPC overlap, whatever its reads show.
        if c.subnets.iter().any(|subnet|overlaps(subnet["cidrBlock"].as_str().unwrap(), &request.cidr_block)) {
            return Err(AlienError::new(CloudClientErrorData::RemoteResourceConflict {
                message: "InvalidSubnet.Conflict".into(),
                resource_type: "Subnet".into(),
                resource_name: request.cidr_block,
            }));
        }
        c.mutations+=1;
        let subnet=json!({"subnetId":format!("subnet-dedicated-{}",c.mutations),"vpcId":request.vpc_id,"cidrBlock":request.cidr_block,"availabilityZone":request.availability_zone,"availabilityZoneId":request.availability_zone,"state":"available","tagSet":tags(request.tag_specifications)});
        c.last_subnet=subnet["subnetId"].as_str().map(str::to_string);
        c.subnets.push(subnet.clone());Ok(parse(json!({"subnet":subnet})))
    });
    let c = cloud.clone();
    ec2.expect_describe_route_tables()
        .returning(move |request| {
            let mut c = c.lock().unwrap();
            let hidden = if c.unlisted_table_lookups > 0 {
                c.unlisted_table_lookups -= 1;
                c.last_table.clone()
            } else {
                None
            };
            let tables: Vec<_> = c
                .tables
                .iter()
                .filter(|table| matches_filters(table, request.filters.as_ref()))
                .filter(|table| {
                    hidden
                        .as_ref()
                        .is_none_or(|id| table["routeTableId"] != *id)
                })
                .collect();
            Ok(parse(json!({"routeTableSet":{"item":tables}})))
        });
    let c = cloud.clone();
    ec2.expect_create_route_table().returning(move|request|{
        let mut c=c.lock().unwrap();
        // EC2 returns the table an earlier call with the same token created.
        if let Some(token) = &request.client_token {
            if let Some((_, table)) = c.table_tokens.iter().find(|(seen, _)| seen == token) {
                return Ok(parse(json!({"routeTable":table})));
            }
        }
        c.mutations+=1;
        let table=json!({"routeTableId":format!("rtb-dedicated-{}",c.mutations),"vpcId":request.vpc_id,"routeSet":{"item":[local()]},"associationSet":{"item":[]},"tagSet":tags(request.tag_specifications)});
        c.last_table=table["routeTableId"].as_str().map(str::to_string);c.unlisted_table_lookups=c.table_lag;
        if let Some(token) = request.client_token { c.table_tokens.push((token, table.clone())); }
        c.tables.push(table.clone());Ok(parse(json!({"routeTable":table})))
    });
    let c = cloud.clone();
    ec2.expect_create_route().returning(move|request|{
        let mut c=c.lock().unwrap();assert_eq!(request.nat_gateway_id.as_deref(),Some("nat-existing"));c.mutations+=1;
        let table=c.tables.iter_mut().find(|table|table["routeTableId"]==request.route_table_id).unwrap();
        table["routeSet"]["item"].as_array_mut().unwrap().push(json!({"destinationCidrBlock":request.destination_cidr_block,"natGatewayId":request.nat_gateway_id,"state":"active"}));Ok(())
    });
    let c = cloud.clone();
    ec2.expect_associate_route_table()
        .returning(move |request| {
            let mut c = c.lock().unwrap();
            // A subnet has at most one explicit association.
            if c.tables.iter().any(|table| {
                table["associationSet"]["item"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|association| association["subnetId"] == request.subnet_id)
            }) {
                return Err(AlienError::new(
                    CloudClientErrorData::RemoteResourceConflict {
                        message: "Resource.AlreadyAssociated".into(),
                        resource_type: "EC2 Resource".into(),
                        resource_name: request.subnet_id,
                    },
                ));
            }
            c.mutations += 1;
            let association = format!("rtbassoc-{}", c.mutations);
            let table = c
                .tables
                .iter_mut()
                .find(|table| table["routeTableId"] == request.route_table_id)
                .unwrap();
            table["associationSet"]["item"]
                .as_array_mut()
                .unwrap()
                .push(json!({"subnetId":request.subnet_id,"routeTableAssociationId":association}));
            Ok(parse(json!({"associationId":association})))
        });
    let c = cloud.clone();
    ec2.expect_replace_route().returning(move |request| {
        let mut c = c.lock().unwrap();
        c.mutations += 1;
        let table = c
            .tables
            .iter_mut()
            .find(|table| table["routeTableId"] == request.route_table_id)
            .unwrap();
        let route = table["routeSet"]["item"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|route| route["destinationCidrBlock"] == request.destination_cidr_block)
            .unwrap();
        *route = json!({"destinationCidrBlock":request.destination_cidr_block,"natGatewayId":request.nat_gateway_id,"state":"active"});
        Ok(())
    });
    let c = cloud.clone();
    ec2.expect_delete_route().returning(move |request| {
        let mut c = c.lock().unwrap();
        c.mutations += 1;
        let (key, value) = match request.destination {
            RouteDestination::Ipv4Cidr(cidr) => ("destinationCidrBlock", cidr),
            RouteDestination::Ipv6Cidr(cidr) => ("destinationIpv6CidrBlock", cidr),
            RouteDestination::PrefixList(id) => ("destinationPrefixListId", id),
        };
        let table = c
            .tables
            .iter_mut()
            .find(|table| table["routeTableId"] == request.route_table_id)
            .unwrap();
        let routes = table["routeSet"]["item"].as_array_mut().unwrap();
        assert!(
            routes.iter().any(|route| route[key] == value),
            "deleted a missing route"
        );
        routes.retain(|route| route[key] != value);
        Ok(())
    });
    let c = cloud.clone();
    ec2.expect_replace_route_table_association()
        .returning(move |request| {
            let mut c = c.lock().unwrap();
            c.mutations += 1;
            let mut moved = None;
            for table in &mut c.tables {
                let associations = table["associationSet"]["item"].as_array_mut().unwrap();
                if let Some(index) = associations.iter().position(|association| {
                    association["routeTableAssociationId"] == request.association_id
                }) {
                    moved = Some(associations.remove(index)["subnetId"].clone());
                }
            }
            let subnet = moved.expect("the replaced association exists");
            let association = format!("rtbassoc-{}", c.mutations);
            c.tables
                .iter_mut()
                .find(|table| table["routeTableId"] == request.route_table_id)
                .unwrap()["associationSet"]["item"]
                .as_array_mut()
                .unwrap()
                .push(json!({"subnetId":subnet,"routeTableAssociationId":association}));
            Ok(parse(json!({"newAssociationId":association})))
        });
    let c = cloud.clone();
    ec2.expect_delete_subnet().returning(move |id| {
        assert!(id.starts_with("subnet-dedicated-"));
        let mut c = c.lock().unwrap();
        if let Some(allowed) = c.subnet_deletes_allowed.as_mut() {
            if *allowed == 0 {
                return Err(AlienError::new(CloudClientErrorData::RemoteAccessDenied {
                    resource_type: "EC2 Resource".into(),
                    resource_name: id.into(),
                }));
            }
            *allowed -= 1;
        }
        c.mutations += 1;
        c.subnets.retain(|subnet| subnet["subnetId"] != id);
        for table in &mut c.tables {
            table["associationSet"]["item"]
                .as_array_mut()
                .unwrap()
                .retain(|association| association["subnetId"] != id);
        }
        Ok(())
    });
    let c = cloud.clone();
    ec2.expect_delete_route_table().returning(move |id| {
        assert!(id.starts_with("rtb-dedicated-"));
        let mut c = c.lock().unwrap();
        c.mutations += 1;
        c.tables.retain(|table| table["routeTableId"] != id);
        Ok(())
    });
    // A created VPC's teardown looks for security groups of lost creates; there are none.
    ec2.expect_describe_security_groups()
        .returning(|_| Ok(parse(json!({}))));
    let ec2 = Arc::new(ec2);
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_aws_ec2_client()
        .returning(move |_| Ok(ec2.clone()));
    Arc::new(provider)
}
fn gateway() -> alien_core::Container {
    alien_core::Container::new(alien_core::sandbox_egress::SANDBOX_EGRESS_GATEWAY_ID.into())
        .code(alien_core::ContainerCode::Image {
            image: "example.invalid/gateway:1".into(),
        })
        .cpu(alien_core::ResourceSpec {
            min: "0.25".into(),
            desired: "0.25".into(),
        })
        .memory(alien_core::ResourceSpec {
            min: "512Mi".into(),
            desired: "512Mi".into(),
        })
        .permissions("gateway".into())
        .build()
}

fn network() -> Network {
    Network::new("network".into())
        .settings(NetworkSettings::ByoVpcAws {
            vpc_id: "vpc-existing".into(),
            public_subnet_ids: vec![],
            private_subnet_ids: vec!["subnet-work-a".into(), "subnet-work-b".into()],
            security_group_ids: vec![],
        })
        .build()
}

async fn executor(
    cloud: &Arc<Mutex<Cloud>>,
    controller: AwsNetworkController,
    with_gateway: bool,
) -> SingleControllerExecutor {
    executor_with_authority(
        cloud,
        controller,
        with_gateway,
        InitialSetupAuthority::DirectSetup,
    )
    .await
}

async fn executor_with_authority(
    cloud: &Arc<Mutex<Cloud>>,
    controller: AwsNetworkController,
    with_gateway: bool,
    authority: InitialSetupAuthority,
) -> SingleControllerExecutor {
    let mut builder = SingleControllerExecutor::builder()
        .resource(network())
        .controller(controller)
        .platform(Platform::Aws)
        .initial_setup_authority(authority);
    if with_gateway {
        builder = builder.with_stack_resource(gateway(), alien_core::ResourceLifecycle::Live);
    }
    builder
        .resource_prefix("test")
        .resource_lifecycle(alien_core::ResourceLifecycle::Frozen)
        .service_provider(provider(cloud))
        .with_test_dependencies()
        .build()
        .await
        .unwrap()
}

fn tables_except_workload(c: &mut Cloud) -> impl Iterator<Item = &mut Value> {
    c.tables
        .iter_mut()
        .filter(|table| table["routeTableId"] != "rtb-work")
}

#[tokio::test]
async fn direct_setup_recovers_lost_checkpoints_and_deletes_only_its_byo_children() {
    let cloud = Arc::new(Mutex::new(Cloud::default()));
    {
        let mut c = cloud.lock().unwrap();
        for (zone, cidr) in [("a", "10.54.0.0/20"), ("b", "10.54.16.0/20")] {
            c.subnets.push(json!({"subnetId":format!("subnet-work-{zone}"),"vpcId":"vpc-existing","cidrBlock":cidr,"availabilityZone":format!("us-east-2{zone}"),"availabilityZoneId":format!("use2-az{zone}"),"state":"available"}));
        }
        c.tables.push(json!({"routeTableId":"rtb-work","vpcId":"vpc-existing","routeSet":{"item":[local(),{"destinationCidrBlock":"0.0.0.0/0","natGatewayId":"nat-existing","state":"active"}]},"associationSet":{"item":[{"subnetId":"subnet-work-a","routeTableAssociationId":"rtbassoc-work-a"},{"subnetId":"subnet-work-b","routeTableAssociationId":"rtbassoc-work-b"}]}}));
    }
    let original = cloud.lock().unwrap().tables[0].clone();
    let mut controller = AwsNetworkController {
        state: AwsNetworkState::CreatingSandboxNetwork,
        vpc_id: Some("vpc-existing".into()),
        private_subnet_ids: vec!["subnet-work-a".into(), "subnet-work-b".into()],
        is_byo_vpc: true,
        ..Default::default()
    };
    let mut lost_subnet = false;
    let mut lost_table = false;
    for step in 0..200 {
        let before = cloud.lock().unwrap().mutations;
        let checkpoint: AwsNetworkController = parse(serde_json::to_value(&controller).unwrap());
        let mut execution = executor(&cloud, controller, true).await;
        execution.step().await.unwrap();
        controller = parse(
            serde_json::to_value(execution.internal_state::<AwsNetworkController>().unwrap())
                .unwrap(),
        );
        let c = cloud.lock().unwrap();
        assert!(
            c.mutations - before <= 1,
            "more than one durable mutation in step {step}"
        );
        if !lost_subnet && c.subnets.len() > 2 {
            lost_subnet = true;
            controller = checkpoint;
            continue;
        }
        if !lost_table && c.tables.len() > 1 {
            lost_table = true;
            controller = checkpoint;
            continue;
        }
        if controller.state == AwsNetworkState::Ready {
            break;
        }
        assert!(step < 199, "setup did not converge");
    }
    assert!(lost_subnet && lost_table);
    let topology = controller.sandbox_network.as_ref().unwrap();
    assert_eq!(topology.sandbox_subnet_ids.len(), 2);
    assert_eq!(topology.gateway_subnet_ids.len(), 2);
    assert_eq!(cloud.lock().unwrap().tables[0], original);
    let created = cloud.lock().unwrap().tables.clone();

    // Ready refuses drift instead of publishing a widened network.
    {
        let mut c = cloud.lock().unwrap();
        for table in tables_except_workload(&mut c) {
            table["routeSet"]["item"]
                .as_array_mut()
                .unwrap()
                .retain(|route| route["destinationCidrBlock"] != "0.0.0.0/0");
            table["associationSet"]["item"]
                .as_array_mut()
                .unwrap()
                .truncate(1);
        }
    }
    // A verified network schedules nothing, so a setup run can finish.
    let healthy = executor(&cloud, controller.clone(), true).await;
    assert!(!healthy.needs_update().unwrap());
    let mut execution = executor(&cloud, controller, true).await;
    assert!(execution.step().await.is_err());
    // The failed check is recorded: the next setup run schedules a repair, a runtime one does not.
    assert!(execution.needs_update().unwrap());
    let unverified = execution
        .internal_state::<AwsNetworkController>()
        .unwrap()
        .clone();
    let runtime = SingleControllerExecutor::builder()
        .resource(network())
        .controller(unverified)
        .platform(Platform::Aws)
        .initial_setup_authority(InitialSetupAuthority::DirectSetup)
        .with_stack_resource(gateway(), alien_core::ResourceLifecycle::Live)
        .resource_prefix("test")
        .resource_lifecycle(alien_core::ResourceLifecycle::Frozen)
        .setup_run(false)
        .service_provider(provider(&cloud))
        .with_test_dependencies()
        .build()
        .await
        .unwrap();
    assert!(!runtime.needs_update().unwrap());

    // Running setup again repairs the missing NAT route and associations, one call per step.
    let mutations = cloud.lock().unwrap().mutations;
    execution.update(network()).unwrap();
    execution
        .run_until_status(ResourceStatus::Running)
        .await
        .unwrap();
    assert_eq!(cloud.lock().unwrap().mutations - mutations, 3);
    // Once repaired, setup has nothing left to schedule.
    assert!(!execution.needs_update().unwrap());
    let mut repaired = cloud.lock().unwrap().tables.clone();
    let mut expected = created.clone();
    for tables in [&mut repaired, &mut expected] {
        for table in tables.iter_mut() {
            let associations = table["associationSet"]["item"].as_array_mut().unwrap();
            for association in associations.iter_mut() {
                association
                    .as_object_mut()
                    .unwrap()
                    .remove("routeTableAssociationId");
            }
            associations.sort_by_key(|association| association.to_string());
        }
    }
    assert_eq!(repaired, expected);
    // A second run changes nothing.
    let controller = execution
        .internal_state::<AwsNetworkController>()
        .unwrap()
        .clone();
    let mutations = cloud.lock().unwrap().mutations;
    let mut execution = executor(&cloud, controller, true).await;
    execution.update(network()).unwrap();
    execution
        .run_until_status(ResourceStatus::Running)
        .await
        .unwrap();
    assert_eq!(cloud.lock().unwrap().mutations, mutations);

    let controller = execution
        .internal_state::<AwsNetworkController>()
        .unwrap()
        .clone();
    // Setup teardown deletes with setup's credentials but the default authority marker.
    let mut execution = executor_with_authority(
        &cloud,
        controller,
        true,
        InitialSetupAuthority::ImportedHandoff,
    )
    .await;
    execution.delete().unwrap();
    execution.run_until_terminal().await.unwrap();
    assert_eq!(
        execution
            .internal_state::<AwsNetworkController>()
            .unwrap()
            .state,
        AwsNetworkState::Deleted
    );
    let c = cloud.lock().unwrap();
    assert_eq!(c.subnets.len(), 2);
    assert_eq!(c.tables, vec![original]);
}

#[tokio::test]
async fn only_a_declared_gateway_schedules_direct_network_setup() {
    let cloud = Arc::new(Mutex::new(Cloud::default()));
    let ready = || AwsNetworkController {
        state: AwsNetworkState::Ready,
        vpc_id: Some("vpc-existing".into()),
        private_subnet_ids: vec!["subnet-work-a".into(), "subnet-work-b".into()],
        is_byo_vpc: true,
        ..Default::default()
    };
    // A legacy allow/deny deployment has no gateway and upgrades without any network change.
    assert!(!executor(&cloud, ready(), false)
        .await
        .needs_update()
        .unwrap());
    assert!(executor(&cloud, ready(), true)
        .await
        .needs_update()
        .unwrap());
    assert_eq!(cloud.lock().unwrap().mutations, 0);
}

#[tokio::test]
async fn teardown_keeps_looking_for_a_subnet_whose_create_response_was_lost() {
    let cloud = Arc::new(Mutex::new(Cloud::default()));
    {
        let mut c = cloud.lock().unwrap();
        for (zone, cidr) in [("a", "10.54.0.0/20"), ("b", "10.54.16.0/20")] {
            c.subnets.push(json!({"subnetId":format!("subnet-work-{zone}"),"vpcId":"vpc-existing","cidrBlock":cidr,"availabilityZone":format!("us-east-2{zone}"),"availabilityZoneId":format!("use2-az{zone}"),"state":"available"}));
        }
        c.tables.push(json!({"routeTableId":"rtb-work","vpcId":"vpc-existing","routeSet":{"item":[local(),{"destinationCidrBlock":"0.0.0.0/0","natGatewayId":"nat-existing","state":"active"}]},"associationSet":{"item":[{"subnetId":"subnet-work-a","routeTableAssociationId":"rtbassoc-work-a"},{"subnetId":"subnet-work-b","routeTableAssociationId":"rtbassoc-work-b"}]}}));
    }
    let mut controller = AwsNetworkController {
        state: AwsNetworkState::CreatingSandboxNetwork,
        vpc_id: Some("vpc-existing".into()),
        private_subnet_ids: vec!["subnet-work-a".into(), "subnet-work-b".into()],
        is_byo_vpc: true,
        ..Default::default()
    };
    // Step until the first dedicated subnet exists, then lose that step's checkpoint.
    let checkpoint = loop {
        let checkpoint: AwsNetworkController = parse(serde_json::to_value(&controller).unwrap());
        let mut execution = executor(&cloud, controller, true).await;
        execution.step().await.unwrap();
        controller = execution
            .internal_state::<AwsNetworkController>()
            .unwrap()
            .clone();
        if cloud.lock().unwrap().subnets.len() == 3 {
            break checkpoint;
        }
    };
    assert!(checkpoint.subnet_create_attempt.is_some());
    // EC2 does not list the new subnet for the first few lookups.
    cloud.lock().unwrap().unlisted_lookups = 3;
    let mut execution = executor_with_authority(
        &cloud,
        checkpoint,
        true,
        InitialSetupAuthority::ImportedHandoff,
    )
    .await;
    execution.delete().unwrap();
    execution.run_until_terminal().await.unwrap();
    let c = cloud.lock().unwrap();
    assert_eq!(c.unlisted_lookups, 0);
    assert_eq!(c.subnets.len(), 2, "the dedicated subnet was left behind");
}

/// A VPC with two workload subnets that route through one NAT gateway.
fn workload_cloud() -> Arc<Mutex<Cloud>> {
    let cloud = Arc::new(Mutex::new(Cloud::default()));
    {
        let mut c = cloud.lock().unwrap();
        for (zone, cidr) in [("a", "10.54.0.0/20"), ("b", "10.54.16.0/20")] {
            c.subnets.push(json!({"subnetId":format!("subnet-work-{zone}"),"vpcId":"vpc-existing","cidrBlock":cidr,"availabilityZone":format!("us-east-2{zone}"),"availabilityZoneId":format!("use2-az{zone}"),"state":"available"}));
        }
        c.tables.push(json!({"routeTableId":"rtb-work","vpcId":"vpc-existing","routeSet":{"item":[local(),{"destinationCidrBlock":"0.0.0.0/0","natGatewayId":"nat-existing","state":"active"}]},"associationSet":{"item":[{"subnetId":"subnet-work-a","routeTableAssociationId":"rtbassoc-work-a"},{"subnetId":"subnet-work-b","routeTableAssociationId":"rtbassoc-work-b"}]}}));
    }
    cloud
}

fn creating_sandbox_network() -> AwsNetworkController {
    AwsNetworkController {
        state: AwsNetworkState::CreatingSandboxNetwork,
        vpc_id: Some("vpc-existing".into()),
        private_subnet_ids: vec!["subnet-work-a".into(), "subnet-work-b".into()],
        is_byo_vpc: true,
        ..Default::default()
    }
}

/// EC2 lists a just-created route table only a few reads later. The first table's create
/// response is lost and the lists after it miss the table: the repeated create returns that same
/// table instead of a second one, and a recorded table missing from a list is looked for again
/// instead of failing setup.
#[tokio::test]
async fn route_table_list_lag_after_a_lost_create_response_creates_no_duplicate() {
    let cloud = workload_cloud();
    cloud.lock().unwrap().table_lag = 3;
    let mut controller = creating_sandbox_network();
    let mut lost = false;
    for step in 0..200 {
        let tables = cloud.lock().unwrap().tables.len();
        let checkpoint = controller.clone();
        let mut execution = executor(&cloud, controller, true).await;
        execution.step().await.unwrap();
        controller = execution
            .internal_state::<AwsNetworkController>()
            .unwrap()
            .clone();
        if !lost && cloud.lock().unwrap().tables.len() > tables {
            lost = true;
            controller = checkpoint;
            continue;
        }
        if controller.state == AwsNetworkState::Ready {
            break;
        }
        assert!(step < 199, "setup did not converge");
    }
    assert!(lost);
    assert_eq!(
        cloud.lock().unwrap().tables.len(),
        3,
        "the workload table and exactly two dedicated tables"
    );
}

/// A failed direct create is replaced after a config change, and its replace delete removes the
/// dedicated subnets one per step. When a later delete is denied, the failed create must not be
/// put back with the ID of a subnet already deleted: reverting the config would resume setup
/// against a subnet that no longer exists.
#[tokio::test]
async fn denied_replace_after_a_dedicated_subnet_was_deleted_keeps_no_deleted_subnet() {
    use crate::core::state_utils::StackResourceStateExt;
    use crate::core::ResourceController;
    use crate::StackExecutor;
    use alien_aws_clients::AwsClientConfigExt;
    use alien_core::{
        ClientConfig, Resource, Stack, StackResourceState, StackSettings, StackState,
    };

    let cloud = workload_cloud();
    let mut controller = creating_sandbox_network();
    // The create made two dedicated subnets, then failed.
    while cloud.lock().unwrap().subnets.len() < 4 {
        let mut execution = executor(&cloud, controller, true).await;
        execution.step().await.unwrap();
        controller = execution
            .internal_state::<AwsNetworkController>()
            .unwrap()
            .clone();
    }
    let checkpoint = controller.clone();
    controller.transition_to_failure();
    let created = Resource::new(network());
    let mut failed = StackResourceState::new_pending(
        created.resource_type().to_string(),
        created,
        Some(alien_core::ResourceLifecycle::Frozen),
        vec![],
    );
    failed.status = ResourceStatus::ProvisionFailed;
    failed
        .set_internal_controller(Some(Box::new(controller)))
        .unwrap();
    failed
        .set_last_failed_controller(Some(Box::new(checkpoint)))
        .unwrap();
    let mut state = StackState::new(Platform::Aws);
    state.resources.insert("network".into(), failed);

    // The first subnet delete succeeds, the second is denied.
    cloud.lock().unwrap().subnet_deletes_allowed = Some(1);
    let changed = Network::new("network".into())
        .settings(NetworkSettings::ByoVpcAws {
            vpc_id: "vpc-existing".into(),
            public_subnet_ids: vec![],
            private_subnet_ids: vec!["subnet-work-a".into(), "subnet-work-b".into()],
            security_group_ids: vec!["sg-changed".into()],
        })
        .build();
    let stack = Stack::new("test".into())
        .add(changed, alien_core::ResourceLifecycle::Frozen)
        .add(gateway(), alien_core::ResourceLifecycle::Live)
        .build();
    let config = alien_core::DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(alien_core::EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(alien_core::ExternalBindings::default())
        .allow_frozen_changes(true)
        .build();
    let executor = StackExecutor::builder(
        &stack,
        ClientConfig::Aws(Box::new(alien_aws_clients::AwsClientConfig::mock())),
    )
    .deployment_config(&config)
    .initial_setup_authority(InitialSetupAuthority::DirectSetup)
    .lifecycle_filter(vec![alien_core::ResourceLifecycle::Frozen])
    .service_provider(provider(&cloud))
    .build()
    .unwrap();
    assert_eq!(
        executor.plan(&state).unwrap().replaces,
        vec!["network".to_string()]
    );
    for _ in 0..20 {
        if state.resources["network"]
            .error
            .as_ref()
            .is_some_and(|error| error.code == "REPLACE_DELETE_DENIED")
        {
            break;
        }
        state = executor.step(state).await.unwrap().next_state;
    }

    let network = &state.resources["network"];
    let error = network.error.as_ref().expect("the denial is recorded");
    assert_eq!(error.code, "REPLACE_DELETE_DENIED");
    assert!(
        error.message.contains("Part of it may already be deleted"),
        "{}",
        error.message
    );
    let kept = network
        .get_internal_controller_typed::<AwsNetworkController>()
        .unwrap();
    let c = cloud.lock().unwrap();
    assert_eq!(c.subnets.len(), 3, "one dedicated subnet was deleted");
    for subnet_id in kept
        .sandbox_setup
        .iter()
        .flat_map(|setup| &setup.subnets)
        .filter_map(|subnet| subnet.id.as_ref())
    {
        assert!(
            c.subnets
                .iter()
                .any(|subnet| subnet["subnetId"] == *subnet_id),
            "deleted subnet '{subnet_id}' is still recorded"
        );
    }
}

/// A recorded gateway route table that never shows up again fails setup once its lookups are
/// used up, even though the isolated table, found first on every pass, is present.
#[tokio::test]
async fn a_missing_recorded_gateway_table_fails_after_its_lookups() {
    let cloud = workload_cloud();
    let mut controller = creating_sandbox_network();
    for step in 0..200 {
        let mut execution = executor(&cloud, controller, true).await;
        execution.step().await.unwrap();
        controller = execution
            .internal_state::<AwsNetworkController>()
            .unwrap()
            .clone();
        if controller.state == AwsNetworkState::Ready {
            break;
        }
        assert!(step < 199, "setup did not converge");
    }
    cloud.lock().unwrap().tables.retain(|table| {
        !table["tagSet"]["item"].as_array().is_some_and(|tags| {
            tags.iter()
                .any(|tag| tag["value"] == "test-network-sandbox-gateway-rt")
        })
    });
    assert_eq!(cloud.lock().unwrap().tables.len(), 2);

    let mut execution = executor(&cloud, controller, true).await;
    execution.update(network()).unwrap();
    let mut failure = None;
    for _ in 0..=LOST_CREATE_LOOKUPS + 2 {
        if let Err(error) = execution.step().await {
            failure = Some(error);
            break;
        }
    }
    let failure = failure.expect("setup kept waiting for the missing gateway table");
    assert!(
        failure
            .to_string()
            .contains("Recorded dedicated route table was not returned"),
        "{failure}"
    );
}

/// Steps direct setup from `controller` until the network is Ready.
async fn set_up(
    cloud: &Arc<Mutex<Cloud>>,
    mut controller: AwsNetworkController,
) -> AwsNetworkController {
    for step in 0..200 {
        let mut execution = executor(cloud, controller, true).await;
        execution.step().await.unwrap();
        controller = execution
            .internal_state::<AwsNetworkController>()
            .unwrap()
            .clone();
        if controller.state == AwsNetworkState::Ready {
            return controller;
        }
        assert!(step < 199, "setup did not converge");
    }
    unreachable!()
}

fn table_named<'a>(c: &'a mut Cloud, name: &str) -> &'a mut Value {
    c.tables
        .iter_mut()
        .find(|table| {
            table["tagSet"]["item"]
                .as_array()
                .is_some_and(|tags| tags.iter().any(|tag| tag["value"] == name))
        })
        .unwrap()
}

/// Tables with association IDs removed and associations sorted, for comparing topologies.
fn topology(tables: &[Value]) -> Vec<Value> {
    let mut tables = tables.to_vec();
    for table in &mut tables {
        let associations = table["associationSet"]["item"].as_array_mut().unwrap();
        for association in associations.iter_mut() {
            association
                .as_object_mut()
                .unwrap()
                .remove("routeTableAssociationId");
        }
        associations.sort_by_key(|association| association.to_string());
    }
    tables
}

/// CreateSubnet takes no idempotency token. After a lost create response the step that creates
/// starts from lookups already used up, so it repeats the create while EC2 still lists nothing.
/// EC2 refuses the repeat because the first subnet holds the CIDR; setup must keep looking for
/// that subnet instead of failing.
#[tokio::test]
async fn subnet_list_lag_after_a_lost_create_response_creates_no_second_subnet() {
    let cloud = workload_cloud();
    let mut controller = creating_sandbox_network();
    let mut lost = None;
    for step in 0..200 {
        let subnets = cloud.lock().unwrap().subnets.len();
        let checkpoint = controller.clone();
        let mut execution = executor(&cloud, controller, true).await;
        execution.step().await.unwrap();
        controller = execution
            .internal_state::<AwsNetworkController>()
            .unwrap()
            .clone();
        let mut c = cloud.lock().unwrap();
        if lost.is_none() && c.subnets.len() > subnets {
            lost = c.subnet_creates.last().cloned();
            // The step's checkpoint is lost and the next two reads miss the new subnet.
            c.unlisted_lookups = 2;
            controller = checkpoint;
            continue;
        }
        if controller.state == AwsNetworkState::Ready {
            break;
        }
        assert!(step < 199, "setup did not converge");
    }
    let lost = lost.expect("a create response was lost");
    let c = cloud.lock().unwrap();
    assert_eq!(c.unlisted_lookups, 0);
    assert_eq!(
        c.subnets.len(),
        6,
        "two workload and four dedicated subnets"
    );
    assert_eq!(
        c.subnet_creates
            .iter()
            .filter(|cidr| **cidr == lost)
            .count(),
        2,
        "the accepted create and the one EC2 refused"
    );
    let topology = controller.sandbox_network.unwrap();
    assert_eq!(
        topology.sandbox_subnet_ids.len() + topology.gateway_subnet_ids.len(),
        4
    );
}

/// A setup run repairs drift on direct setup's own route tables, not only missing pieces: a
/// gateway default route that is no longer an active route to the workload NAT (here the
/// workload moved to a new NAT and the old one left a blackhole), extra routes on the connector
/// table, and a connector subnet associated with another route table. The customer's workload
/// table keeps its routes.
#[tokio::test]
async fn setup_repairs_wrong_routes_and_associations_on_its_route_tables() {
    let cloud = workload_cloud();
    let controller = set_up(&cloud, creating_sandbox_network()).await;
    let connector = controller
        .sandbox_network
        .as_ref()
        .unwrap()
        .sandbox_subnet_ids[0]
        .clone();
    {
        let mut c = cloud.lock().unwrap();
        let workload = c
            .tables
            .iter_mut()
            .find(|t| t["routeTableId"] == "rtb-work")
            .unwrap();
        workload["routeSet"]["item"][1]["natGatewayId"] = json!("nat-new");
        let gateway = table_named(&mut c, "test-network-sandbox-gateway-rt");
        let default = gateway["routeSet"]["item"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|route| route["destinationCidrBlock"] == "0.0.0.0/0")
            .unwrap();
        default["state"] = json!("blackhole");
        let isolated = table_named(&mut c, "test-network-sandbox-isolated-rt");
        let routes = isolated["routeSet"]["item"].as_array_mut().unwrap();
        routes.push(json!({"destinationCidrBlock":"10.99.0.0/16","vpcPeeringConnectionId":"pcx-1","state":"active","origin":"CreateRoute"}));
        routes.push(json!({"destinationIpv6CidrBlock":"::/0","gatewayId":"igw-1","state":"active","origin":"CreateRoute"}));
        let associations = isolated["associationSet"]["item"].as_array_mut().unwrap();
        let index = associations
            .iter()
            .position(|association| association["subnetId"] == connector)
            .unwrap();
        let moved = associations.remove(index);
        c.tables
            .iter_mut()
            .find(|t| t["routeTableId"] == "rtb-work")
            .unwrap()["associationSet"]["item"]
            .as_array_mut()
            .unwrap()
            .push(moved);
    }
    let drifted = topology(&cloud.lock().unwrap().tables);

    // The refresh refuses the drift, which schedules the repair for the next setup run.
    let mut execution = executor(&cloud, controller, true).await;
    assert!(execution.step().await.is_err());
    assert!(execution.needs_update().unwrap());
    let mutations = cloud.lock().unwrap().mutations;
    execution.update(network()).unwrap();
    execution
        .run_until_status(ResourceStatus::Running)
        .await
        .unwrap();
    // Two route deletes, one route replace and one association replace.
    assert_eq!(cloud.lock().unwrap().mutations - mutations, 4);
    assert!(!execution.needs_update().unwrap());

    let c = cloud.lock().unwrap();
    let mut expected = drifted;
    for table in &mut expected {
        let name = table["tagSet"]["item"]
            .as_array()
            .and_then(|tags| tags.iter().find(|tag| tag["key"] == "Name"))
            .map(|tag| tag["value"].clone());
        if table["routeTableId"] == "rtb-work" {
            table["associationSet"]["item"]
                .as_array_mut()
                .unwrap()
                .retain(|association| association["subnetId"] != connector);
        } else if name == Some(json!("test-network-sandbox-gateway-rt")) {
            for route in table["routeSet"]["item"].as_array_mut().unwrap() {
                if route["destinationCidrBlock"] == "0.0.0.0/0" {
                    *route = json!({"destinationCidrBlock":"0.0.0.0/0","natGatewayId":"nat-new","state":"active"});
                }
            }
        } else {
            table["routeSet"]["item"] = json!([local()]);
            let associations = table["associationSet"]["item"].as_array_mut().unwrap();
            associations.push(json!({"subnetId":connector}));
            associations.sort_by_key(|association| association.to_string());
        }
    }
    assert_eq!(topology(&c.tables), expected);
}

/// A run without direct setup's authority never changes direct setup's subnets, but it does not
/// refuse a network whose topology is complete: it verifies it read-only and carries on. A
/// broken topology fails that check and is left for the next direct setup run to repair.
#[tokio::test]
async fn a_run_without_setup_authority_verifies_a_complete_topology_read_only() {
    let cloud = workload_cloud();
    let controller = set_up(&cloud, creating_sandbox_network()).await;
    let mutations = cloud.lock().unwrap().mutations;

    let mut execution = executor_with_authority(
        &cloud,
        controller.clone(),
        true,
        InitialSetupAuthority::ImportedHandoff,
    )
    .await;
    execution.update(network()).unwrap();
    execution
        .run_until_status(ResourceStatus::Running)
        .await
        .expect("a complete topology needs no setup authority");
    assert_eq!(cloud.lock().unwrap().mutations, mutations);

    {
        let mut c = cloud.lock().unwrap();
        table_named(&mut c, "test-network-sandbox-gateway-rt")["routeSet"]["item"]
            .as_array_mut()
            .unwrap()
            .retain(|route| route["destinationCidrBlock"] != "0.0.0.0/0");
    }
    let mut execution = executor_with_authority(
        &cloud,
        controller,
        true,
        InitialSetupAuthority::ImportedHandoff,
    )
    .await;
    execution.update(network()).unwrap();
    let error = execution
        .run_until_status(ResourceStatus::Running)
        .await
        .expect_err("a broken topology is refused");
    assert_eq!(cloud.lock().unwrap().mutations, mutations);
    let failed = execution.internal_state::<AwsNetworkController>().unwrap();
    assert!(
        failed.sandbox_setup.as_ref().unwrap().is_unverified(),
        "{error}"
    );
}

/// The checkpoint saved just before the step whose create response is lost.
async fn checkpoint_before_first_dedicated_subnet(
    cloud: &Arc<Mutex<Cloud>>,
) -> AwsNetworkController {
    let mut controller = creating_sandbox_network();
    loop {
        let checkpoint = controller.clone();
        let mut execution = executor(cloud, controller, true).await;
        execution.step().await.unwrap();
        controller = execution
            .internal_state::<AwsNetworkController>()
            .unwrap()
            .clone();
        if cloud.lock().unwrap().subnets.len() == 3 {
            assert!(checkpoint.subnet_create_attempt.is_some());
            return checkpoint;
        }
    }
}

/// A subnet that EC2 lists only after many reads is still found and deleted: teardown never
/// concludes from empty reads that a lost create made nothing.
#[tokio::test]
async fn teardown_deletes_a_subnet_listed_only_after_many_reads() {
    let cloud = workload_cloud();
    let checkpoint = checkpoint_before_first_dedicated_subnet(&cloud).await;
    cloud.lock().unwrap().unlisted_lookups = 14;
    let mut execution = executor_with_authority(
        &cloud,
        checkpoint,
        true,
        InitialSetupAuthority::ImportedHandoff,
    )
    .await;
    execution.delete().unwrap();
    execution.run_until_terminal().await.unwrap();
    let state = execution.internal_state::<AwsNetworkController>().unwrap();
    assert_eq!(state.state, AwsNetworkState::Deleted);
    let c = cloud.lock().unwrap();
    assert_eq!(c.unlisted_lookups, 0);
    assert_eq!(c.subnets.len(), 2, "the dedicated subnet was left behind");
}

/// While no read shows the subnet a lost create made, teardown fails with the attempt still
/// recorded instead of reporting success, and keeps looking once retried.
#[tokio::test]
async fn teardown_keeps_a_lost_create_until_its_subnet_is_found() {
    let cloud = workload_cloud();
    let checkpoint = checkpoint_before_first_dedicated_subnet(&cloud).await;
    cloud.lock().unwrap().unlisted_lookups = usize::MAX;
    let mut execution = executor_with_authority(
        &cloud,
        checkpoint,
        true,
        InitialSetupAuthority::ImportedHandoff,
    )
    .await;
    execution.delete().unwrap();
    let error = loop {
        if let Err(error) = execution.step().await {
            break error;
        }
    };
    assert!(
        error
            .to_string()
            .contains("no read shows the subnet holding it"),
        "{error}"
    );
    let state = execution.internal_state::<AwsNetworkController>().unwrap();
    assert!(state.subnet_create_attempt.is_some(), "the attempt is kept");
    assert!(state.sandbox_setup.is_some());
    {
        let c = cloud.lock().unwrap();
        assert_eq!(c.subnets.len(), 3);
        assert_eq!(
            c.subnet_creates.len(),
            2,
            "one accepted create and one refused repeat"
        );
    }

    // Reads show the subnet again; the retried teardown deletes it.
    cloud.lock().unwrap().unlisted_lookups = 0;
    execution.run_until_terminal().await.unwrap();
    assert_eq!(
        execution
            .internal_state::<AwsNetworkController>()
            .unwrap()
            .state,
        AwsNetworkState::Deleted
    );
    let c = cloud.lock().unwrap();
    assert_eq!(c.subnets.len(), 2);
    assert_eq!(
        c.subnet_creates.len(),
        2,
        "teardown never creates once the CIDR is taken"
    );
}

/// A VPC that direct setup created: the dedicated /24s avoid the network's own public and
/// private /20s, the gateway subnets route through the network's NAT, and teardown removes the
/// dedicated subnets and tables before it moves on to the VPC's own objects.
#[tokio::test]
async fn direct_setup_adds_dedicated_subnets_to_a_created_vpc() {
    let cloud = Arc::new(Mutex::new(Cloud::default()));
    {
        let mut c = cloud.lock().unwrap();
        for (name, zone, cidr) in [
            ("public-a", "a", "10.54.0.0/20"),
            ("public-b", "b", "10.54.16.0/20"),
            ("work-a", "a", "10.54.128.0/20"),
            ("work-b", "b", "10.54.144.0/20"),
        ] {
            c.subnets.push(json!({"subnetId":format!("subnet-{name}"),"vpcId":"vpc-existing","cidrBlock":cidr,"availabilityZone":format!("us-east-2{zone}"),"availabilityZoneId":format!("use2-az{zone}"),"state":"available"}));
        }
        c.tables.push(json!({"routeTableId":"rtb-public","vpcId":"vpc-existing","routeSet":{"item":[local(),{"destinationCidrBlock":"0.0.0.0/0","gatewayId":"igw-created","state":"active"}]},"associationSet":{"item":[{"subnetId":"subnet-public-a","routeTableAssociationId":"rtbassoc-pa"},{"subnetId":"subnet-public-b","routeTableAssociationId":"rtbassoc-pb"}]}}));
        c.tables.push(json!({"routeTableId":"rtb-work","vpcId":"vpc-existing","routeSet":{"item":[local(),{"destinationCidrBlock":"0.0.0.0/0","natGatewayId":"nat-existing","state":"active"}]},"associationSet":{"item":[{"subnetId":"subnet-work-a","routeTableAssociationId":"rtbassoc-wa"},{"subnetId":"subnet-work-b","routeTableAssociationId":"rtbassoc-wb"}]}}));
    }
    let created_network = || {
        Network::new("network".into())
            .settings(NetworkSettings::Create {
                cidr: Some("10.54.0.0/16".into()),
                availability_zones: 2,
            })
            .build()
    };
    let builder = |controller: AwsNetworkController| {
        SingleControllerExecutor::builder()
            .resource(created_network())
            .controller(controller)
            .platform(Platform::Aws)
            .initial_setup_authority(InitialSetupAuthority::DirectSetup)
            .with_stack_resource(gateway(), alien_core::ResourceLifecycle::Live)
            .resource_prefix("test")
            .resource_lifecycle(alien_core::ResourceLifecycle::Frozen)
            .service_provider(provider(&cloud))
            .with_test_dependencies()
    };
    let mut controller = AwsNetworkController {
        state: AwsNetworkState::CreatingSandboxNetwork,
        vpc_id: Some("vpc-existing".into()),
        cidr_block: Some("10.54.0.0/16".into()),
        public_subnet_ids: vec!["subnet-public-a".into(), "subnet-public-b".into()],
        private_subnet_ids: vec!["subnet-work-a".into(), "subnet-work-b".into()],
        public_route_table_id: Some("rtb-public".into()),
        private_route_table_id: Some("rtb-work".into()),
        is_byo_vpc: false,
        ..Default::default()
    };
    for step in 0..200 {
        let mut execution = builder(controller).build().await.unwrap();
        execution.step().await.unwrap();
        controller = execution
            .internal_state::<AwsNetworkController>()
            .unwrap()
            .clone();
        if controller.state == AwsNetworkState::Ready {
            break;
        }
        assert!(step < 199, "setup did not converge");
    }
    let topology = controller.sandbox_network.clone().unwrap();
    let mut dedicated = topology.sandbox_cidrs.clone();
    dedicated.extend(topology.gateway_cidrs.clone());
    dedicated.sort();
    assert_eq!(
        dedicated,
        [
            "10.54.32.0/24",
            "10.54.33.0/24",
            "10.54.34.0/24",
            "10.54.35.0/24"
        ]
    );
    assert_eq!(
        topology.workload_cidrs,
        ["10.54.128.0/20", "10.54.144.0/20"]
    );
    assert_eq!(cloud.lock().unwrap().tables.len(), 4);

    let mut execution = builder(controller).build().await.unwrap();
    execution.delete().unwrap();
    while execution
        .internal_state::<AwsNetworkController>()
        .unwrap()
        .state
        == AwsNetworkState::DeleteStart
    {
        execution.step().await.unwrap();
    }
    let state = execution.internal_state::<AwsNetworkController>().unwrap();
    assert!(state.sandbox_setup.is_none() && state.sandbox_network.is_none());
    assert_ne!(
        state.state,
        AwsNetworkState::Deleted,
        "the created VPC is deleted next"
    );
    let c = cloud.lock().unwrap();
    assert_eq!(c.subnets.len(), 4);
    assert_eq!(c.tables.len(), 2);
}

/// Sandbox and gateway subnets a customer made: route-less connector subnets in one table, NAT
/// gateway subnets in another.
fn customer_sandbox_subnets(c: &mut Cloud) {
    for (name, zone, cidr) in [
        ("sandbox-a", "a", "10.54.200.0/24"),
        ("sandbox-b", "b", "10.54.201.0/24"),
        ("gateway-a", "a", "10.54.210.0/24"),
        ("gateway-b", "b", "10.54.211.0/24"),
    ] {
        c.subnets.push(json!({"subnetId":format!("subnet-{name}"),"vpcId":"vpc-existing","cidrBlock":cidr,"availabilityZone":format!("us-east-2{zone}"),"availabilityZoneId":format!("use2-az{zone}"),"state":"available"}));
    }
    c.tables.push(json!({"routeTableId":"rtb-customer-isolated","vpcId":"vpc-existing","routeSet":{"item":[local()]},"associationSet":{"item":[{"subnetId":"subnet-sandbox-a","routeTableAssociationId":"rtbassoc-sa"},{"subnetId":"subnet-sandbox-b","routeTableAssociationId":"rtbassoc-sb"}]}}));
    c.tables.push(json!({"routeTableId":"rtb-customer-gateway","vpcId":"vpc-existing","routeSet":{"item":[local(),{"destinationCidrBlock":"0.0.0.0/0","natGatewayId":"nat-existing","state":"active"}]},"associationSet":{"item":[{"subnetId":"subnet-gateway-a","routeTableAssociationId":"rtbassoc-ga"},{"subnetId":"subnet-gateway-b","routeTableAssociationId":"rtbassoc-gb"}]}}));
}

/// A CloudFormation import hands over the customer's sandbox subnets. An update of that network
/// verifies them and changes nothing; a broken route fails the update without touching them.
#[tokio::test]
async fn an_imported_cloudformation_network_is_updated_without_changing_its_subnets() {
    let cloud = workload_cloud();
    customer_sandbox_subnets(&mut cloud.lock().unwrap());
    let settings = alien_core::StackSettings {
        network: Some(network().settings),
        ..Default::default()
    };
    let entry = alien_core::ResourceEntry {
        config: alien_core::Resource::new(network()),
        lifecycle: alien_core::ResourceLifecycle::Frozen,
        dependencies: Vec::new(),
        remote_access: false,
        enabled_when: None,
    };
    let data: AwsNetworkImportData = parse(json!({
        "vpcId": "vpc-existing",
        "cidrBlock": null,
        "internetGatewayId": null,
        "natGatewayId": null,
        "eipAllocationId": null,
        "publicSubnetIds": [],
        "privateSubnetIds": ["subnet-work-a", "subnet-work-b"],
        "publicRouteTableId": null,
        "privateRouteTableId": null,
        "availabilityZones": ["us-east-2a", "us-east-2b"],
        "subnetsByFailureDomain": {
            "us-east-2a": {"privateSubnetIds": ["subnet-work-a"]},
            "us-east-2b": {"privateSubnetIds": ["subnet-work-b"]}
        },
        "isByoVpc": true,
        "sandboxNetwork": {
            "sandboxSubnetIds": "subnet-sandbox-a,subnet-sandbox-b",
            "gatewaySubnetIds": "subnet-gateway-a,subnet-gateway-b",
            "sandboxCidrs": [],
            "gatewayCidrs": "10.54.210.0/24,10.54.211.0/24",
            "workloadCidrs": [],
            "vpcCidrs": []
        }
    }));
    let imported = crate::AwsNetworkImporter
        .import(
            data,
            &ImportContext {
                resource_id: "network",
                platform: Platform::Aws,
                region: "us-east-2",
                stack_settings: &settings,
                management_config: None,
                resource: &entry,
            },
        )
        .unwrap();
    let controller: AwsNetworkController = parse(imported.internal_state.unwrap());
    assert_eq!(controller.state, AwsNetworkState::Ready);
    assert!(controller.sandbox_setup.is_none());
    let before = cloud.lock().unwrap().tables.clone();

    let mut execution = executor_with_authority(
        &cloud,
        controller.clone(),
        true,
        InitialSetupAuthority::ImportedHandoff,
    )
    .await;
    execution.update(network()).unwrap();
    execution
        .run_until_status(ResourceStatus::Running)
        .await
        .unwrap();
    let updated = execution.internal_state::<AwsNetworkController>().unwrap();
    let verified = updated.sandbox_network.as_ref().unwrap();
    assert_eq!(verified.sandbox_cidrs, ["10.54.200.0/24", "10.54.201.0/24"]);
    assert_eq!(verified.workload_cidrs, ["10.54.0.0/20", "10.54.16.0/20"]);
    assert!(updated.sandbox_setup.is_none());
    {
        let c = cloud.lock().unwrap();
        assert_eq!(c.mutations, 0);
        assert_eq!(c.tables, before);
    }

    // The customer's gateway subnets lose their NAT route: the update fails and changes nothing.
    cloud
        .lock()
        .unwrap()
        .tables
        .iter_mut()
        .find(|table| table["routeTableId"] == "rtb-customer-gateway")
        .unwrap()["routeSet"]["item"]
        .as_array_mut()
        .unwrap()
        .truncate(1);
    let mut execution = executor_with_authority(
        &cloud,
        controller,
        true,
        InitialSetupAuthority::ImportedHandoff,
    )
    .await;
    execution.update(network()).unwrap();
    let error = execution
        .run_until_status(ResourceStatus::Running)
        .await
        .expect_err("the broken gateway route is refused");
    assert!(
        format!("{error:?}").contains("exactly one active 0.0.0.0/0 route"),
        "{error:?}"
    );
    assert_eq!(cloud.lock().unwrap().mutations, 0);
}

/// EC2 refuses a subnet whose CIDR overlaps another subnet's, not only an equal one. When a
/// customer subnet overlapping the allocation took the range before the create, teardown
/// settles the lost create against it: the attempt made nothing, and the customer's subnet stays.
#[tokio::test]
async fn teardown_settles_a_lost_create_against_an_overlapping_subnet() {
    let cloud = workload_cloud();
    let checkpoint = checkpoint_before_first_dedicated_subnet(&cloud).await;
    let attempt = checkpoint.subnet_create_attempt.clone().unwrap();
    {
        let mut c = cloud.lock().unwrap();
        // The create never reached EC2; a customer subnet covering its range did.
        c.subnets
            .retain(|subnet| subnet["cidrBlock"] != attempt.cidr);
        c.subnets.push(json!({"subnetId":"subnet-customer","vpcId":"vpc-existing","cidrBlock":"10.54.32.0/20","availabilityZone":"us-east-2a","availabilityZoneId":"use2-aza","state":"available"}));
    }
    let mut execution = executor_with_authority(
        &cloud,
        checkpoint,
        true,
        InitialSetupAuthority::ImportedHandoff,
    )
    .await;
    execution.delete().unwrap();
    execution.run_until_terminal().await.unwrap();
    assert_eq!(
        execution
            .internal_state::<AwsNetworkController>()
            .unwrap()
            .state,
        AwsNetworkState::Deleted
    );
    let c = cloud.lock().unwrap();
    assert!(c
        .subnets
        .iter()
        .any(|subnet| subnet["subnetId"] == "subnet-customer"));
    assert_eq!(c.subnets.len(), 3);
}

/// When the VPC itself was deleted outside Alien, a lost subnet create left nothing behind and
/// teardown finishes instead of failing on every retry.
#[tokio::test]
async fn teardown_settles_a_lost_create_in_a_vpc_deleted_outside_alien() {
    let cloud = workload_cloud();
    let checkpoint = checkpoint_before_first_dedicated_subnet(&cloud).await;
    {
        let mut c = cloud.lock().unwrap();
        c.vpc_deleted = true;
        c.subnets.clear();
        c.tables.clear();
    }
    let mut execution = executor_with_authority(
        &cloud,
        checkpoint,
        true,
        InitialSetupAuthority::ImportedHandoff,
    )
    .await;
    execution.delete().unwrap();
    execution.run_until_terminal().await.unwrap();
    let state = execution.internal_state::<AwsNetworkController>().unwrap();
    assert_eq!(state.state, AwsNetworkState::Deleted);
    assert!(state.subnet_create_attempt.is_none());
}
