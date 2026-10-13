use std::sync::{Arc, Mutex};

use alien_aws_clients::elbv2::{ElbTag, MockElbv2Api};
use alien_client_core::ErrorData as ClientErrorData;
use serde_json::{json, Value};

use super::*;

#[derive(Default)]
struct World {
    group: Option<Value>,
    lb: Option<Value>,
    listener: Option<Value>,
    tags: std::collections::BTreeMap<String, Vec<ElbTag>>,
    mutations: Vec<&'static str>,
    lose_response: Option<&'static str>,
}

impl World {
    fn committed(&mut self, operation: &'static str) -> alien_client_core::Result<()> {
        self.mutations.push(operation);
        if self.lose_response == Some(operation) {
            self.lose_response = None;
            return Err(AlienError::new(ClientErrorData::Timeout {
                message: "provider committed mutation but response was lost".to_string(),
            }));
        }
        Ok(())
    }
}

fn client(world: Arc<Mutex<World>>) -> MockElbv2Api {
    let mut elb = MockElbv2Api::new();
    let state = world.clone();
    elb.expect_describe_tags().returning(move |arns| {
        assert_eq!(arns.len(), 1);
        let state = state.lock().unwrap();
        let tags: Vec<Value> = state.tags.get(&arns[0]).unwrap().iter().map(|tag| json!({"Key":tag.key,"Value":tag.value})).collect();
        Ok(serde_json::from_value(json!({"DescribeTagsResult":{"TagDescriptions":{"member":[{"ResourceArn":arns[0],"Tags":{"member":tags}}]}}})).unwrap())
    });
    let state = world.clone();
    elb.expect_describe_target_groups().returning(move |request| {
        let state = state.lock().unwrap();
        if let Some(group) = &state.group {
            if let Some(names) = &request.names {
                assert_eq!(names[0], group["TargetGroupName"].as_str().unwrap());
            } else {
                assert_eq!(request.target_group_arns.as_ref().unwrap(), &["group"]);
            }
        }
        Ok(serde_json::from_value(json!({"DescribeTargetGroupsResult":{"TargetGroups":{"member":state.group.iter().cloned().collect::<Vec<_>>()}}})).unwrap())
    });
    let state = world.clone();
    elb.expect_create_target_group().returning(move |request| {
        let mut state = state.lock().unwrap();
        assert!(state.group.is_none());
        let group = json!({"TargetGroupArn":"group","TargetGroupName":request.name,"Protocol":request.protocol,"Port":request.port,"VpcId":request.vpc_id,"TargetType":request.target_type});
        state.group = Some(group.clone());
        state.tags.insert("group".to_string(), request.tags.unwrap());
        state.committed("create-group")?;
        Ok(serde_json::from_value(json!({"CreateTargetGroupResult":{"TargetGroups":{"member":[group]}}})).unwrap())
    });
    let state = world.clone();
    elb.expect_describe_load_balancers().returning(move |request| {
        let state = state.lock().unwrap();
        if let Some(lb) = &state.lb {
            if let Some(names) = &request.names {
                assert_eq!(names[0], lb["LoadBalancerName"].as_str().unwrap());
            } else {
                assert_eq!(request.load_balancer_arns.as_ref().unwrap(), &["lb"]);
            }
        }
        Ok(serde_json::from_value(json!({"DescribeLoadBalancersResult":{"LoadBalancers":{"member":state.lb.iter().cloned().collect::<Vec<_>>()}}})).unwrap())
    });
    let state = world.clone();
    elb.expect_create_load_balancer().returning(move |request| {
        let mut state = state.lock().unwrap();
        assert!(state.group.is_some());
        assert!(state.lb.is_none());
        let zones: Vec<Value> = request.subnets.iter().map(|subnet| json!({"SubnetId":subnet})).collect();
        let lb = json!({"LoadBalancerArn":"lb","LoadBalancerName":request.name,"Type":request.load_balancer_type,"Scheme":request.scheme,"IpAddressType":request.ip_address_type,"AvailabilityZones":{"member":zones},"SecurityGroups":{"member":request.security_groups.unwrap_or_default()},"State":{"Code":"active"},"DNSName":"lb.example.test","CanonicalHostedZoneId":"zone"});
        state.lb = Some(lb.clone());
        state.tags.insert("lb".to_string(), request.tags.unwrap());
        state.committed("create-lb")?;
        Ok(serde_json::from_value(json!({"CreateLoadBalancerResult":{"LoadBalancers":{"member":[lb]}}})).unwrap())
    });
    let state = world.clone();
    elb.expect_describe_listeners().returning(move |request| {
        if let Some(arns) = &request.listener_arns {
            assert_eq!(arns, &["listener"]);
        } else {
            assert_eq!(request.load_balancer_arn.as_deref(), Some("lb"));
        }
        let state = state.lock().unwrap();
        Ok(serde_json::from_value(json!({"DescribeListenersResult":{"Listeners":{"member":state.listener.iter().cloned().collect::<Vec<_>>()}}})).unwrap())
    });
    let state = world.clone();
    elb.expect_create_listener().returning(move |request| {
        let mut state = state.lock().unwrap();
        assert!(state.listener.is_none());
        assert_eq!(request.load_balancer_arn, "lb");
        assert_eq!(request.default_actions[0].target_group_arn.as_deref(), Some("group"));
        let certs: Vec<Value> = request.certificates.unwrap_or_default().iter().map(|certificate| json!({"CertificateArn":certificate.certificate_arn})).collect();
        let listener = json!({"ListenerArn":"listener","LoadBalancerArn":"lb","Port":request.port,"Protocol":request.protocol,"Certificates":{"member":certs},"DefaultActions":{"member":[{"Type":"forward","TargetGroupArn":"group"}]}});
        state.listener = Some(listener.clone());
        state.tags.insert("listener".to_string(), request.tags.unwrap());
        state.committed("create-listener")?;
        Ok(serde_json::from_value(json!({"CreateListenerResult":{"Listeners":{"member":[listener]}}})).unwrap())
    });
    let state = world.clone();
    elb.expect_modify_listener().returning(move |request| {
        assert_eq!(request.listener_arn, "listener");
        assert!(request.default_actions.is_none());
        assert!(request.port.is_none() && request.protocol.is_none());
        let certs: Vec<Value> = request.certificates.unwrap().iter().map(|certificate| json!({"CertificateArn":certificate.certificate_arn})).collect();
        let mut state = state.lock().unwrap();
        state.listener.as_mut().unwrap()["Certificates"] = json!({"member":certs});
        state.committed("rotate-listener")?;
        Ok(serde_json::from_value(json!({"ModifyListenerResult":{"Listeners":{"member":[state.listener.as_ref().unwrap()]}}})).unwrap())
    });
    let state = world.clone();
    elb.expect_delete_load_balancer().returning(move |arn| {
        assert_eq!(arn, "lb");
        let mut state = state.lock().unwrap();
        assert!(state.lb.take().is_some());
        state.listener = None;
        state.committed("delete-lb")
    });
    elb.expect_delete_target_group().returning(move |arn| {
        assert_eq!(arn, "group");
        let mut state = world.lock().unwrap();
        assert!(
            state.lb.is_none(),
            "load balancer must be confirmed gone first"
        );
        assert!(state.group.take().is_some());
        state.committed("delete-group")
    });
    elb
}

fn plan(tcp: bool, private: bool) -> AwsVmFrontendPlan {
    let tags = vec![ElbTag {
        key: "Owner".to_string(),
        value: "test-resource".to_string(),
    }];
    AwsVmFrontendPlan {
        target_group: CreateTargetGroupRequest::builder()
            .name("frontend".to_string())
            .protocol(if tcp { "TCP" } else { "HTTP" }.to_string())
            .port(8080)
            .vpc_id("vpc".to_string())
            .tags(tags.clone())
            .build(),
        load_balancer: CreateLoadBalancerRequest::builder()
            .name("frontend".to_string())
            .scheme(
                if private {
                    "internal"
                } else {
                    "internet-facing"
                }
                .to_string(),
            )
            .subnets(vec!["subnet-a".to_string(), "subnet-b".to_string()])
            .security_groups(vec!["shared-sg".to_string()])
            .load_balancer_type(if tcp { "network" } else { "application" }.to_string())
            .tags(tags)
            .build(),
        listener_port: if tcp { 8080 } else { 443 },
        certificate_arn: (!tcp).then(|| "cert".to_string()),
        ssl_policy: None,
    }
}

fn reload(frontend: &AwsVmFrontend) -> AwsVmFrontend {
    serde_json::from_value(serde_json::to_value(frontend).unwrap()).unwrap()
}

#[tokio::test]
async fn incomplete_setup_or_ownership_is_rejected_before_any_child_intent_or_api_call() {
    for invalid_case in 0..4 {
        let elb = MockElbv2Api::new(); // Every provider operation is unexpected.
        let mut frontend = AwsVmFrontend::default();
        let mut desired = plan(false, false);
        match invalid_case {
            0 => desired.load_balancer.subnets.clear(),
            1 => desired.load_balancer.scheme = None,
            2 => desired.load_balancer.tags = None,
            _ => desired.load_balancer.tags.as_mut().unwrap().push(ElbTag {
                key: "Owner".to_string(),
                value: "ambiguous".to_string(),
            }),
        }
        let before = serde_json::to_value(&frontend).unwrap();
        let error = frontend
            .step_prepare(&elb, desired)
            .await
            .err()
            .expect("invalid plan must fail immediately");
        assert_eq!(error.code, "RESOURCE_CONFIG_INVALID");
        assert_eq!(
            serde_json::to_value(&frontend).unwrap(),
            before,
            "invalid setup must not begin child ownership"
        );
    }
}

#[tokio::test]
async fn every_preparation_checkpoint_can_resume_or_delete_with_one_mutation_per_step() {
    for tcp in [false, true] {
        for private in [false, true] {
            for checkpoint in 0..=7 {
                let world = Arc::new(Mutex::new(World::default()));
                let elb = client(world.clone());
                let mut frontend = AwsVmFrontend::default();
                for _ in 0..checkpoint {
                    let before = world.lock().unwrap().mutations.len();
                    frontend
                        .step_prepare(&elb, plan(tcp, private))
                        .await
                        .unwrap();
                    assert!(world.lock().unwrap().mutations.len() - before <= 1);
                    frontend = reload(&frontend);
                }
                let mut deleted = false;
                for _ in 0..10 {
                    let before = world.lock().unwrap().mutations.len();
                    deleted = frontend.step_delete(&elb).await.unwrap();
                    assert!(world.lock().unwrap().mutations.len() - before <= 1);
                    frontend = reload(&frontend);
                    if deleted {
                        break;
                    }
                }
                assert!(deleted, "cleanup did not finish at checkpoint {checkpoint}");
                let state = world.lock().unwrap();
                assert!(state.group.is_none() && state.lb.is_none() && state.listener.is_none());
            }
        }
    }
}

#[tokio::test]
async fn partial_child_progress_survives_lost_create_responses_without_duplicate_resources() {
    for operation in ["create-group", "create-lb", "create-listener"] {
        let world = Arc::new(Mutex::new(World {
            lose_response: Some(operation),
            ..World::default()
        }));
        let elb = client(world.clone());
        let mut frontend = AwsVmFrontend::default();
        let mut ready = None;
        let mut failures = 0;
        for _ in 0..15 {
            let saved = reload(&frontend);
            let before = world.lock().unwrap().mutations.len();
            let result = frontend.step_prepare(&elb, plan(false, true)).await;
            assert!(world.lock().unwrap().mutations.len() - before <= 1);
            match result {
                Ok(Some(prepared)) => {
                    ready = Some(prepared);
                    break;
                }
                Ok(None) => frontend = reload(&frontend),
                Err(_) => {
                    failures += 1;
                    frontend = saved;
                }
            }
        }
        let prepared = ready.expect("frontend preparation did not finish");
        assert_eq!(failures, 1);
        assert!(world.lock().unwrap().lose_response.is_none());
        assert_eq!(prepared.target_group_arn, "group");
        assert_eq!(prepared.load_balancer_arn, "lb");
        assert_eq!(prepared.listener_arn, "listener");
        assert_eq!(prepared.endpoint.dns_name, "lb.example.test");
        assert_eq!(frontend.serving_certificate_arns(), ["cert"]);
        assert_eq!(
            world.lock().unwrap().mutations,
            ["create-group", "create-lb", "create-listener"]
        );
    }
}

#[tokio::test]
async fn lost_deletes_resume_the_remaining_children_without_repeating_mutations() {
    for operation in ["delete-lb", "delete-group"] {
        let world = Arc::new(Mutex::new(World::default()));
        let elb = client(world.clone());
        let mut frontend = AwsVmFrontend::default();
        let mut prepared = false;
        for _ in 0..10 {
            prepared = frontend
                .step_prepare(&elb, plan(false, false))
                .await
                .unwrap()
                .is_some();
            if prepared {
                break;
            }
        }
        assert!(prepared);
        world.lock().unwrap().lose_response = Some(operation);
        let mut deleted = false;
        let mut failures = 0;
        for _ in 0..10 {
            let saved = reload(&frontend);
            let before = world.lock().unwrap().mutations.len();
            let result = frontend.step_delete(&elb).await;
            assert!(world.lock().unwrap().mutations.len() - before <= 1);
            match result {
                Ok(done) => {
                    deleted = done;
                    frontend = reload(&frontend);
                }
                Err(_) => {
                    failures += 1;
                    frontend = saved;
                }
            }
            if deleted {
                break;
            }
        }
        assert!(deleted);
        assert_eq!(failures, 1);
        let state = world.lock().unwrap();
        assert!(state.group.is_none() && state.lb.is_none() && state.listener.is_none());
        assert_eq!(
            state.mutations,
            [
                "create-group",
                "create-lb",
                "create-listener",
                "delete-lb",
                "delete-group"
            ]
        );
    }
}

fn recorded_receipt() -> AwsVmFrontendReceipt {
    AwsVmFrontendReceipt {
        target_group_arn: "group".to_string(),
        load_balancer_arn: "lb".to_string(),
        listener_arn: "listener".to_string(),
        ownership_tags: std::collections::BTreeMap::from([(
            "Owner".to_string(),
            "test-resource".to_string(),
        )]),
    }
}

async fn prepare_existing(elb: &MockElbv2Api, tcp: bool, private: bool) {
    let mut existing = AwsVmFrontend::default();
    let mut prepared = false;
    for _ in 0..10 {
        prepared = existing
            .step_prepare(elb, plan(tcp, private))
            .await
            .unwrap()
            .is_some();
        if prepared {
            break;
        }
    }
    assert!(prepared);
}

#[tokio::test]
async fn recorded_frontend_without_attempt_tags_keeps_exact_provider_ids_and_can_retire() {
    for tcp in [false, true] {
        for private in [false, true] {
            let world = Arc::new(Mutex::new(World::default()));
            let elb = client(world.clone());
            prepare_existing(&elb, tcp, private).await;
            for tags in world.lock().unwrap().tags.values_mut() {
                tags.retain(|tag| tag.key != "CreateAttempt");
            }
            let mut recorded = AwsVmFrontend::from_recorded(recorded_receipt()).unwrap();
            let mut prepared = None;
            for _ in 0..3 {
                prepared = recorded
                    .step_prepare(&elb, plan(tcp, private))
                    .await
                    .unwrap();
                recorded = reload(&recorded);
                if prepared.is_some() {
                    break;
                }
            }
            let prepared = prepared.expect("recorded frontend did not confirm");
            assert_eq!(prepared.target_group_arn, "group");
            assert_eq!(prepared.load_balancer_arn, "lb");
            assert_eq!(prepared.listener_arn, "listener");
            assert_eq!(
                world.lock().unwrap().mutations,
                ["create-group", "create-lb", "create-listener"]
            );
            let mut deleted = false;
            for _ in 0..8 {
                let before = world.lock().unwrap().mutations.len();
                deleted = recorded.step_delete(&elb).await.unwrap();
                assert!(world.lock().unwrap().mutations.len() - before <= 1);
                recorded = reload(&recorded);
                if deleted {
                    break;
                }
            }
            assert!(deleted);
            recorded
                .step_prepare(&elb, plan(tcp, private))
                .await
                .err()
                .expect("retired recorded IDs must never be recreated");
            assert_eq!(
                world.lock().unwrap().mutations,
                [
                    "create-group",
                    "create-lb",
                    "create-listener",
                    "delete-lb",
                    "delete-group"
                ]
            );
        }
    }
}

#[tokio::test]
async fn recorded_listener_renewal_checkpoints_serving_certificate_before_lost_update_response() {
    let world = Arc::new(Mutex::new(World::default()));
    let elb = client(world.clone());
    prepare_existing(&elb, false, true).await;
    let mut recorded = AwsVmFrontend::from_recorded(recorded_receipt()).unwrap();
    let mut desired = plan(false, true);
    desired.certificate_arn = Some("renewed-cert".to_string());
    assert!(recorded
        .step_prepare(&elb, desired)
        .await
        .unwrap()
        .is_none());
    assert_eq!(recorded.serving_certificate_arns(), ["cert"]);
    let saved = reload(&recorded);
    world.lock().unwrap().lose_response = Some("rotate-listener");
    let mut desired = plan(false, true);
    desired.certificate_arn = Some("renewed-cert".to_string());
    recorded
        .step_prepare(&elb, desired)
        .await
        .err()
        .expect("injected response loss");
    let mut recorded = saved;
    assert_eq!(recorded.serving_certificate_arns(), ["cert"]);
    let mut desired = plan(false, true);
    desired.certificate_arn = Some("renewed-cert".to_string());
    assert!(recorded
        .step_prepare(&elb, desired)
        .await
        .unwrap()
        .is_none());
    assert_eq!(recorded.serving_certificate_arns(), ["renewed-cert"]);
    let mut desired = plan(false, true);
    desired.certificate_arn = Some("renewed-cert".to_string());
    assert_eq!(
        recorded
            .step_prepare(&elb, desired)
            .await
            .unwrap()
            .unwrap()
            .listener_arn,
        "listener"
    );
    assert_eq!(
        world.lock().unwrap().mutations,
        [
            "create-group",
            "create-lb",
            "create-listener",
            "rotate-listener"
        ]
    );
}

#[tokio::test]
async fn recorded_frontend_refuses_foreign_ownership_and_changed_access_before_mutation() {
    let world = Arc::new(Mutex::new(World::default()));
    let elb = client(world.clone());
    prepare_existing(&elb, false, false).await;
    let mut recorded = AwsVmFrontend::from_recorded(recorded_receipt()).unwrap();
    recorded
        .step_prepare(&elb, plan(false, true))
        .await
        .err()
        .expect("same LB cannot change public/private scheme");
    world
        .lock()
        .unwrap()
        .tags
        .get_mut("lb")
        .unwrap()
        .iter_mut()
        .find(|tag| tag.key == "Owner")
        .unwrap()
        .value = "foreign".to_string();
    recorded
        .step_prepare(&elb, plan(false, false))
        .await
        .err()
        .expect("foreign LB must not be adopted");
    let error = recorded.step_delete(&elb).await.unwrap_err();
    assert!(!matches!(error.http_status_code, Some(401 | 403 | 404)));
    assert!(error.source.is_none());
    assert_eq!(
        world.lock().unwrap().mutations,
        ["create-group", "create-lb", "create-listener"]
    );
}
