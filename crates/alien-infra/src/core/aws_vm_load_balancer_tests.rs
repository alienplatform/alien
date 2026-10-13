use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Mutex,
};

use alien_aws_clients::elbv2::{Action, Certificate, CreateListenerRequest, MockElbv2Api};
use serde_json::{json, Value};

use super::*;

#[derive(Default)]
struct World {
    lb: Mutex<Option<Value>>,
    listener: Mutex<Option<Value>>,
    listener_tags: Mutex<Vec<ElbTag>>,
    listener_creates: AtomicUsize,
    listener_updates: AtomicUsize,
    lose_listener_create: AtomicBool,
    lose_listener_update: AtomicBool,
    tags: Mutex<Vec<ElbTag>>,
    creates: AtomicUsize,
    deletes: AtomicUsize,
    lose_create: AtomicBool,
    lose_delete: AtomicBool,
    draining: AtomicBool,
    deny_tags: AtomicBool,
}

fn lost_response() -> AlienError<ClientErrorData> {
    AlienError::new(ClientErrorData::Timeout {
        message: "response lost after provider committed mutation".to_string(),
    })
}

fn client(world: Arc<World>) -> MockElbv2Api {
    let mut elb = MockElbv2Api::new();
    let read = world.clone();
    elb.expect_describe_load_balancers()
        .returning(move |request| {
            let lb = read.lb.lock().unwrap();
            let lbs = if let Some(lb) = lb.as_ref() {
                assert_eq!(
                    request.names.as_ref().unwrap(),
                    &[lb["LoadBalancerName"].as_str().unwrap().to_string()]
                );
                vec![lb.clone()]
            } else {
                vec![]
            };
            Ok(serde_json::from_value(
                json!({"DescribeLoadBalancersResult":{"LoadBalancers":{"member":lbs}}}),
            )
            .unwrap())
        });
    let read = world.clone();
    elb.expect_describe_tags().returning(move |arns| {
        assert_eq!(arns.len(), 1);
        assert!(matches!(arns[0].as_str(), "owned-lb" | "owned-listener"));
        if read.deny_tags.load(Ordering::SeqCst) {
            return Err(AlienError::new(ClientErrorData::RemoteAccessDenied { resource_type: "LoadBalancer".to_string(), resource_name: "owned-lb".to_string() }));
        }
        let source = if arns[0] == "owned-lb" { &read.tags } else { &read.listener_tags };
        let tags: Vec<Value> = source.lock().unwrap().iter().map(|tag| json!({"Key":tag.key,"Value":tag.value})).collect();
        Ok(serde_json::from_value(json!({"DescribeTagsResult":{"TagDescriptions":{"member":[{"ResourceArn":arns[0],"Tags":{"member":tags}}]}}})).unwrap())
    });
    let create = world.clone();
    elb.expect_create_load_balancer().returning(move |request| {
        create.creates.fetch_add(1, Ordering::SeqCst);
        assert!(request.name.len() <= 32);
        *create.tags.lock().unwrap() = request.tags.unwrap();
        let zones: Vec<Value> = request.subnets.iter().map(|subnet| json!({"SubnetId":subnet})).collect();
        let lb = json!({"LoadBalancerArn":"owned-lb","LoadBalancerName":request.name,"Scheme":request.scheme,"Type":request.load_balancer_type,"IpAddressType":request.ip_address_type,"AvailabilityZones":{"member":zones},"SecurityGroups":{"member":request.security_groups.unwrap_or_default()},"State":{"Code":"provisioning"},"DNSName":"lb.example.test","CanonicalHostedZoneId":"owned-zone"});
        *create.lb.lock().unwrap() = Some(lb.clone());
        if create.lose_create.swap(false, Ordering::SeqCst) { return Err(lost_response()); }
        Ok(serde_json::from_value(json!({"CreateLoadBalancerResult":{"LoadBalancers":{"member":[lb]}}})).unwrap())
    });
    let read = world.clone();
    elb.expect_describe_listeners().returning(move |request| {
        assert_eq!(request.load_balancer_arn.as_deref(), Some("owned-lb"));
        let listeners: Vec<Value> = read.listener.lock().unwrap().iter().cloned().collect();
        Ok(serde_json::from_value(
            json!({"DescribeListenersResult":{"Listeners":{"member":listeners}}}),
        )
        .unwrap())
    });
    let create = world.clone();
    elb.expect_create_listener().returning(move |request| {
        create.listener_creates.fetch_add(1, Ordering::SeqCst);
        assert!(create.listener.lock().unwrap().is_none());
        *create.listener_tags.lock().unwrap() = request.tags.unwrap();
        let certs: Vec<Value> = request.certificates.unwrap_or_default().iter().map(|certificate| json!({"CertificateArn":certificate.certificate_arn,"IsDefault":true})).collect();
        let listener = json!({"ListenerArn":"owned-listener","LoadBalancerArn":request.load_balancer_arn,"Port":request.port,"Protocol":request.protocol,"Certificates":{"member":certs},"DefaultActions":{"member":[{"Type":"forward","TargetGroupArn":request.default_actions[0].target_group_arn}]}});
        *create.listener.lock().unwrap() = Some(listener.clone());
        if create.lose_listener_create.swap(false, Ordering::SeqCst) { return Err(lost_response()); }
        Ok(serde_json::from_value(json!({"CreateListenerResult":{"Listeners":{"member":[listener]}}})).unwrap())
    });
    let update = world.clone();
    elb.expect_modify_listener().returning(move |request| {
        assert_eq!(request.listener_arn, "owned-listener");
        assert!(request.default_actions.is_none(), "rotation must preserve forwarding");
        assert!(request.port.is_none());
        assert!(request.protocol.is_none());
        update.listener_updates.fetch_add(1, Ordering::SeqCst);
        let certs: Vec<Value> = request.certificates.unwrap().iter().map(|certificate| json!({"CertificateArn":certificate.certificate_arn,"IsDefault":true})).collect();
        let mut listener = update.listener.lock().unwrap();
        listener.as_mut().unwrap()["Certificates"] = json!({"member":certs});
        if update.lose_listener_update.swap(false, Ordering::SeqCst) { return Err(lost_response()); }
        Ok(serde_json::from_value(json!({"ModifyListenerResult":{"Listeners":{"member":[listener.as_ref().unwrap()]}}})).unwrap())
    });
    elb.expect_delete_load_balancer().returning(move |arn| {
        assert_eq!(arn, "owned-lb");
        if world.draining.load(Ordering::SeqCst) {
            return Err(AlienError::new(ClientErrorData::RemoteResourceConflict {
                resource_type: "LoadBalancer".to_string(),
                resource_name: arn.to_string(),
                message: "ResourceInUse: association is draining".to_string(),
            }));
        }
        world.deletes.fetch_add(1, Ordering::SeqCst);
        assert!(world.lb.lock().unwrap().take().is_some());
        world.listener.lock().unwrap().take();
        if world.lose_delete.swap(false, Ordering::SeqCst) {
            return Err(lost_response());
        }
        Ok(())
    });
    elb
}

fn request(scheme: &str) -> CreateLoadBalancerRequest {
    CreateLoadBalancerRequest::builder()
        .name("vm-frontend".to_string())
        .subnets(vec!["subnet-b".to_string(), "subnet-a".to_string()])
        .security_groups(vec!["sg-owned".to_string()])
        .scheme(scheme.to_string())
        .load_balancer_type("application".to_string())
        .tags(vec![ElbTag {
            key: "Owner".to_string(),
            value: "test-resource".to_string(),
        }])
        .build()
}

fn reload(state: &AwsVmLoadBalancer) -> AwsVmLoadBalancer {
    serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap()
}

#[tokio::test]
async fn lost_create_and_delete_resume_without_replacing_either_frontend() {
    for scheme in ["internal", "internet-facing"] {
        let world = Arc::new(World::default());
        world.lose_create.store(true, Ordering::SeqCst);
        world.lose_delete.store(true, Ordering::SeqCst);
        let elb = client(world.clone());
        let mut lb = AwsVmLoadBalancer::default();
        assert!(lb
            .step_create(&elb, request(scheme))
            .await
            .unwrap()
            .is_none());
        assert_eq!(world.creates.load(Ordering::SeqCst), 0);
        let intent = reload(&lb);
        lb.step_create(&elb, request(scheme)).await.unwrap_err();
        let mut resumed = intent;
        assert!(resumed
            .step_create(&elb, request(scheme))
            .await
            .unwrap()
            .is_none());
        assert!(
            resumed
                .step_create(&elb, request(scheme))
                .await
                .unwrap()
                .is_none(),
            "provisioning is not ready"
        );
        world.lb.lock().unwrap().as_mut().unwrap()["State"]["Code"] = json!("active");
        let observed = resumed
            .step_create(&elb, request(scheme))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(observed.scheme.as_deref(), Some(scheme));
        assert_eq!(observed.load_balancer_arn.as_deref(), Some("owned-lb"));
        let saved = reload(&resumed);
        resumed.step_delete(&elb).await.unwrap_err();
        let mut resumed = saved;
        assert!(resumed.step_delete(&elb).await.unwrap());
        assert_eq!(world.creates.load(Ordering::SeqCst), 1);
        assert_eq!(world.deletes.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn abandoned_create_cleanup_checkpoints_identity_and_waits_for_drain() {
    let world = Arc::new(World::default());
    world.lose_create.store(true, Ordering::SeqCst);
    let elb = client(world.clone());
    let mut lb = AwsVmLoadBalancer::default();
    lb.step_create(&elb, request("internal")).await.unwrap();
    let mut intent = reload(&lb);
    lb.step_create(&elb, request("internal")).await.unwrap_err();
    assert!(!intent.step_delete(&elb).await.unwrap());
    assert_eq!(world.deletes.load(Ordering::SeqCst), 0);
    world.draining.store(true, Ordering::SeqCst);
    assert!(!intent.step_delete(&elb).await.unwrap());
    assert_eq!(intent.arn.as_deref(), Some("owned-lb"));
    world.draining.store(false, Ordering::SeqCst);
    assert!(!intent.step_delete(&elb).await.unwrap());
    assert!(intent.arn.is_some());
    assert!(intent.step_delete(&elb).await.unwrap());
    assert_eq!(world.deletes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn denied_or_foreign_metadata_cannot_skip_remaining_parent_cleanup() {
    let world = Arc::new(World::default());
    let elb = client(world.clone());
    let mut lb = AwsVmLoadBalancer::default();
    lb.step_create(&elb, request("internal")).await.unwrap();
    lb.step_create(&elb, request("internal")).await.unwrap();
    world.deny_tags.store(true, Ordering::SeqCst);
    let error = lb.step_delete(&elb).await.unwrap_err();
    assert!(!matches!(error.http_status_code, Some(401 | 403 | 404)));
    assert!(error.source.is_none());
    world.deny_tags.store(false, Ordering::SeqCst);
    world
        .tags
        .lock()
        .unwrap()
        .iter_mut()
        .find(|tag| tag.key == "Owner")
        .unwrap()
        .value = "foreign".to_string();
    lb.step_create(&elb, request("internal")).await.unwrap_err();
    lb.step_delete(&elb).await.unwrap_err();
    assert_eq!(world.creates.load(Ordering::SeqCst), 1);
    assert_eq!(world.deletes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn access_and_network_changes_cannot_replace_a_retained_frontend() {
    let world = Arc::new(World::default());
    let elb = client(world.clone());
    let mut lb = AwsVmLoadBalancer::default();
    lb.step_create(&elb, request("internal")).await.unwrap();
    lb.step_create(&elb, request("internal")).await.unwrap();
    lb.step_create(&elb, request("internet-facing"))
        .await
        .unwrap_err();
    let mut changed = request("internal");
    changed.subnets = vec!["subnet-other".to_string()];
    lb.step_create(&elb, changed).await.unwrap_err();
    world.lb.lock().unwrap().as_mut().unwrap()["Scheme"] = json!("internet-facing");
    lb.step_create(&elb, request("internal")).await.unwrap_err();
    assert_eq!(world.creates.load(Ordering::SeqCst), 1);
    assert_eq!(world.deletes.load(Ordering::SeqCst), 0);
}

fn listener_request(certificate: Option<&str>) -> CreateListenerRequest {
    let mut request = CreateListenerRequest::builder()
        .load_balancer_arn("owned-lb".to_string())
        .port(if certificate.is_some() { 443 } else { 8080 })
        .protocol(
            if certificate.is_some() {
                "HTTPS"
            } else {
                "TCP"
            }
            .to_string(),
        )
        .default_actions(vec![Action::builder()
            .action_type("forward".to_string())
            .target_group_arn("owned-tg".to_string())
            .build()])
        .build();
    request.certificates = certificate.map(|arn| {
        vec![Certificate {
            certificate_arn: arn.to_string(),
            is_default: None,
        }]
    });
    request
}

async fn active_lb(world: &World, elb: &MockElbv2Api) -> AwsVmLoadBalancer {
    let mut lb = AwsVmLoadBalancer::default();
    lb.step_create(elb, request("internal")).await.unwrap();
    lb.step_create(elb, request("internal")).await.unwrap();
    world.lb.lock().unwrap().as_mut().unwrap()["State"]["Code"] = json!("active");
    assert!(lb
        .step_create(elb, request("internal"))
        .await
        .unwrap()
        .is_some());
    lb
}

#[tokio::test]
async fn listener_create_and_certificate_rotation_resume_with_same_target_and_listener() {
    let world = Arc::new(World::default());
    let elb = client(world.clone());
    let mut lb = active_lb(&world, &elb).await;
    lb.step_listener(&elb, listener_request(Some("old-cert")))
        .await
        .unwrap();
    let intent = reload(&lb);
    world.lose_listener_create.store(true, Ordering::SeqCst);
    lb.step_listener(&elb, listener_request(Some("old-cert")))
        .await
        .unwrap_err();
    let mut lb = intent;
    assert!(lb
        .step_listener(&elb, listener_request(Some("old-cert")))
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        lb.step_listener(&elb, listener_request(Some("old-cert")))
            .await
            .unwrap()
            .as_deref(),
        Some("owned-listener")
    );
    assert_eq!(lb.serving_certificate_arns(), ["old-cert"]);
    let before_rotation = reload(&lb);
    world.lose_listener_update.store(true, Ordering::SeqCst);
    lb.step_listener(&elb, listener_request(Some("new-cert")))
        .await
        .unwrap_err();
    assert_eq!(
        lb.serving_certificate_arns(),
        ["old-cert"],
        "retain old serving reference through an uncertain update"
    );
    let mut lb = before_rotation;
    assert_eq!(
        lb.step_listener(&elb, listener_request(Some("new-cert")))
            .await
            .unwrap()
            .as_deref(),
        Some("owned-listener")
    );
    assert_eq!(lb.serving_certificate_arns(), ["new-cert"]);
    assert_eq!(world.listener_creates.load(Ordering::SeqCst), 1);
    assert_eq!(world.listener_updates.load(Ordering::SeqCst), 1);
    assert_eq!(
        world.listener.lock().unwrap().as_ref().unwrap()["DefaultActions"]["member"][0]
            ["TargetGroupArn"],
        "owned-tg"
    );
}

#[tokio::test]
async fn abandoned_listener_create_is_retired_with_its_owned_load_balancer() {
    for certificate in [None, Some("cert")] {
        let world = Arc::new(World::default());
        let elb = client(world.clone());
        let mut lb = active_lb(&world, &elb).await;
        lb.step_listener(&elb, listener_request(certificate))
            .await
            .unwrap();
        let mut intent = reload(&lb);
        world.lose_listener_create.store(true, Ordering::SeqCst);
        lb.step_listener(&elb, listener_request(certificate))
            .await
            .unwrap_err();
        assert!(!intent.step_delete(&elb).await.unwrap());
        assert!(intent.step_delete(&elb).await.unwrap());
        assert!(world.listener.lock().unwrap().is_none());
        assert!(intent.serving_certificate_arns().is_empty());
        assert_eq!(world.listener_creates.load(Ordering::SeqCst), 1);
        assert_eq!(world.deletes.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn foreign_or_redirected_listener_is_never_adopted_or_rotated() {
    let world = Arc::new(World::default());
    let elb = client(world.clone());
    let mut lb = active_lb(&world, &elb).await;
    lb.step_listener(&elb, listener_request(Some("cert")))
        .await
        .unwrap();
    lb.step_listener(&elb, listener_request(Some("cert")))
        .await
        .unwrap();
    world
        .listener_tags
        .lock()
        .unwrap()
        .iter_mut()
        .find(|tag| tag.key == "Owner")
        .unwrap()
        .value = "foreign".to_string();
    lb.step_listener(&elb, listener_request(Some("new-cert")))
        .await
        .unwrap_err();
    world
        .listener_tags
        .lock()
        .unwrap()
        .iter_mut()
        .find(|tag| tag.key == "Owner")
        .unwrap()
        .value = "test-resource".to_string();
    world.listener.lock().unwrap().as_mut().unwrap()["DefaultActions"]["member"][0]
        ["ForwardConfig"] = json!({"TargetGroups":{"member":[{"TargetGroupArn":"owned-tg"},{"TargetGroupArn":"foreign-tg"}]}});
    lb.step_listener(&elb, listener_request(Some("new-cert")))
        .await
        .unwrap_err();
    assert_eq!(world.listener_creates.load(Ordering::SeqCst), 1);
    assert_eq!(world.listener_updates.load(Ordering::SeqCst), 0);
    assert_eq!(world.deletes.load(Ordering::SeqCst), 0);
}
