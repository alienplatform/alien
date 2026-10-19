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
        let c = c.lock().unwrap();
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
    ec2.expect_describe_route_tables().returning(move|request|{
        let c=c.lock().unwrap();Ok(parse(json!({"routeTableSet":{"item":c.tables.iter().filter(|table|matches_filters(table,request.filters.as_ref())).collect::<Vec<_>>()}})))
    });
    let c = cloud.clone();
    ec2.expect_create_route_table().returning(move|request|{
        let mut c=c.lock().unwrap();c.mutations+=1;
        let table=json!({"routeTableId":format!("rtb-dedicated-{}",c.mutations),"vpcId":request.vpc_id,"routeSet":{"item":[local()]},"associationSet":{"item":[]},"tagSet":tags(request.tag_specifications)});
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
    let mut builder = SingleControllerExecutor::builder()
        .resource(network())
        .controller(controller)
        .platform(Platform::Aws)
        .initial_setup_authority(InitialSetupAuthority::DirectSetup);
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
    let mut execution = executor(&cloud, controller, true).await;
    assert!(!execution.needs_update().unwrap());
    assert!(execution.step().await.is_err());

    // Running setup again repairs the missing NAT route and associations, one call per step.
    let mutations = cloud.lock().unwrap().mutations;
    execution.update(network()).unwrap();
    execution
        .run_until_status(ResourceStatus::Running)
        .await
        .unwrap();
    assert_eq!(cloud.lock().unwrap().mutations - mutations, 3);
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
    let mut execution = executor(&cloud, controller, true).await;
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
