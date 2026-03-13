/*!
# GCP Secret Manager Client Integration Tests

These tests perform real GCP Secret Manager operations including creating, updating,
retrieving, and deleting secrets and secret versions.

## Prerequisites

### 1. GCP Credentials
Set up `.env.test` in the workspace root with:
```
GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY={"type":"service_account",...}
```

### 2. Required Permissions
Your service account needs these permissions:
- `secretmanager.secrets.create`
- `secretmanager.secrets.delete`
- `secretmanager.secrets.get`
- `secretmanager.secrets.update`
- `secretmanager.versions.add`
- `secretmanager.versions.access`

### 3. Enable Secret Manager API
Enable the Secret Manager API in your GCP project:
https://console.developers.google.com/apis/api/secretmanager.googleapis.com/overview

## Running Tests
```bash
# Run all Secret Manager tests
cargo test --package alien-gcp-clients --test gcp_secret_manager_client_tests

# Run specific test
cargo test --package alien-gcp-clients --test gcp_secret_manager_client_tests test_create_secret -- --nocapture
```
*/

use alien_client_core::{Error, ErrorData};
use alien_gcp_clients::gcp::{GcpClientConfig, GcpCredentials};
use alien_gcp_clients::secret_manager::{
    AddSecretVersionRequest, AutomaticReplication, Replication, ReplicationPolicy, Secret,
    SecretManagerApi, SecretManagerClient, SecretPayload,
};
use base64::{engine::general_purpose::STANDARD as base64_standard, Engine as _};
use reqwest::Client;
use std::collections::{HashMap, HashSet};
use std::env;
use std::path::PathBuf;
use std::sync::Mutex;
use test_context::{test_context, AsyncTestContext};
use tracing::{info, warn};
use uuid::Uuid;

// Global Secret Manager doesn't require a specific location

struct SecretManagerTestContext {
    client: SecretManagerClient,
    created_secrets: Mutex<HashSet<String>>,
}

impl AsyncTestContext for SecretManagerTestContext {
    async fn setup() -> SecretManagerTestContext {
        let root: PathBuf = workspace_root::get_workspace_root();
        dotenvy::from_path(root.join(".env.test")).expect("Failed to load .env.test");
        tracing_subscriber::fmt::try_init().ok();

        let gcp_credentials_json = env::var("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY")
            .unwrap_or_else(|_| panic!("GOOGLE_MANAGEMENT_SERVICE_ACCOUNT_KEY must be set"));

        // Parse project_id from service account
        let service_account_value: serde_json::Value =
            serde_json::from_str(&gcp_credentials_json).unwrap();
        let project_id = service_account_value
            .get("project_id")
            .and_then(|v| v.as_str())
            .map(String::from)
            .expect("'project_id' must be present in the service account JSON");

        let config = GcpClientConfig {
            project_id: project_id.clone(),
            region: "".to_string(), // Global Secret Manager doesn't require a specific region
            credentials: GcpCredentials::ServiceAccountKey {
                json: gcp_credentials_json,
            },
            service_overrides: None,
        };

        let client = SecretManagerClient::new(Client::new(), config);

        SecretManagerTestContext {
            client,
            created_secrets: Mutex::new(HashSet::new()),
        }
    }

    async fn teardown(self) {
        info!("Starting Secret Manager test cleanup...");

        let secrets_to_cleanup = {
            let secrets = self.created_secrets.lock().unwrap();
            secrets.clone()
        };

        for secret_id in secrets_to_cleanup {
            self.cleanup_secret(&secret_id).await;
        }

        info!("Secret Manager test cleanup completed");
    }
}

impl SecretManagerTestContext {
    fn track_secret(&self, secret_id: &str) {
        let mut secrets = self.created_secrets.lock().unwrap();
        secrets.insert(secret_id.to_string());
    }

    fn untrack_secret(&self, secret_id: &str) {
        let mut secrets = self.created_secrets.lock().unwrap();
        secrets.remove(secret_id);
    }

    async fn cleanup_secret(&self, secret_id: &str) {
        match self.client.delete_secret(secret_id.to_string()).await {
            Ok(_) => {
                info!("Secret {} deletion initiated successfully", secret_id);
            }
            Err(infra_err) => match &infra_err.error {
                Some(ErrorData::RemoteResourceNotFound { .. }) => {
                    info!("Secret {} was already deleted", secret_id);
                }
                _ => {
                    warn!(
                        "Failed to delete secret {} during cleanup: {:?}",
                        secret_id, infra_err
                    );
                }
            },
        }
    }

    fn generate_unique_secret_id(&self) -> String {
        format!(
            "alien-test-secret-{}",
            Uuid::new_v4().hyphenated().to_string().replace("-", "")[..12].to_lowercase()
        )
    }

    async fn create_test_secret(&self, secret_id: String, secret: Secret) -> Result<Secret, Error> {
        let result = self.client.create_secret(secret_id.clone(), secret).await;
        if result.is_ok() {
            self.track_secret(&secret_id);
        }
        result
    }

    fn create_basic_secret(&self) -> Secret {
        let replication = Replication::builder()
            .replication_policy(ReplicationPolicy::Automatic(
                AutomaticReplication::builder().build(),
            ))
            .build();

        Secret::builder().replication(replication).build()
    }

    fn create_secret_with_labels(&self, labels: HashMap<String, String>) -> Secret {
        let replication = Replication::builder()
            .replication_policy(ReplicationPolicy::Automatic(
                AutomaticReplication::builder().build(),
            ))
            .build();

        Secret::builder()
            .replication(replication)
            .labels(labels)
            .build()
    }
}

#[test_context(SecretManagerTestContext)]
#[tokio::test]
async fn test_create_secret(ctx: &mut SecretManagerTestContext) {
    let secret_id = ctx.generate_unique_secret_id();
    let secret = ctx.create_basic_secret();

    let create_result = ctx.create_test_secret(secret_id.clone(), secret).await;
    match create_result {
        Ok(response) => {
            assert!(response.name.is_some());
            assert!(response.name.as_ref().unwrap().contains(&secret_id));
            assert!(response.create_time.is_some());
        }
        Err(e) => {
            panic!("Secret creation failed: {:?}. Please ensure you have proper GCP credentials, Secret Manager API enabled, and permissions set up in .env.test", e);
        }
    }
}

#[test_context(SecretManagerTestContext)]
#[tokio::test]
async fn test_get_secret(ctx: &mut SecretManagerTestContext) {
    let secret_id = ctx.generate_unique_secret_id();

    // Create a secret with labels first
    let mut labels = HashMap::new();
    labels.insert("purpose".to_string(), "testing".to_string());
    labels.insert("environment".to_string(), "test".to_string());

    let secret = ctx.create_secret_with_labels(labels.clone());
    let create_response = ctx
        .create_test_secret(secret_id.clone(), secret)
        .await
        .expect("Failed to create secret for get test");
    let secret_name = create_response.name.unwrap();

    // Get the secret
    let get_response = ctx
        .client
        .get_secret(secret_id.clone())
        .await
        .expect("Failed to get secret");

    assert_eq!(get_response.name.as_ref().unwrap(), &secret_name);
    assert!(get_response.labels.is_some());
    assert_eq!(
        get_response.labels.unwrap().get("purpose"),
        Some(&"testing".to_string())
    );
    assert!(get_response.create_time.is_some());
}

#[test_context(SecretManagerTestContext)]
#[tokio::test]
async fn test_patch_secret(ctx: &mut SecretManagerTestContext) {
    let secret_id = ctx.generate_unique_secret_id();

    // Create a secret with initial labels
    let mut initial_labels = HashMap::new();
    initial_labels.insert("version".to_string(), "1".to_string());
    let secret = ctx.create_secret_with_labels(initial_labels);
    ctx.create_test_secret(secret_id.clone(), secret)
        .await
        .expect("Failed to create secret for patch test");

    // Update the secret labels
    let mut updated_labels = HashMap::new();
    updated_labels.insert("version".to_string(), "2".to_string());
    updated_labels.insert("updated".to_string(), "true".to_string());

    let replication = Replication::builder()
        .replication_policy(ReplicationPolicy::Automatic(
            AutomaticReplication::builder().build(),
        ))
        .build();

    let updated_secret = Secret::builder()
        .replication(replication)
        .labels(updated_labels.clone())
        .build();

    let patch_response = ctx
        .client
        .patch_secret(
            secret_id.clone(),
            updated_secret,
            Some("labels".to_string()),
        )
        .await
        .expect("Failed to patch secret");

    assert!(patch_response.labels.is_some());
    let response_labels = patch_response.labels.unwrap();
    assert_eq!(response_labels.get("version"), Some(&"2".to_string()));
    assert_eq!(response_labels.get("updated"), Some(&"true".to_string()));
}

#[test_context(SecretManagerTestContext)]
#[tokio::test]
async fn test_add_secret_version(ctx: &mut SecretManagerTestContext) {
    let secret_id = ctx.generate_unique_secret_id();
    let secret_data = "my-secret-value";

    // Create a secret first
    let secret = ctx.create_basic_secret();
    ctx.create_test_secret(secret_id.clone(), secret)
        .await
        .expect("Failed to create secret for add version test");

    // Add a secret version
    let payload = SecretPayload::builder()
        .data(base64_standard.encode(secret_data))
        .build();

    let add_version_request = AddSecretVersionRequest::builder().payload(payload).build();

    let version_response = ctx
        .client
        .add_secret_version(secret_id.clone(), add_version_request)
        .await
        .expect("Failed to add secret version");

    assert!(version_response.name.is_some());
    assert!(version_response.create_time.is_some());
    assert!(version_response.state.is_some());
}

#[test_context(SecretManagerTestContext)]
#[tokio::test]
async fn test_access_secret_version(ctx: &mut SecretManagerTestContext) {
    let secret_id = ctx.generate_unique_secret_id();
    let secret_data = "my-test-secret-value-12345";

    // Create a secret first
    let secret = ctx.create_basic_secret();
    ctx.create_test_secret(secret_id.clone(), secret)
        .await
        .expect("Failed to create secret for access version test");

    // Add a secret version with known data
    let payload = SecretPayload::builder()
        .data(base64_standard.encode(secret_data))
        .build();

    let add_version_request = AddSecretVersionRequest::builder().payload(payload).build();

    let version_response = ctx
        .client
        .add_secret_version(secret_id.clone(), add_version_request)
        .await
        .expect("Failed to add secret version");

    // Small delay to ensure version is ready for access
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let version_name = version_response.name.expect("Version should have a name");

    // Extract version number from the full resource name
    // Format: projects/{project}/locations/{location}/secrets/{secret}/versions/{version}
    let version_id = version_name
        .split('/')
        .last()
        .expect("Invalid version name format");
    let secret_version_path = format!("{}/versions/{}", secret_id, version_id);

    // Access the secret version and verify we get back the original data
    let access_response = ctx
        .client
        .access_secret_version(secret_version_path)
        .await
        .expect("Failed to access secret version");

    assert!(access_response.payload.is_some());
    let payload = access_response.payload.unwrap();
    assert!(payload.data.is_some());

    let returned_data = payload.data.unwrap();
    let decoded_data = base64_standard
        .decode(&returned_data)
        .expect("Failed to decode returned secret data");
    let secret_value =
        String::from_utf8(decoded_data).expect("Failed to convert decoded data to string");

    assert_eq!(secret_value, secret_data);
}

#[test_context(SecretManagerTestContext)]
#[tokio::test]
async fn test_access_latest_secret_version(ctx: &mut SecretManagerTestContext) {
    let secret_id = ctx.generate_unique_secret_id();
    let secret_data_v1 = "secret-value-version-1";
    let secret_data_v2 = "secret-value-version-2";

    // Create a secret first
    let secret = ctx.create_basic_secret();
    ctx.create_test_secret(secret_id.clone(), secret)
        .await
        .expect("Failed to create secret for latest version test");

    // Add first version
    let payload_v1 = SecretPayload::builder()
        .data(base64_standard.encode(secret_data_v1))
        .build();

    let add_version_request_v1 = AddSecretVersionRequest::builder()
        .payload(payload_v1)
        .build();

    ctx.client
        .add_secret_version(secret_id.clone(), add_version_request_v1)
        .await
        .expect("Failed to add first secret version");

    // Add second version
    let payload_v2 = SecretPayload::builder()
        .data(base64_standard.encode(secret_data_v2))
        .build();

    let add_version_request_v2 = AddSecretVersionRequest::builder()
        .payload(payload_v2)
        .build();

    ctx.client
        .add_secret_version(secret_id.clone(), add_version_request_v2)
        .await
        .expect("Failed to add second secret version");

    // Longer delay for global endpoints to ensure version propagation
    tokio::time::sleep(std::time::Duration::from_millis(2000)).await;

    // Access the latest version using the "latest" alias
    let latest_version_path = format!("{}/versions/latest", secret_id);
    let access_response = ctx
        .client
        .access_secret_version(latest_version_path)
        .await
        .expect("Failed to access latest secret version");

    assert!(access_response.payload.is_some());
    let payload = access_response.payload.unwrap();
    assert!(payload.data.is_some());

    let returned_data = payload.data.unwrap();
    let decoded_data = base64_standard
        .decode(&returned_data)
        .expect("Failed to decode latest secret data");
    let secret_value =
        String::from_utf8(decoded_data).expect("Failed to convert decoded data to string");

    // Should get back the second version (latest)
    assert_eq!(secret_value, secret_data_v2);
}

#[test_context(SecretManagerTestContext)]
#[tokio::test]
async fn test_delete_secret(ctx: &mut SecretManagerTestContext) {
    let secret_id = ctx.generate_unique_secret_id();

    // Create a secret first
    let secret = ctx.create_basic_secret();
    ctx.create_test_secret(secret_id.clone(), secret)
        .await
        .expect("Failed to create secret for delete test");

    // Delete the secret
    match ctx.client.delete_secret(secret_id.clone()).await {
        Ok(_) => (),
        Err(infra_err) => match &infra_err.error {
            Some(ErrorData::RemoteResourceNotFound { .. }) => (),
            _ => panic!("Failed to delete secret: {:?}", infra_err),
        },
    }

    ctx.untrack_secret(&secret_id); // Untrack since we manually deleted it

    // Verify the secret is deleted by trying to get it
    let result = ctx.client.get_secret(secret_id.clone()).await;
    assert!(result.is_err()); // Should fail because secret is deleted
}

#[test_context(SecretManagerTestContext)]
#[tokio::test]
async fn test_get_non_existent_secret(ctx: &mut SecretManagerTestContext) {
    let non_existent_secret = "alien-test-non-existent-secret-12345";

    let result = ctx.client.get_secret(non_existent_secret.to_string()).await;

    assert!(result.is_err());
    match result.unwrap_err() {
        Error {
            error:
                Some(ErrorData::RemoteResourceNotFound {
                    resource_type,
                    resource_name,
                    ..
                }),
            ..
        } => {
            assert_eq!(resource_type, "Secret Manager");
            assert_eq!(resource_name, non_existent_secret);
        }
        other => panic!("Expected RemoteResourceNotFound, got: {:?}", other),
    }
}

#[test_context(SecretManagerTestContext)]
#[tokio::test]
async fn test_secret_lifecycle(ctx: &mut SecretManagerTestContext) {
    let secret_id = ctx.generate_unique_secret_id();
    let secret_data_v1 = "initial-secret-value";
    let secret_data_v2 = "updated-secret-value";

    // 1. Create secret with labels
    let mut initial_labels = HashMap::new();
    initial_labels.insert("environment".to_string(), "test".to_string());
    initial_labels.insert("version".to_string(), "initial".to_string());

    let secret = ctx.create_secret_with_labels(initial_labels);
    let create_response = ctx
        .create_test_secret(secret_id.clone(), secret)
        .await
        .expect("Failed to create secret for lifecycle test");
    let secret_name = create_response.name.unwrap();

    // 2. Get secret to verify creation
    let get_response = ctx
        .client
        .get_secret(secret_id.clone())
        .await
        .expect("Failed to get secret in lifecycle test");
    assert_eq!(get_response.name.as_ref().unwrap(), &secret_name);
    assert!(get_response.labels.is_some());

    // 3. Add first secret version
    let payload_v1 = SecretPayload::builder()
        .data(base64_standard.encode(secret_data_v1))
        .build();

    let add_version_request_v1 = AddSecretVersionRequest::builder()
        .payload(payload_v1)
        .build();

    let version_v1_response = ctx
        .client
        .add_secret_version(secret_id.clone(), add_version_request_v1)
        .await
        .expect("Failed to add first secret version");
    assert!(version_v1_response.name.is_some());

    // 4. Update secret metadata
    let mut updated_labels = HashMap::new();
    updated_labels.insert("environment".to_string(), "test".to_string());
    updated_labels.insert("version".to_string(), "updated".to_string());
    updated_labels.insert("has_versions".to_string(), "true".to_string());

    let replication = Replication::builder()
        .replication_policy(ReplicationPolicy::Automatic(
            AutomaticReplication::builder().build(),
        ))
        .build();

    let updated_secret = Secret::builder()
        .replication(replication)
        .labels(updated_labels.clone())
        .build();

    let patch_response = ctx
        .client
        .patch_secret(
            secret_id.clone(),
            updated_secret,
            Some("labels".to_string()),
        )
        .await
        .expect("Failed to patch secret in lifecycle test");
    assert!(patch_response.labels.is_some());

    // 5. Add second secret version
    let payload_v2 = SecretPayload::builder()
        .data(base64_standard.encode(secret_data_v2))
        .build();

    let add_version_request_v2 = AddSecretVersionRequest::builder()
        .payload(payload_v2)
        .build();

    let version_v2_response = ctx
        .client
        .add_secret_version(secret_id.clone(), add_version_request_v2)
        .await
        .expect("Failed to add second secret version");
    assert!(version_v2_response.name.is_some());

    // Small delay to ensure version is ready for access
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    // 6. Verify final state - both metadata and secret value
    let final_get_response = ctx
        .client
        .get_secret(secret_id.clone())
        .await
        .expect("Failed to get final secret state");
    let final_labels = final_get_response.labels.unwrap();
    assert_eq!(final_labels.get("version"), Some(&"updated".to_string()));
    assert_eq!(final_labels.get("has_versions"), Some(&"true".to_string()));

    // 7. Verify we can access the latest secret value
    let latest_version_path = format!("{}/versions/latest", secret_id);
    let latest_access_response = ctx
        .client
        .access_secret_version(latest_version_path)
        .await
        .expect("Failed to access latest secret version");

    let latest_payload = latest_access_response.payload.unwrap();
    let latest_data = latest_payload.data.unwrap();
    let latest_decoded = base64_standard
        .decode(&latest_data)
        .expect("Failed to decode latest secret data");
    let latest_value =
        String::from_utf8(latest_decoded).expect("Failed to convert decoded data to string");

    // Should get back the second version data
    assert_eq!(latest_value, secret_data_v2);
}
