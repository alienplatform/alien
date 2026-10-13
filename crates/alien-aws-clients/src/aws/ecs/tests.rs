use std::collections::HashMap;

use alien_core::{AwsClientConfig, AwsCredentials};
use httpmock::prelude::*;
use serde_json::json;

use super::*;
use crate::ServiceOverrides;

fn client(server: &MockServer) -> EcsClient {
    EcsClient::new(
        Client::new(),
        AwsCredentialProvider::from_config_sync(AwsClientConfig {
            account_id: "123456789012".into(),
            region: "us-east-1".into(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: "test-access-key".into(),
                secret_access_key: "test-secret-key".into(),
                session_token: None,
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("ecs".into(), server.base_url())]),
            }),
        }),
    )
}

fn service_request() -> CreateServiceRequest {
    serde_json::from_value(json!({
        "cluster": "owned-cluster", "serviceName": "owned-service", "clientToken": "persisted-attempt",
        "taskDefinition": "arn:aws:ecs:us-east-1:123456789012:task-definition/owned:1",
        "desiredCount": 2, "launchType": "FARGATE", "platformVersion": "1.4.0",
        "networkConfiguration": {"awsvpcConfiguration": {"subnets": ["subnet-test"], "securityGroups": ["sg-test"], "assignPublicIp": "ENABLED"}},
        "loadBalancers": [{"targetGroupArn": "target", "containerName": "gateway", "containerPort": 8080}],
        "deploymentConfiguration": {"minimumHealthyPercent": 100, "maximumPercent": 200},
        "healthCheckGracePeriodSeconds": 60, "tags": [{"key": "CreateAttempt", "value": "persisted-attempt"}]
    })).unwrap()
}

#[tokio::test]
async fn create_service_signs_explicit_network_and_replays_the_recorded_token() {
    let server = MockServer::start_async().await;
    let request = service_request();
    let call = server.mock_async(|when, then| {
        when.method(POST).path("/")
            .header("x-amz-target", "AmazonEC2ContainerServiceV20141113.CreateService")
            .header("content-type", "application/x-amz-json-1.1")
            .header_exists("authorization")
            .json_body(serde_json::to_value(&request).unwrap());
        then.status(200).json_body(json!({"service": {"serviceArn": "service-arn", "serviceName": "owned-service", "status": "ACTIVE"}}));
    }).await;
    for _ in 0..2 {
        let response = client(&server)
            .create_service(request.clone())
            .await
            .unwrap();
        assert_eq!(response.service.service_arn, "service-arn");
        assert_eq!(response.service.status.as_deref(), Some("ACTIVE"));
    }
    call.assert_hits_async(2).await;
}

#[tokio::test]
async fn uncertain_task_definition_registration_is_never_retried_inside_the_sdk() {
    let server = MockServer::start_async().await;
    let call = server.mock_async(|when, then| {
        when.method(POST).path("/").header("x-amz-target", "AmazonEC2ContainerServiceV20141113.RegisterTaskDefinition")
            .body_contains("\"networkMode\":\"awsvpc\"")
            .body_contains("\"requiresCompatibilities\":[\"FARGATE\"]");
        then.status(503).json_body(json!({"__type": "com.amazonaws.ecs#ServerException", "message": "reply lost after registration"}));
    }).await;
    let request = RegisterTaskDefinitionRequest::builder()
        .family("owned".into())
        .container_definitions(vec![ContainerDefinition::builder()
            .name("gateway".into())
            .image("example@sha256:fixture".into())
            .build()])
        .network_mode("awsvpc".into())
        .requires_compatibilities(vec!["FARGATE".into()])
        .cpu("256".into())
        .memory("512".into())
        .runtime_platform(
            RuntimePlatform::builder()
                .cpu_architecture("X86_64".into())
                .operating_system_family("LINUX".into())
                .build(),
        )
        .tags(vec![Tag::builder()
            .key("CreateAttempt".into())
            .value("attempt".into())
            .build()])
        .build();
    assert_eq!(
        client(&server)
            .register_task_definition(request)
            .await
            .unwrap_err()
            .code,
        "REMOTE_SERVICE_UNAVAILABLE"
    );
    call.assert_hits_async(1).await;
}

#[tokio::test]
async fn describe_and_delete_preserve_batch_failures_in_successful_http_responses() {
    let server = MockServer::start_async().await;
    let describe = server.mock_async(|when, then| {
        when.method(POST).path("/").header("x-amz-target", "AmazonEC2ContainerServiceV20141113.DescribeServices")
            .json_body(json!({"cluster": "cluster", "services": ["service"], "include": ["TAGS"]}));
        then.status(200).json_body(json!({"services": [], "failures": [{"arn": "service", "reason": "MISSING", "detail": "not found"}]}));
    }).await;
    let response = client(&server)
        .describe_services(
            DescribeServicesRequest::builder()
                .cluster("cluster".into())
                .services(vec!["service".into()])
                .include(vec!["TAGS".into()])
                .build(),
        )
        .await
        .unwrap();
    assert!(response.services.is_empty());
    assert_eq!(response.failures.len(), 1);
    assert_eq!(response.failures[0].reason.as_deref(), Some("MISSING"));
    describe.assert_hits_async(1).await;
    let delete = server.mock_async(|when, then| {
        when.method(POST).path("/").header("x-amz-target", "AmazonEC2ContainerServiceV20141113.DeleteTaskDefinitions")
            .json_body(json!({"taskDefinitions": ["definition"]}));
        then.status(200).json_body(json!({"taskDefinitions": [], "failures": [{"arn": "definition", "reason": "RESOURCE_IN_USE", "detail": "active task"}]}));
    }).await;
    let response = client(&server)
        .delete_task_definitions(
            DeleteTaskDefinitionsRequest::builder()
                .task_definitions(vec!["definition".into()])
                .build(),
        )
        .await
        .unwrap();
    assert_eq!(response.failures[0].arn.as_deref(), Some("definition"));
    assert_eq!(
        response.failures[0].reason.as_deref(),
        Some("RESOURCE_IN_USE")
    );
    delete.assert_hits_async(1).await;
}

#[tokio::test]
async fn task_inventory_keeps_pagination_and_interface_identity() {
    let server = MockServer::start_async().await;
    let list = server.mock_async(|when, then| {
        when.method(POST).path("/").header("x-amz-target", "AmazonEC2ContainerServiceV20141113.ListTasks")
            .json_body(json!({"cluster": "cluster", "serviceName": "service", "nextToken": "page-one"}));
        then.status(200).json_body(json!({"taskArns": ["task"], "nextToken": "page-two"}));
    }).await;
    let response = client(&server)
        .list_tasks(
            ListTasksRequest::builder()
                .cluster("cluster".into())
                .service_name("service".into())
                .next_token("page-one".into())
                .build(),
        )
        .await
        .unwrap();
    assert_eq!(response.task_arns, vec!["task"]);
    assert_eq!(response.next_token.as_deref(), Some("page-two"));
    list.assert_hits_async(1).await;
    let describe = server.mock_async(|when, then| {
        when.method(POST).path("/").header("x-amz-target", "AmazonEC2ContainerServiceV20141113.DescribeTasks");
        then.status(200).json_body(json!({"tasks": [{"taskArn": "task", "lastStatus": "RUNNING", "taskDefinitionArn": "definition",
            "attachments": [{"type": "ElasticNetworkInterface", "status": "ATTACHED", "details": [{"name": "networkInterfaceId", "value": "eni-test"}]}],
            "tags": [{"key": "CreateAttempt", "value": "attempt"}]}], "failures": []}));
    }).await;
    let response = client(&server)
        .describe_tasks(
            DescribeTasksRequest::builder()
                .cluster("cluster".into())
                .tasks(vec!["task".into()])
                .include(vec!["TAGS".into()])
                .build(),
        )
        .await
        .unwrap();
    assert_eq!(response.tasks[0].last_status.as_deref(), Some("RUNNING"));
    assert_eq!(
        response.tasks[0].attachments[0].details[0].value,
        "eni-test"
    );
    assert_eq!(response.tasks[0].tags[0].value, "attempt");
    describe.assert_hits_async(1).await;
}

#[tokio::test]
async fn cleanup_errors_distinguish_denial_absence_dependency_drain_and_bad_requests() {
    for (native, expected) in [
        ("AccessDeniedException", "REMOTE_ACCESS_DENIED"),
        ("ClusterNotFoundException", "REMOTE_RESOURCE_NOT_FOUND"),
        ("ClusterContainsTasksException", "REMOTE_RESOURCE_CONFLICT"),
        ("InvalidParameterException", "INVALID_INPUT"),
    ] {
        let server = MockServer::start_async().await;
        let call = server
            .mock_async(|when, then| {
                when.method(POST).path("/").header(
                    "x-amz-target",
                    "AmazonEC2ContainerServiceV20141113.DeleteCluster",
                );
                then.status(400)
                    .json_body(json!({"__type": native, "message": "native reason"}));
            })
            .await;
        assert_eq!(
            client(&server)
                .delete_cluster(ClusterRequest::builder().cluster("owned".into()).build())
                .await
                .unwrap_err()
                .code,
            expected
        );
        call.assert_hits_async(1).await;
    }
}
