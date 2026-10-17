use alien_gcp_clients::gcs::{GcsApi, GcsClient, GcsNotification};
use alien_gcp_clients::{GcpClientConfig, GcpCredentials, ServiceOverrides};
use httpmock::prelude::*;
use std::collections::HashMap;

#[tokio::test]
async fn notification_create_list_and_delete_preserve_provider_fields() {
    let server = MockServer::start_async().await;
    let wire = serde_json::json!({
        "topic": "//pubsub.googleapis.com/projects/test/topics/owned",
        "event_types": ["OBJECT_FINALIZE"],
        "payload_format": "JSON_API_V1",
        "object_name_prefix": "incoming/",
        "custom_attributes": {"owner": "test-owner"}
    });
    let mut created = wire.clone();
    created["id"] = serde_json::json!("123");
    let insert = server.mock_async(|when, then| {
        when.method(POST).path("/b/test-bucket/notificationConfigs")
            .header("authorization", "Bearer test-token").json_body_obj(&wire);
        then.status(200).json_body_obj(&created);
    }).await;
    let list = server.mock_async(|when, then| {
        when.method(GET).path("/b/test-bucket/notificationConfigs");
        then.status(200).json_body_obj(&serde_json::json!({"items": [created]}));
    }).await;
    let delete = server.mock_async(|when, then| {
        when.method(DELETE).path("/b/test-bucket/notificationConfigs/123");
        then.status(204);
    }).await;
    let client = GcsClient::new(reqwest::Client::new(), GcpClientConfig {
        project_id: "test".into(), region: "us-central1".into(), project_number: None,
        credentials: GcpCredentials::AccessToken { token: "test-token".into() },
        service_overrides: Some(ServiceOverrides { endpoints: HashMap::from([("storage".into(), server.base_url())]) }),
    });
    let inserted = client.insert_notification("test-bucket".into(), GcsNotification {
        topic: Some("//pubsub.googleapis.com/projects/test/topics/owned".into()),
        event_types: vec!["OBJECT_FINALIZE".into()], payload_format: Some("JSON_API_V1".into()),
        object_name_prefix: Some("incoming/".into()), custom_attributes: HashMap::from([("owner".into(), "test-owner".into())]),
        ..Default::default()
    }).await.expect("provider accepts exact notification fields");
    insert.assert_async().await;
    assert_eq!(serde_json::to_value(&inserted).unwrap(), created);
    let listed = client.list_notifications("test-bucket".into()).await.unwrap();
    list.assert_async().await;
    assert_eq!(listed.items.len(), 1);
    assert_eq!(serde_json::to_value(&listed.items[0]).unwrap(), created);
    client.delete_notification("test-bucket".into(), listed.items[0].id.clone().unwrap()).await.unwrap();
    delete.assert_async().await;
}
