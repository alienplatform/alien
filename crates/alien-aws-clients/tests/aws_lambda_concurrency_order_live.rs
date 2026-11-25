//! Opt-in live probe: Lambda refuses an SQS event source mapping whose MaximumConcurrency is above
//! the function's reserved concurrency. The caller provides a function image, an execution role and
//! an SQS queue ARN through the environment, and owns teardown of the queue and role.
use alien_aws_clients::lambda::*;
use alien_aws_clients::{AwsClientConfig, AwsClientConfigExt, AwsCredentialProvider};
use alien_client_core::ErrorData;
use reqwest::Client;

fn mapping_update(max: i32) -> UpdateEventSourceMappingRequest {
    UpdateEventSourceMappingRequest::builder()
        .scaling_config(ScalingConfig::builder().maximum_concurrency(max).build())
        .build()
}

/// Lambda refuses updates while a mapping is still changing state.
async fn wait_enabled(client: &LambdaClient, uuid: &str) {
    for _ in 0..60 {
        let mapping = client.get_event_source_mapping(uuid).await.unwrap();
        if mapping.state.as_deref() == Some("Enabled") {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    panic!("mapping {uuid} did not reach Enabled");
}

#[tokio::test]
#[ignore = "requires AWS credentials, a function image, an execution role and an SQS queue"]
async fn live_concurrency_and_mapping_order() {
    let image = std::env::var("PROBE_IMAGE").unwrap();
    let role = std::env::var("PROBE_ROLE_ARN").unwrap();
    let queue_arn = std::env::var("PROBE_QUEUE_ARN").unwrap();
    let name = std::env::var("PROBE_FUNCTION_NAME").unwrap();
    let config = AwsClientConfig::from_std_env().await.unwrap();
    let client = LambdaClient::new(Client::new(), AwsCredentialProvider::from_config_sync(config));

    client
        .create_function(
            CreateFunctionRequest::builder()
                .function_name(name.clone())
                .role(role)
                .code(FunctionCode::builder().image_uri(image).build())
                .timeout(30)
                .memory_size(128)
                .architectures(vec!["arm64".to_string()])
                .build(),
        )
        .await
        .unwrap();
    for _ in 0..30 {
        let state = client.get_function_configuration(&name, None).await.unwrap();
        if state.state.as_deref() == Some("Active") {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }

    // The body runs in its own task so a failed assertion cannot skip the cleanup below.
    let body_client = client.clone();
    let body_name = name.clone();
    let result = tokio::spawn(async move {
        let client = body_client;
        let name = body_name;
        client.put_function_concurrency(&name, 2).await.unwrap();
        let mapping = client
            .create_event_source_mapping(
                CreateEventSourceMappingRequest::builder()
                    .event_source_arn(queue_arn)
                    .function_name(name.clone())
                    .batch_size(100)
                    .maximum_batching_window_in_seconds(10)
                    .scaling_config(ScalingConfig::builder().maximum_concurrency(2).build())
                    .build(),
            )
            .await
            .unwrap();
        let uuid = mapping.uuid.clone().unwrap();
        wait_enabled(&client, &uuid).await;
        println!("created mapping {uuid} at concurrency 2");

        // The old order for a raise: mapping first. AWS refuses and the client must not retry it.
        let refused = client
            .update_event_source_mapping(&uuid, mapping_update(16))
            .await
            .expect_err("AWS must refuse a mapping above the reserved concurrency");
        println!("old order refused: {:?}", refused.error);
        assert!(
            matches!(refused.error, Some(ErrorData::InvalidInput { .. })),
            "{refused:?}"
        );
        assert!(!refused.retryable);

        // The new order for a raise: concurrency first, then the mapping.
        client.put_function_concurrency(&name, 16).await.unwrap();
        wait_enabled(&client, &uuid).await;
        client
            .update_event_source_mapping(&uuid, mapping_update(16))
            .await
            .unwrap();
        wait_enabled(&client, &uuid).await;
        println!("raise 2 -> 16 converged (concurrency, then mapping)");

        // The new order for a lowering: mapping first, then concurrency.
        client
            .update_event_source_mapping(&uuid, mapping_update(2))
            .await
            .unwrap();
        wait_enabled(&client, &uuid).await;
        client.put_function_concurrency(&name, 2).await.unwrap();
        println!("lower 16 -> 2 converged (mapping, then concurrency)");

        // The old order for a lowering (concurrency first) is what the fix avoids.
        client
            .update_event_source_mapping(&uuid, mapping_update(16))
            .await
            .expect_err("still refused at reserved 2");
        wait_enabled(&client, &uuid).await;
        client.delete_event_source_mapping(&uuid).await.unwrap();
    })
    .await;
    client.delete_function(&name, None).await.ok();
    if let Err(error) = result {
        std::panic::resume_unwind(error.into_panic());
    }
}
