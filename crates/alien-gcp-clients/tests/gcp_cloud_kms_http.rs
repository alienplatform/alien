use alien_gcp_clients::{
    cloud_kms::{CloudKmsApi, CloudKmsClient, EncryptRequest},
    GcpClientConfig, GcpCredentials, ServiceOverrides,
};
use httpmock::prelude::*;
use std::collections::HashMap;

#[tokio::test]
async fn a_failed_encrypt_never_records_the_plaintext() {
    let server = MockServer::start_async().await;
    let client = CloudKmsClient::new(
        reqwest::Client::new(),
        GcpClientConfig {
            project_id: "test-project".into(),
            region: "us-central1".into(),
            credentials: GcpCredentials::AccessToken {
                token: "test-token".into(),
            },
            service_overrides: Some(ServiceOverrides {
                endpoints: HashMap::from([("cloudkms".into(), server.base_url())]),
            }),
            project_number: None,
        },
    );
    let key = "projects/test-project/locations/us-central1/keyRings/ring/cryptoKeys/key";
    server
        .mock_async(|when, then| {
            when.method(POST).path(format!("/{key}:encrypt"));
            then.status(403).json_body(serde_json::json!({
                "error": {"code": 403, "message": "Permission 'cloudkms.cryptoKeyVersions.useToEncrypt' denied", "status": "PERMISSION_DENIED"}
            }));
        })
        .await;

    let plaintext = "cGxhaW50ZXh0LWNhbmFyeS0wNzI4";
    let error = client
        .encrypt(
            key,
            EncryptRequest {
                plaintext: plaintext.to_string(),
                additional_authenticated_data: None,
            },
        )
        .await
        .expect_err("HTTP 403 must reach the caller");

    let recorded = serde_json::to_string(&error).unwrap();
    assert!(recorded.contains("useToEncrypt"), "{recorded}");
    assert!(!recorded.contains(plaintext), "{recorded}");
}
