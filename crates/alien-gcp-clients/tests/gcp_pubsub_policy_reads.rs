use alien_gcp_clients::pubsub::{PubSubApi, PubSubClient};
use alien_gcp_clients::{GcpClientConfig, GcpCredentials, ServiceOverrides};
use httpmock::prelude::*;
use std::collections::HashMap;

#[tokio::test]
async fn topic_policy_read_requests_conditions_and_preserves_etag() {
    let server = MockServer::start_async().await;
    let response = server.mock_async(|when, then| {
        when.method(GET).path("/projects/test/topics/owned:getIamPolicy")
            .query_param("options.requestedPolicyVersion", "3")
            .header("authorization", "Bearer test-token");
        then.status(200).json_body_obj(&serde_json::json!({
            "version":3,"etag":"saved-etag","bindings":[{"role":"roles/pubsub.viewer","members":["group:operators@example.test"],"condition":{"title":"expires","expression":"request.time < timestamp('2030-01-01T00:00:00Z')"}}]
        }));
    }).await;
    let client = PubSubClient::new(
        reqwest::Client::new(),
        GcpClientConfig {
            project_id: "test".into(),
            region: "us-central1".into(),
            project_number: None,
            credentials: GcpCredentials::AccessToken {
                token: "test-token".into(),
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("pubsub".into(), server.base_url())]),
            }),
        },
    );
    let policy = client.get_topic_iam_policy("owned".into()).await.unwrap();
    response.assert_async().await;
    assert_eq!(policy.version, Some(3));
    assert_eq!(policy.etag.as_deref(), Some("saved-etag"));
    assert_eq!(
        policy.bindings[0]
            .condition
            .as_ref()
            .unwrap()
            .title
            .as_deref(),
        Some("expires")
    );
}
