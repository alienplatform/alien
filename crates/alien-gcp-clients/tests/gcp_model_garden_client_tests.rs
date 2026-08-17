use std::collections::HashMap;

use alien_gcp_clients::{
    GcpClientConfig, GcpCredentials, ModelGardenApi, ModelGardenClient, ServiceOverrides,
};
use httpmock::prelude::*;

fn client(server: &MockServer) -> ModelGardenClient {
    ModelGardenClient::new(
        reqwest::Client::new(),
        GcpClientConfig {
            project_id: "test-project".to_string(),
            region: "us-central1".to_string(),
            credentials: GcpCredentials::AccessToken {
                token: "test-token".to_string(),
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("aiplatform".to_string(), server.base_url())]),
            }),
            project_number: None,
        },
    )
}

#[tokio::test]
async fn lists_all_publisher_model_pages_with_bearer_auth() {
    let server = MockServer::start_async().await;
    let first = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/publishers/google/models")
                .query_param("pageSize", "1000")
                .matches(|request: &HttpMockRequest| {
                    request.query_params.as_ref().is_none_or(|parameters| {
                        parameters.iter().all(|(name, _)| name != "pageToken")
                    })
                })
                .header("authorization", "Bearer test-token");
            then.status(200).json_body_obj(&serde_json::json!({
                "publisherModels": [{"name": "publishers/google/models/gemini-1"}],
                "nextPageToken": "next"
            }));
        })
        .await;
    let second = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/publishers/google/models")
                .query_param("pageSize", "1000")
                .query_param("pageToken", "next")
                .header("authorization", "Bearer test-token");
            then.status(200).json_body_obj(&serde_json::json!({
                "publisherModels": [{"name": "publishers/google/models/gemini-2"}]
            }));
        })
        .await;

    let models = client(&server)
        .list_publisher_models("google")
        .await
        .expect("publisher models");

    first.assert_async().await;
    second.assert_async().await;
    assert_eq!(models.len(), 2);
}

#[tokio::test]
async fn checks_project_eula_without_mutating_it() {
    let server = MockServer::start_async().await;
    let request = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/projects/test-project/modelGardenEula:check")
                .header("authorization", "Bearer test-token")
                .json_body_obj(&serde_json::json!({
                    "publisherModel": "publishers/anthropic/models/claude-sonnet"
                }));
            then.status(200).json_body_obj(&serde_json::json!({
                "publisherModel": "publishers/anthropic/models/claude-sonnet",
                "publisherModelEulaAcked": false
            }));
        })
        .await;

    let result = client(&server)
        .check_publisher_model_eula("publishers/anthropic/models/claude-sonnet")
        .await
        .expect("EULA check");

    request.assert_async().await;
    assert!(!result.publisher_model_eula_acked);
}
