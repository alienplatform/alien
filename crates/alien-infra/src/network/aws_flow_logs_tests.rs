use super::*;
use crate::core::{controller_test::SingleControllerExecutor, MockPlatformServiceProvider};
use alien_aws_clients::cloudwatch_logs::MockCloudWatchLogsApi;
use alien_aws_clients::ec2::{
    CreateFlowLogsResponse, DescribeFlowLogsResponse, FlowLog, FlowLogIdSet, FlowLogSet, MockEc2Api,
};
use alien_aws_clients::iam::{
    CreateRoleResponse, CreateRoleResult, GetRoleResponse, GetRoleResult, MockIamApi, Role,
    Tag as IamTag, Tags as IamTags,
};
use alien_core::InitialSetupAuthority;
use std::sync::{Arc, Mutex};

const PREFIX: &str = "test";
const NETWORK_ID: &str = "net";
const NAME: &str = "test-net-flow-logs";

/// What exists in the account, and every mutating call made.
#[derive(Default)]
struct Cloud {
    /// Log groups and roles by name, with their tags.
    log_groups: Vec<(String, HashMap<String, String>)>,
    roles: Vec<(String, HashMap<String, String>)>,
    /// Flow logs by ID, with the client token that created each.
    flow_logs: Vec<(String, String)>,
    /// Creates to apply and then answer with an error, as a lost response.
    lose_flow_log_creates: usize,
    /// Flow log lists that miss every flow log, as an eventually consistent read.
    stale_flow_log_lists: usize,
    calls: Vec<String>,
}

fn ours() -> HashMap<String, String> {
    standard_resource_tags(PREFIX, NETWORK_ID)
}

fn unavailable() -> AlienError<CloudClientErrorData> {
    AlienError::new(CloudClientErrorData::RemoteServiceUnavailable {
        message: "connection reset".to_string(),
    })
}

fn exists(kind: &str, name: &str) -> AlienError<CloudClientErrorData> {
    AlienError::new(CloudClientErrorData::RemoteResourceConflict {
        message: format!("{kind} already exists"),
        resource_type: kind.to_string(),
        resource_name: name.to_string(),
    })
}

fn missing(kind: &str, name: &str) -> AlienError<CloudClientErrorData> {
    AlienError::new(CloudClientErrorData::RemoteResourceNotFound {
        resource_type: kind.to_string(),
        resource_name: name.to_string(),
    })
}

fn role(name: &str, tags: &HashMap<String, String>) -> Role {
    Role {
        path: "/".to_string(),
        role_name: name.to_string(),
        role_id: "AROAEXAMPLE".to_string(),
        arn: format!("arn:aws:iam::123456789012:role/{name}"),
        create_date: "2026-01-01T00:00:00Z".to_string(),
        assume_role_policy_document: None,
        description: None,
        max_session_duration: None,
        permissions_boundary: None,
        tags: Some(IamTags {
            member: tags
                .iter()
                .map(|(key, value)| IamTag {
                    key: key.clone(),
                    value: value.clone(),
                })
                .collect(),
        }),
        role_last_used: None,
    }
}

fn empty<T: serde::de::DeserializeOwned>(
) -> std::result::Result<T, AlienError<CloudClientErrorData>> {
    Ok(serde_json::from_value(serde_json::json!({})).unwrap())
}

fn provider(cloud: &Arc<Mutex<Cloud>>) -> Arc<MockPlatformServiceProvider> {
    let mut logs = MockCloudWatchLogsApi::new();
    let c = cloud.clone();
    logs.expect_create_log_group().returning(move |request| {
        let mut cloud = c.lock().unwrap();
        cloud
            .calls
            .push(format!("CreateLogGroup {}", request.log_group_name));
        if cloud
            .log_groups
            .iter()
            .any(|(name, _)| *name == request.log_group_name)
        {
            return Err(exists("LogGroup", &request.log_group_name));
        }
        cloud
            .log_groups
            .push((request.log_group_name, request.tags.unwrap_or_default()));
        Ok(())
    });
    let c = cloud.clone();
    logs.expect_list_tags_for_resource().returning(move |arn| {
        let cloud = c.lock().unwrap();
        let name = arn.rsplit_once(":log-group:").expect("log group ARN").1;
        cloud
            .log_groups
            .iter()
            .find(|(group, _)| group == name)
            .map(|(_, tags)| tags.clone())
            .ok_or_else(|| missing("LogGroup", name))
    });
    let c = cloud.clone();
    logs.expect_put_retention_policy()
        .returning(move |request| {
            c.lock().unwrap().calls.push(format!(
                "PutRetentionPolicy {} {}",
                request.log_group_name, request.retention_in_days
            ));
            Ok(())
        });
    let c = cloud.clone();
    logs.expect_delete_log_group().returning(move |name| {
        let mut cloud = c.lock().unwrap();
        cloud.calls.push(format!("DeleteLogGroup {name}"));
        let before = cloud.log_groups.len();
        cloud.log_groups.retain(|(group, _)| group != name);
        if cloud.log_groups.len() == before {
            return Err(missing("LogGroup", name));
        }
        Ok(())
    });

    let mut iam = MockIamApi::new();
    let c = cloud.clone();
    iam.expect_create_role().returning(move |request| {
        let mut cloud = c.lock().unwrap();
        cloud
            .calls
            .push(format!("CreateRole {}", request.role_name));
        if cloud
            .roles
            .iter()
            .any(|(name, _)| *name == request.role_name)
        {
            return Err(exists("Role", &request.role_name));
        }
        let tags: HashMap<String, String> = request
            .tags
            .iter()
            .flatten()
            .map(|tag| (tag.key.clone(), tag.value.clone()))
            .collect();
        let created = role(&request.role_name, &tags);
        cloud.roles.push((request.role_name, tags));
        Ok(CreateRoleResponse {
            create_role_result: CreateRoleResult { role: created },
        })
    });
    let c = cloud.clone();
    iam.expect_get_role().returning(move |name| {
        let cloud = c.lock().unwrap();
        cloud
            .roles
            .iter()
            .find(|(role_name, _)| role_name == name)
            .map(|(role_name, tags)| GetRoleResponse {
                get_role_result: GetRoleResult {
                    role: role(role_name, tags),
                },
            })
            .ok_or_else(|| missing("Role", name))
    });
    let c = cloud.clone();
    iam.expect_put_role_policy()
        .returning(move |role_name, policy_name, _| {
            c.lock()
                .unwrap()
                .calls
                .push(format!("PutRolePolicy {role_name} {policy_name}"));
            Ok(())
        });
    let c = cloud.clone();
    iam.expect_delete_role_policy()
        .returning(move |role_name, policy_name| {
            c.lock()
                .unwrap()
                .calls
                .push(format!("DeleteRolePolicy {role_name} {policy_name}"));
            Ok(())
        });
    let c = cloud.clone();
    iam.expect_delete_role().returning(move |name| {
        let mut cloud = c.lock().unwrap();
        cloud.calls.push(format!("DeleteRole {name}"));
        cloud.roles.retain(|(role, _)| role != name);
        Ok(())
    });

    let mut ec2 = MockEc2Api::new();
    let c = cloud.clone();
    ec2.expect_create_flow_logs().returning(move |request| {
        let mut cloud = c.lock().unwrap();
        cloud.calls.push(format!(
            "CreateFlowLogs {} {} {} {} {}",
            request.resource_ids.join(","),
            request.traffic_type,
            request.log_group_name,
            request.deliver_logs_permission_arn,
            request.client_token
        ));
        let id = match cloud
            .flow_logs
            .iter()
            .find(|(_, token)| *token == request.client_token)
        {
            Some((id, _)) => id.clone(),
            None => {
                let id = format!("fl-{}", cloud.flow_logs.len() + 1);
                cloud
                    .flow_logs
                    .push((id.clone(), request.client_token.clone()));
                id
            }
        };
        if cloud.lose_flow_log_creates > 0 {
            cloud.lose_flow_log_creates -= 1;
            return Err(unavailable());
        }
        Ok(CreateFlowLogsResponse {
            flow_log_id_set: Some(FlowLogIdSet { items: vec![id] }),
            unsuccessful: None,
        })
    });
    let c = cloud.clone();
    ec2.expect_describe_flow_logs().returning(move |request| {
        let mut cloud = c.lock().unwrap();
        let filter = |name: &str| {
            request
                .filters
                .iter()
                .flatten()
                .find(|filter| filter.name == name)
                .map(|filter| filter.values.clone())
        };
        assert_eq!(filter("resource-id"), Some(vec!["vpc-1".to_string()]));
        assert_eq!(filter("log-group-name"), Some(vec![NAME.to_string()]));
        assert_eq!(filter("tag:deployment"), Some(vec![PREFIX.to_string()]));
        assert_eq!(filter("tag:managed-by"), Some(vec!["runtime".to_string()]));
        let items = if cloud.stale_flow_log_lists > 0 {
            cloud.stale_flow_log_lists -= 1;
            Vec::new()
        } else {
            cloud
                .flow_logs
                .iter()
                .map(|(id, _)| FlowLog {
                    flow_log_id: Some(id.clone()),
                    flow_log_status: Some("ACTIVE".to_string()),
                    resource_id: Some("vpc-1".to_string()),
                    traffic_type: Some("ALL".to_string()),
                    log_destination_type: Some("cloud-watch-logs".to_string()),
                    log_group_name: Some(NAME.to_string()),
                    deliver_logs_permission_arn: None,
                    deliver_logs_status: Some("SUCCESS".to_string()),
                    deliver_logs_error_message: None,
                    max_aggregation_interval: Some(600),
                    tag_set: None,
                })
                .collect()
        };
        Ok(DescribeFlowLogsResponse {
            flow_log_set: Some(FlowLogSet { items }),
            next_token: None,
        })
    });
    let c = cloud.clone();
    ec2.expect_delete_flow_log().returning(move |id| {
        let mut cloud = c.lock().unwrap();
        cloud.calls.push(format!("DeleteFlowLogs {id}"));
        let before = cloud.flow_logs.len();
        cloud.flow_logs.retain(|(existing, _)| existing != id);
        if cloud.flow_logs.len() == before {
            return Err(missing("FlowLog", id));
        }
        Ok(())
    });
    // The rest of a delete from a network with only its VPC recorded.
    ec2.expect_describe_vpcs().returning(|_| empty());
    ec2.expect_describe_network_interfaces()
        .returning(|_| empty());
    ec2.expect_describe_route_tables().returning(|_| empty());
    ec2.expect_describe_internet_gateways()
        .returning(|_| empty());
    ec2.expect_describe_subnets().returning(|_| empty());
    ec2.expect_describe_security_groups().returning(|_| empty());
    ec2.expect_describe_nat_gateways().returning(|_| empty());
    let c = cloud.clone();
    ec2.expect_delete_vpc().returning(move |id| {
        c.lock().unwrap().calls.push(format!("DeleteVpc {id}"));
        Ok(())
    });

    let (logs, iam, ec2) = (Arc::new(logs), Arc::new(iam), Arc::new(ec2));
    let mut provider = MockPlatformServiceProvider::new();
    provider
        .expect_get_aws_logs_client()
        .returning(move |_| Ok(logs.clone()));
    provider
        .expect_get_aws_iam_client()
        .returning(move |_| Ok(iam.clone()));
    provider
        .expect_get_aws_ec2_client()
        .returning(move |_| Ok(ec2.clone()));
    Arc::new(provider)
}

fn network() -> Network {
    Network::new(NETWORK_ID.to_string())
        .settings(NetworkSettings::Create {
            cidr: None,
            availability_zones: 2,
        })
        .build()
}

/// A network whose VPC exists and whose create flow reached the flow log.
fn creating_flow_logs() -> AwsNetworkController {
    AwsNetworkController {
        state: AwsNetworkState::CreatingFlowLogs,
        vpc_id: Some("vpc-1".to_string()),
        ..Default::default()
    }
}

async fn executor(
    cloud: &Arc<Mutex<Cloud>>,
    controller: AwsNetworkController,
    authority: InitialSetupAuthority,
    setup_run: bool,
) -> SingleControllerExecutor {
    SingleControllerExecutor::builder()
        .resource(network())
        .controller(controller)
        .platform(Platform::Aws)
        .service_provider(provider(cloud))
        .resource_prefix(PREFIX)
        .initial_setup_authority(authority)
        .setup_run(setup_run)
        .build()
        .await
        .expect("executor builds")
}

async fn direct_setup(
    cloud: &Arc<Mutex<Cloud>>,
    controller: AwsNetworkController,
) -> SingleControllerExecutor {
    executor(cloud, controller, InitialSetupAuthority::DirectSetup, false).await
}

fn flow_logs(executor: &SingleControllerExecutor) -> Option<FlowLogSetup> {
    executor
        .internal_state::<AwsNetworkController>()
        .expect("network controller")
        .flow_logs
        .clone()
}

#[tokio::test]
async fn creates_the_log_group_then_the_role_then_the_flow_log_and_deletes_them_before_the_vpc() {
    let cloud = Arc::new(Mutex::new(Cloud::default()));
    let mut executor = direct_setup(&cloud, creating_flow_logs()).await;

    executor
        .run_until_terminal()
        .await
        .expect("create finishes");
    assert_eq!(executor.status(), ResourceStatus::Running);
    let recorded = flow_logs(&executor).expect("flow log recorded");
    assert_eq!(recorded.flow_log_id.as_deref(), Some("fl-1"));
    let token = recorded.create_token.clone().expect("token recorded");
    let role_arn = format!("arn:aws:iam::123456789012:role/{NAME}");
    {
        let mut cloud = cloud.lock().unwrap();
        assert_eq!(
            std::mem::take(&mut cloud.calls),
            [
                format!("CreateLogGroup {NAME}"),
                format!("PutRetentionPolicy {NAME} 365"),
                format!("CreateRole {NAME}"),
                format!("PutRolePolicy {NAME} flow-logs-delivery"),
                format!("CreateFlowLogs vpc-1 ALL {NAME} {role_arn} {token}"),
            ]
        );
        assert_eq!(cloud.log_groups, [(NAME.to_string(), ours())]);
        assert_eq!(cloud.roles, [(NAME.to_string(), ours())]);
    }

    executor.delete().expect("delete starts");
    executor
        .run_until_terminal()
        .await
        .expect("delete finishes");
    assert_eq!(executor.status(), ResourceStatus::Deleted);
    let cloud = cloud.lock().unwrap();
    assert_eq!(
        cloud.calls,
        [
            "DeleteFlowLogs fl-1".to_string(),
            format!("DeleteRolePolicy {NAME} flow-logs-delivery"),
            format!("DeleteRole {NAME}"),
            format!("DeleteLogGroup {NAME}"),
            "DeleteVpc vpc-1".to_string(),
        ]
    );
    assert!(cloud.log_groups.is_empty() && cloud.roles.is_empty() && cloud.flow_logs.is_empty());
    assert_eq!(flow_logs(&executor), None);
}

/// EC2 created the flow log but the response was lost, and the lists that follow do not show
/// it yet. The retry repeats the call with the recorded token, which returns the same flow log
/// instead of a second one.
#[tokio::test]
async fn a_lost_flow_log_create_is_repeated_with_its_token_and_makes_one_flow_log() {
    let cloud = Arc::new(Mutex::new(Cloud {
        lose_flow_log_creates: 1,
        stale_flow_log_lists: 2,
        ..Default::default()
    }));
    let mut executor = direct_setup(&cloud, creating_flow_logs()).await;

    for _ in 0..3 {
        executor.step().await.expect("log group, role, policy");
    }
    executor
        .step()
        .await
        .expect_err("the create's response is lost");
    let token = flow_logs(&executor)
        .and_then(|setup| setup.create_token)
        .expect("token kept after the lost response");
    executor.run_until_terminal().await.expect("retry finishes");

    assert_eq!(executor.status(), ResourceStatus::Running);
    let cloud = cloud.lock().unwrap();
    assert_eq!(cloud.flow_logs, [("fl-1".to_string(), token.clone())]);
    let creates: Vec<_> = cloud
        .calls
        .iter()
        .filter(|call| call.starts_with("CreateFlowLogs"))
        .collect();
    assert_eq!(creates.len(), 2);
    assert!(creates.iter().all(|call| call.ends_with(&token)));
    assert_eq!(
        flow_logs(&executor).and_then(|setup| setup.flow_log_id),
        Some("fl-1".to_string())
    );
}

/// A log group with the derived name that a Terraform setup made (its tags say `setup`) is
/// refused, and the delete that follows leaves it alone.
#[tokio::test]
async fn a_same_named_log_group_without_this_networks_tags_is_refused_and_kept_on_delete() {
    let mut template_tags = ours();
    template_tags.insert("managed-by".to_string(), "setup".to_string());
    let cloud = Arc::new(Mutex::new(Cloud {
        log_groups: vec![(NAME.to_string(), template_tags.clone())],
        ..Default::default()
    }));
    let mut executor = direct_setup(&cloud, creating_flow_logs()).await;

    let error = executor
        .step()
        .await
        .expect_err("a group without this network's tags is not adopted");
    assert_eq!(error.code, "RESOURCE_CONFIG_INVALID");

    executor.delete().expect("delete starts");
    executor
        .run_until_terminal()
        .await
        .expect("delete finishes");
    assert_eq!(executor.status(), ResourceStatus::Deleted);
    let cloud = cloud.lock().unwrap();
    assert_eq!(cloud.log_groups, [(NAME.to_string(), template_tags)]);
    assert_eq!(
        cloud.calls,
        [
            format!("CreateLogGroup {NAME}"),
            "DeleteVpc vpc-1".to_string()
        ]
    );
}

/// Objects this network made earlier are taken over by name and tags, even when the state
/// that recorded them is gone, as after an older binary rewrote it: no duplicate, no refusal,
/// and a delete without any record still removes them.
#[tokio::test]
async fn objects_carrying_this_networks_tags_are_taken_over_without_a_record() {
    let cloud = Arc::new(Mutex::new(Cloud {
        log_groups: vec![(NAME.to_string(), ours())],
        roles: vec![(NAME.to_string(), ours())],
        flow_logs: vec![("fl-7".to_string(), "earlier-token".to_string())],
        ..Default::default()
    }));
    let mut executor = direct_setup(&cloud, creating_flow_logs()).await;

    executor
        .run_until_terminal()
        .await
        .expect("create finishes");
    assert_eq!(executor.status(), ResourceStatus::Running);
    assert_eq!(
        flow_logs(&executor).and_then(|setup| setup.flow_log_id),
        Some("fl-7".to_string())
    );
    assert!(!cloud
        .lock()
        .unwrap()
        .calls
        .iter()
        .any(|call| call.starts_with("CreateFlowLogs")));

    // Lose the record, as an older binary rewriting the checkpoint would.
    let mut forgotten = executor
        .internal_state::<AwsNetworkController>()
        .expect("network controller")
        .clone();
    forgotten.flow_logs = None;
    cloud.lock().unwrap().calls.clear();
    let mut executor = direct_setup(&cloud, forgotten).await;
    executor.delete().expect("delete starts");
    executor
        .run_until_terminal()
        .await
        .expect("delete finishes");
    assert_eq!(executor.status(), ResourceStatus::Deleted);
    let cloud = cloud.lock().unwrap();
    assert!(cloud.log_groups.is_empty() && cloud.roles.is_empty() && cloud.flow_logs.is_empty());
    assert_eq!(
        cloud.calls,
        [
            "DeleteFlowLogs fl-7".to_string(),
            format!("DeleteRolePolicy {NAME} flow-logs-delivery"),
            format!("DeleteRole {NAME}"),
            format!("DeleteLogGroup {NAME}"),
            "DeleteVpc vpc-1".to_string(),
        ]
    );
}

/// A VPC that direct setup created before flow logs existed gets one on the next setup run,
/// and only then: a release update runs under the management identity, which cannot create
/// the role, and an imported network belongs to its template.
#[tokio::test]
async fn a_setup_run_adds_the_flow_log_to_a_vpc_direct_setup_created_before() {
    // Saved by the previous version: no flow log state at all.
    let saved = serde_json::json!({
        "state": "ready",
        "vpcId": "vpc-1",
        "cidrBlock": "10.42.0.0/16",
        "publicSubnetIds": [],
        "privateSubnetIds": [],
        "routeTableAssociationIds": [],
        "availabilityZones": [],
        "isByoVpc": false
    });
    let previous: AwsNetworkController =
        serde_json::from_value(saved).expect("previous checkpoint loads");
    assert_eq!(previous.flow_logs, None);

    for (authority, setup_run, expected) in [
        (InitialSetupAuthority::DirectSetup, false, false),
        (InitialSetupAuthority::ImportedHandoff, true, false),
        (InitialSetupAuthority::DirectSetup, true, true),
    ] {
        let cloud = Arc::new(Mutex::new(Cloud::default()));
        let executor = executor(&cloud, previous.clone(), authority, setup_run).await;
        assert_eq!(
            executor.needs_update().expect("needs_update"),
            expected,
            "{authority:?}, setup run: {setup_run}"
        );
    }

    let cloud = Arc::new(Mutex::new(Cloud::default()));
    let mut executor = executor(&cloud, previous, InitialSetupAuthority::DirectSetup, true).await;
    executor.update(network()).expect("update starts");
    executor
        .run_until_terminal()
        .await
        .expect("update finishes");
    assert_eq!(executor.status(), ResourceStatus::Running);
    assert!(flow_logs(&executor).is_some_and(|setup| setup.is_complete()));
    assert_eq!(cloud.lock().unwrap().flow_logs.len(), 1);
    assert!(!executor.needs_update().expect("needs_update"));
}
