use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};

use alien_aws_clients::elbv2::MockElbv2Api;
use serde_json::{json, Value};

use super::*;

#[derive(Default)]
struct World {
    group: Mutex<Option<Value>>,
    tags: Mutex<Vec<ElbTag>>,
    creates: AtomicUsize,
    deletes: AtomicUsize,
    lose_create: AtomicBool,
    lose_delete: AtomicBool,
    draining: AtomicBool,
    deny_tags: AtomicBool,
}

fn denied() -> AlienError<ClientErrorData> {
    AlienError::new(ClientErrorData::RemoteAccessDenied {
        resource_type: "TargetGroup".to_string(),
        resource_name: "owned".to_string(),
    })
}

fn client(world: Arc<World>) -> MockElbv2Api {
    let mut elb = MockElbv2Api::new();
    let read = world.clone();
    elb.expect_describe_target_groups()
        .returning(move |request| {
            let group = read.group.lock().unwrap();
            let items = if let Some(group) = group.as_ref() {
                assert_eq!(
                    request.names.as_ref().unwrap(),
                    &[group["TargetGroupName"].as_str().unwrap().to_string()]
                );
                vec![group.clone()]
            } else {
                vec![]
            };
            Ok(serde_json::from_value(
                json!({"DescribeTargetGroupsResult":{"TargetGroups":{"member":items}}}),
            )
            .unwrap())
        });
    let read = world.clone();
    elb.expect_describe_tags().returning(move |arns| {
        assert_eq!(arns, ["owned-tg"]);
        if read.deny_tags.load(Ordering::SeqCst) {return Err(denied());}
        let tags:Vec<Value> = read.tags.lock().unwrap().iter().map(|tag| json!({"Key":tag.key,"Value":tag.value})).collect();
        Ok(serde_json::from_value(json!({"DescribeTagsResult":{"TagDescriptions":{"member":[{"ResourceArn":"owned-tg","Tags":{"member":tags}}]}}})).unwrap())
    });
    let create = world.clone();
    elb.expect_create_target_group().returning(move |request| {
        create.creates.fetch_add(1,Ordering::SeqCst);
        assert!(request.name.len() <=32);
        *create.tags.lock().unwrap() = request.tags.unwrap();
        let group = json!({"TargetGroupArn":"owned-tg","TargetGroupName":request.name,"Protocol":request.protocol,"Port":request.port,"VpcId":request.vpc_id,"TargetType":request.target_type});
        *create.group.lock().unwrap()=Some(group.clone());
        if create.lose_create.swap(false,Ordering::SeqCst) {return Err(AlienError::new(ClientErrorData::Timeout { message: "create response lost after commit".to_string() }));}
        Ok(serde_json::from_value(json!({"CreateTargetGroupResult":{"TargetGroups":{"member":[group]}}})).unwrap())
    });
    elb.expect_delete_target_group().returning(move |arn| {
        assert_eq!(arn, "owned-tg");
        if world.draining.load(Ordering::SeqCst) {
            return Err(AlienError::new(ClientErrorData::RemoteResourceConflict {
                resource_type: "TargetGroup".to_string(),
                resource_name: arn.to_string(),
                message: "ResourceInUse: load balancer association is still draining".to_string(),
            }));
        }
        world.deletes.fetch_add(1, Ordering::SeqCst);
        assert!(world.group.lock().unwrap().take().is_some());
        if world.lose_delete.swap(false, Ordering::SeqCst) {
            return Err(AlienError::new(ClientErrorData::Timeout {
                message: "delete response lost after commit".to_string(),
            }));
        }
        Ok(())
    });
    elb
}

fn request() -> CreateTargetGroupRequest {
    CreateTargetGroupRequest::builder()
        .name("test-worker-public-tg".to_string())
        .protocol("HTTP".to_string())
        .port(8080)
        .vpc_id("vpc-owned".to_string())
        .target_type("instance".to_string())
        .tags(vec![ElbTag {
            key: "Owner".to_string(),
            value: "test-resource".to_string(),
        }])
        .build()
}

fn reload(state: &AwsVmTargetGroup) -> AwsVmTargetGroup {
    serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap()
}

#[tokio::test]
async fn lost_create_and_delete_resume_from_saved_ownership_without_duplicate_mutations() {
    let world = Arc::new(World::default());
    world.lose_create.store(true, Ordering::SeqCst);
    world.lose_delete.store(true, Ordering::SeqCst);
    let elb = client(world.clone());
    let mut group = AwsVmTargetGroup::default();
    assert!(group.step_create(&elb, request()).await.unwrap().is_none());
    assert_eq!(world.creates.load(Ordering::SeqCst), 0);
    let intent = reload(&group);
    group.step_create(&elb, request()).await.unwrap_err();
    let mut restarted = intent;
    assert!(restarted
        .step_create(&elb, request())
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        restarted
            .step_create(&elb, request())
            .await
            .unwrap()
            .as_deref(),
        Some("owned-tg")
    );
    assert_eq!(world.creates.load(Ordering::SeqCst), 1);
    let before_delete = reload(&restarted);
    let error = restarted.step_delete(&elb).await.unwrap_err();
    assert!(
        error.retryable,
        "lost delete response must remain retryable"
    );
    let mut restarted = before_delete;
    assert!(restarted.step_delete(&elb).await.unwrap());
    assert_eq!(world.deletes.load(Ordering::SeqCst), 1);
    assert!(restarted.token.is_none());
}

#[tokio::test]
async fn abandoned_lost_create_is_discovered_checkpointed_and_drained_before_cleanup() {
    let world = Arc::new(World::default());
    world.lose_create.store(true, Ordering::SeqCst);
    let elb = client(world.clone());
    let mut group = AwsVmTargetGroup::default();
    group.step_create(&elb, request()).await.unwrap();
    let mut intent = reload(&group);
    group.step_create(&elb, request()).await.unwrap_err();
    assert!(!intent.step_delete(&elb).await.unwrap());
    assert_eq!(
        world.deletes.load(Ordering::SeqCst),
        0,
        "persist recovered ARN before deleting"
    );
    world.draining.store(true, Ordering::SeqCst);
    assert!(!intent.step_delete(&elb).await.unwrap());
    assert_eq!(intent.arn.as_deref(), Some("owned-tg"));
    world.draining.store(false, Ordering::SeqCst);
    assert!(!intent.step_delete(&elb).await.unwrap());
    assert!(
        intent.arn.is_some(),
        "retain ownership until later absence read"
    );
    assert!(intent.step_delete(&elb).await.unwrap());
    assert_eq!(world.creates.load(Ordering::SeqCst), 1);
    assert_eq!(world.deletes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn foreign_tags_and_denied_metadata_never_adopt_or_best_effort_delete() {
    let world = Arc::new(World::default());
    let elb = client(world.clone());
    let mut group = AwsVmTargetGroup::default();
    group.step_create(&elb, request()).await.unwrap();
    group.step_create(&elb, request()).await.unwrap();
    world.deny_tags.store(true, Ordering::SeqCst);
    let error = group.step_delete(&elb).await.unwrap_err();
    assert!(!matches!(error.http_status_code, Some(401 | 403 | 404)));
    assert!(
        error.source.is_none(),
        "generic delete executor must not skip the rest of cleanup"
    );
    world.deny_tags.store(false, Ordering::SeqCst);
    world
        .tags
        .lock()
        .unwrap()
        .iter_mut()
        .find(|tag| tag.key == "Owner")
        .unwrap()
        .value = "foreign-resource".to_string();
    group.step_create(&elb, request()).await.unwrap_err();
    group.step_delete(&elb).await.unwrap_err();
    assert_eq!(world.creates.load(Ordering::SeqCst), 1);
    assert_eq!(world.deletes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn changed_plan_cannot_replace_a_registered_target_group() {
    let world = Arc::new(World::default());
    let elb = client(world.clone());
    let mut group = AwsVmTargetGroup::default();
    group.step_create(&elb, request()).await.unwrap();
    group.step_create(&elb, request()).await.unwrap();
    let mut changed = request();
    changed.port = Some(9090);
    group.step_create(&elb, changed).await.unwrap_err();
    assert_eq!(world.creates.load(Ordering::SeqCst), 1);
    assert_eq!(world.deletes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn changed_remote_health_check_is_not_confirmed_as_the_requested_frontend() {
    let world = Arc::new(World::default());
    let elb = client(world.clone());
    let mut group = AwsVmTargetGroup::default();
    let mut plan = request();
    plan.health_check_path = Some("/ready".to_string());
    group.step_create(&elb, plan.clone()).await.unwrap();
    group.step_create(&elb, plan.clone()).await.unwrap();
    world.group.lock().unwrap().as_mut().unwrap()["HealthCheckPath"] = json!("/wrong");
    group.step_create(&elb, plan.clone()).await.unwrap_err();
    world.group.lock().unwrap().as_mut().unwrap()["HealthCheckPath"] = json!("/ready");
    assert_eq!(
        group.step_create(&elb, plan).await.unwrap().as_deref(),
        Some("owned-tg")
    );
    assert_eq!(world.creates.load(Ordering::SeqCst), 1);
    assert_eq!(world.deletes.load(Ordering::SeqCst), 0);
}
