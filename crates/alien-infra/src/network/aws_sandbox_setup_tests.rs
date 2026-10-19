use super::*;
use crate::core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider};
use alien_aws_clients::ec2::MockEc2Api;
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
    ec2.expect_describe_vpcs().returning(|_|Ok(parse(json!({"vpcSet":{"item":[{"vpcId":"vpc-existing","cidrBlock":"10.54.0.0/16","state":"available"}]}}))));
    let c = cloud.clone();
    ec2.expect_describe_subnets().returning(move |request| {
        let mut c = c.lock().unwrap();
        let by_cidr = request
            .filters
            .iter()
            .flatten()
            .any(|filter| filter.name == "cidr-block");
        if by_cidr && c.unlisted_lookups > 0 {
            c.unlisted_lookups -= 1;
            return Ok(parse(json!({"subnetSet":{"item":[]}})));
        }
        let subnets: Vec<_> = c
            .subnets
            .iter()
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
        let mut c=c.lock().unwrap();c.mutations+=1;
        assert!(!c.subnets.iter().any(|subnet|subnet["cidrBlock"]==request.cidr_block));
        let subnet=json!({"subnetId":format!("subnet-dedicated-{}",c.mutations),"vpcId":request.vpc_id,"cidrBlock":request.cidr_block,"availabilityZone":request.availability_zone,"availabilityZoneId":request.availability_zone,"state":"available","tagSet":tags(request.tag_specifications)});
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
        c.tables.push(json!({"routeTableId":"rtb-work","vpcId":"vpc-existing","routeSet":{"item":[local(),{"destinationCidrBlock":"0.0.0.0/0","natGatewayId":"nat-existing","state":"active"}]},"associationSet":{"item":[{"subnetId":"subnet-work-a"},{"subnetId":"subnet-work-b"}]}}));
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
        c.tables.push(json!({"routeTableId":"rtb-work","vpcId":"vpc-existing","routeSet":{"item":[local(),{"destinationCidrBlock":"0.0.0.0/0","natGatewayId":"nat-existing","state":"active"}]},"associationSet":{"item":[{"subnetId":"subnet-work-a"},{"subnetId":"subnet-work-b"}]}}));
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
        c.tables.push(json!({"routeTableId":"rtb-work","vpcId":"vpc-existing","routeSet":{"item":[local(),{"destinationCidrBlock":"0.0.0.0/0","natGatewayId":"nat-existing","state":"active"}]},"associationSet":{"item":[{"subnetId":"subnet-work-a"},{"subnetId":"subnet-work-b"}]}}));
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
