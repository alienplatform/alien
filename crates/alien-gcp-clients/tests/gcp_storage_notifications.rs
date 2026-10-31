use alien_gcp_clients::{
    gcs::{GcsApi, GcsClient, GcsNotification},
    GcpClientConfig, GcpCredentials, ServiceOverrides,
};
use httpmock::prelude::*;
use serde_json::json;
use std::collections::HashMap;

fn client(server: &MockServer) -> GcsClient {
    GcsClient::new(
        reqwest::Client::new(),
        GcpClientConfig {
            project_id: "test-project".into(),
            region: "us-central1".into(),
            credentials: GcpCredentials::AccessToken {
                token: "test-token".into(),
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("storage".into(), server.base_url())]),
            }),
            project_number: None,
        },
    )
}

#[tokio::test]
async fn notification_create_and_list_preserve_provider_filters_and_attributes() {
    let server = MockServer::start_async().await;
    let wire = json!({
        "topic": "projects/test-project/topics/work",
        "event_types": ["OBJECT_FINALIZE"],
        "payload_format": "JSON_API_V1",
        "object_name_prefix": "in/é*",
        "custom_attributes": {"storageEventSuffixes0": "[\".json\"]"}
    });
    let mut response = wire.clone();
    response["id"] = json!("7");
    let insert = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/b/source/notificationConfigs")
                .header("authorization", "Bearer test-token")
                .json_body(wire);
            then.status(200).json_body(response.clone());
        })
        .await;
    let list = server
        .mock_async(|when, then| {
            when.method(GET).path("/b/source/notificationConfigs");
            then.status(200)
                .json_body(json!({"items": [response.clone()]}));
        })
        .await;
    let client = client(&server);
    let created = client
        .insert_notification(
            "source".into(),
            GcsNotification {
                topic: Some("projects/test-project/topics/work".into()),
                event_types: vec!["OBJECT_FINALIZE".into()],
                payload_format: Some("JSON_API_V1".into()),
                object_name_prefix: Some("in/é*".into()),
                custom_attributes: HashMap::from([(
                    "storageEventSuffixes0".into(),
                    r#"[".json"]"#.into(),
                )]),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(created).unwrap(), response);
    let listed = client.list_notifications("source".into()).await.unwrap();
    assert_eq!(listed.items.len(), 1);
    assert_eq!(serde_json::to_value(&listed.items[0]).unwrap(), response);
    insert.assert_async().await;
    list.assert_async().await;
}

#[tokio::test]
async fn notification_insert_does_not_retry_an_ambiguous_failure() {
    let server = MockServer::start_async().await;
    let insert = server
        .mock_async(|when, then| {
            when.method(POST).path("/b/source/notificationConfigs");
            then.status(503)
                .json_body(json!({"error": {"code": 503, "message": "unavailable"}}));
        })
        .await;
    client(&server)
        .insert_notification("source".into(), GcsNotification::default())
        .await
        .expect_err("caller must reconcile before retrying a server-assigned ID");
    insert.assert_hits_async(1).await;
}
