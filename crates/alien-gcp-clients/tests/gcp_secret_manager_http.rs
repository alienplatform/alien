use alien_gcp_clients::{
    secret_manager::{
        AddSecretVersionRequest, SecretManagerApi, SecretManagerClient, SecretPayload,
    },
    GcpClientConfig, GcpCredentials, ServiceOverrides,
};
use httpmock::prelude::*;
use std::collections::HashMap;

const SENTINEL: &str = "c2VjcmV0LWNhbmFyeS0wNzI4";

fn client(server: &MockServer) -> SecretManagerClient {
    SecretManagerClient::new(
        reqwest::Client::new(),
        GcpClientConfig {
            project_id: "test-project".into(),
            region: "us-central1".into(),
            credentials: GcpCredentials::AccessToken {
                token: "test-token".into(),
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("secretmanager".into(), server.base_url())]),
            }),
            project_number: None,
        },
    )
}

#[tokio::test]
async fn a_failed_add_version_never_records_the_secret() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/projects/test-project/secrets/s:addVersion");
            then.status(403).json_body(serde_json::json!({
                "error": {"code": 403, "message": "Permission 'secretmanager.versions.add' denied", "status": "PERMISSION_DENIED"}
            }));
        })
        .await;

    let error = client(&server)
        .add_secret_version(
            "s".into(),
            AddSecretVersionRequest {
                payload: SecretPayload {
                    data: Some(SENTINEL.into()),
                },
            },
        )
        .await
        .expect_err("HTTP 403 must reach the caller");

    let recorded = serde_json::to_string(&error).unwrap();
    assert!(recorded.contains("versions.add"), "{recorded}");
    assert!(!recorded.contains(SENTINEL), "{recorded}");
}

#[tokio::test]
async fn an_unparsable_access_answer_never_records_the_secret() {
    let server = MockServer::start_async().await;
    server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/projects/test-project/secrets/s/versions/latest:access");
            // `name` has the wrong type, so the answer cannot be parsed.
            then.status(200)
                .json_body(serde_json::json!({"name": 7, "payload": {"data": SENTINEL}}));
        })
        .await;

    let error = client(&server)
        .access_secret_version("s/versions/latest".into())
        .await
        .expect_err("an unparsable answer must be an error");

    let recorded = serde_json::to_string(&error).unwrap();
    assert!(!recorded.contains(SENTINEL), "{recorded}");
}
