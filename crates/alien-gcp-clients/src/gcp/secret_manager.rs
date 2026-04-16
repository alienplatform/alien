use crate::gcp::api_client::{GcpClientBase, GcpServiceConfig};
use crate::gcp::iam::IamPolicy;
use crate::gcp::GcpClientConfig;
use crate::gcp::GcpClientConfigExt;
use alien_client_core::Result;
use bon::Builder;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;
use std::fmt::Debug;

/// Secret Manager service configuration
#[derive(Debug)]
pub struct SecretManagerServiceConfig;

impl GcpServiceConfig for SecretManagerServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://secretmanager.googleapis.com/v1"
    }

    fn default_audience(&self) -> &'static str {
        "https://secretmanager.googleapis.com/"
    }

    fn service_name(&self) -> &'static str {
        "Secret Manager"
    }

    fn service_key(&self) -> &'static str {
        "secretmanager"
    }
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait SecretManagerApi: Send + Sync + Debug {
    async fn create_secret(&self, secret_id: String, secret: Secret) -> Result<Secret>;

    async fn delete_secret(&self, secret_name: String) -> Result<()>;

    async fn get_secret(&self, secret_name: String) -> Result<Secret>;

    async fn patch_secret(
        &self,
        secret_name: String,
        secret: Secret,
        update_mask: Option<String>,
    ) -> Result<Secret>;

    async fn add_secret_version(
        &self,
        secret_name: String,
        request: AddSecretVersionRequest,
    ) -> Result<SecretVersion>;

    async fn access_secret_version(
        &self,
        secret_version_name: String,
    ) -> Result<AccessSecretVersionResponse>;

    async fn get_secret_iam_policy(&self, secret_name: String) -> Result<IamPolicy>;

    async fn set_secret_iam_policy(
        &self,
        secret_name: String,
        iam_policy: IamPolicy,
    ) -> Result<IamPolicy>;
}

/// Secret Manager client for managing secrets and operations
#[derive(Debug)]
pub struct SecretManagerClient {
    base: GcpClientBase,
    project_id: String,
}

impl SecretManagerClient {
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        let project_id = config.project_id.clone();
        Self {
            base: GcpClientBase::new(client, config, Box::new(SecretManagerServiceConfig)),
            project_id,
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl SecretManagerApi for SecretManagerClient {
    /// Creates a new secret.
    /// See: https://cloud.google.com/secret-manager/docs/reference/rest/v1/projects.secrets/create
    async fn create_secret(&self, secret_id: String, secret: Secret) -> Result<Secret> {
        let path = format!("projects/{}/secrets", self.project_id);
        let query_params = vec![("secretId", secret_id.to_string())];

        self.base
            .execute_request(
                Method::POST,
                &path,
                Some(query_params),
                Some(secret),
                &secret_id,
            )
            .await
    }

    /// Deletes a secret.
    /// See: https://cloud.google.com/secret-manager/docs/reference/rest/v1/projects.secrets/delete
    async fn delete_secret(&self, secret_name: String) -> Result<()> {
        let path = format!("projects/{}/secrets/{}", self.project_id, secret_name);

        self.base
            .execute_request(
                Method::DELETE,
                &path,
                None,
                Option::<()>::None,
                &secret_name,
            )
            .await
    }

    /// Gets information about a secret.
    /// See: https://cloud.google.com/secret-manager/docs/reference/rest/v1/projects.secrets/get
    async fn get_secret(&self, secret_name: String) -> Result<Secret> {
        let path = format!("projects/{}/secrets/{}", self.project_id, secret_name);

        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &secret_name)
            .await
    }

    /// Updates a secret.
    /// See: https://cloud.google.com/secret-manager/docs/reference/rest/v1/projects.secrets/patch
    async fn patch_secret(
        &self,
        secret_name: String,
        secret: Secret,
        update_mask: Option<String>,
    ) -> Result<Secret> {
        let path = format!("projects/{}/secrets/{}", self.project_id, secret_name);
        let mut query_params = Vec::new();

        if let Some(mask) = update_mask {
            query_params.push(("updateMask", mask.to_string()));
        }

        self.base
            .execute_request(
                Method::PATCH,
                &path,
                Some(query_params).filter(|v| !v.is_empty()),
                Some(secret),
                &secret_name,
            )
            .await
    }

    /// Adds a secret version with new secret data.
    /// See: https://cloud.google.com/secret-manager/docs/reference/rest/v1/projects.secrets/addVersion
    async fn add_secret_version(
        &self,
        secret_name: String,
        request: AddSecretVersionRequest,
    ) -> Result<SecretVersion> {
        let path = format!(
            "projects/{}/secrets/{}:addVersion",
            self.project_id, secret_name
        );

        self.base
            .execute_request(Method::POST, &path, None, Some(request), &secret_name)
            .await
    }

    /// Accesses a secret version and returns its data.
    /// See: https://cloud.google.com/secret-manager/docs/reference/rest/v1/projects.secrets.versions/access
    async fn access_secret_version(
        &self,
        secret_version_name: String,
    ) -> Result<AccessSecretVersionResponse> {
        let path = format!(
            "projects/{}/secrets/{}:access",
            self.project_id, secret_version_name
        );

        self.base
            .execute_request(
                Method::GET,
                &path,
                None,
                Option::<()>::None,
                &secret_version_name,
            )
            .await
    }

    /// Gets the IAM policy for a secret.
    /// See: https://cloud.google.com/secret-manager/docs/reference/rest/v1/projects.secrets/getIamPolicy
    async fn get_secret_iam_policy(&self, secret_name: String) -> Result<IamPolicy> {
        let path = format!(
            "projects/{}/secrets/{}:getIamPolicy",
            self.project_id, secret_name
        );

        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &secret_name)
            .await
    }

    /// Sets the IAM policy for a secret.
    /// See: https://cloud.google.com/secret-manager/docs/reference/rest/v1/projects.secrets/setIamPolicy
    async fn set_secret_iam_policy(
        &self,
        secret_name: String,
        iam_policy: IamPolicy,
    ) -> Result<IamPolicy> {
        let path = format!(
            "projects/{}/secrets/{}:setIamPolicy",
            self.project_id, secret_name
        );
        let request = SetIamPolicyRequest { policy: iam_policy };

        self.base
            .execute_request(Method::POST, &path, None, Some(request), &secret_name)
            .await
    }
}

// --- Data Structures ---

/// Request message for setting IAM policy.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SetIamPolicyRequest {
    /// The policy to be applied.
    pub policy: IamPolicy,
}

/// A Secret is a logical secret whose value and versions can be accessed.
/// Based on: https://cloud.google.com/secret-manager/docs/reference/rest/v1/projects.secrets#Secret
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Secret {
    /// Output only. The resource name of the Secret.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Required. Immutable. The replication policy of the secret data attached to the Secret.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replication: Option<Replication>,

    /// Output only. The time at which the Secret was created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time: Option<String>,

    /// The labels assigned to this Secret.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,

    /// Optional. Immutable. The TTL for the Secret.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<String>,

    /// Output only. The time at which the Secret will expire.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expire_time: Option<String>,

    /// Optional. A list of up to 10 Pub/Sub topics to which messages are published when control plane operations are called on the secret or its versions.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topics: Vec<Topic>,

    /// Optional. Rotation policy attached to the Secret.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<Rotation>,

    /// Optional. Mapping from version alias to version name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_aliases: Option<HashMap<String, i64>>,

    /// Optional. Custom metadata about the secret.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<HashMap<String, String>>,

    /// Optional. Etag of the currently stored Secret.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
}

/// A secret version resource in the Secret Manager API.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SecretVersion {
    /// Output only. The resource name of the SecretVersion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Output only. The time at which the SecretVersion was created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time: Option<String>,

    /// Output only. The time this SecretVersion was destroyed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destroy_time: Option<String>,

    /// Output only. The current state of the SecretVersion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<SecretVersionState>,

    /// Output only. Etag of the currently stored SecretVersion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,

    /// Output only. True if payload is stored in Google Secret Manager and false if payload is stored externally.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_specified_payload_checksum: Option<bool>,
}

/// The state of a SecretVersion.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SecretVersionState {
    /// Not specified. This value is unused and invalid.
    StateUnspecified,
    /// The SecretVersion may be accessed.
    Enabled,
    /// The SecretVersion may not be accessed, but the secret data is still available and can be placed back into the ENABLED state.
    Disabled,
    /// The SecretVersion is destroyed and the secret data is no longer stored.
    Destroyed,
}

/// Request message for adding a secret version.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AddSecretVersionRequest {
    /// Required. The secret payload.
    pub payload: SecretPayload,
}

/// Response message for AccessSecretVersion.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AccessSecretVersionResponse {
    /// The resource name of the SecretVersion in the format projects/*/secrets/*/versions/*.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Secret payload.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<SecretPayload>,
}

/// A secret payload resource in the Secret Manager API.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct SecretPayload {
    /// The secret data. Must be no larger than 64KiB.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

/// A policy that defines the replication configuration of data.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Replication {
    /// Union field replication_policy. replication_policy can be only one of the following:
    #[serde(flatten)]
    pub replication_policy: Option<ReplicationPolicy>,
}

/// Union field representing the replication policy. Only one replication type can be specified.
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum ReplicationPolicy {
    /// The Secret will automatically be replicated without any restrictions.
    Automatic(AutomaticReplication),

    /// The Secret will only be replicated into the locations specified.
    UserManaged(UserManagedReplication),
}

/// A replication policy that replicates the Secret payload without any restrictions.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AutomaticReplication {
    /// Optional. The customer-managed encryption configuration of the Secret.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_managed_encryption: Option<CustomerManagedEncryption>,
}

/// A replication policy that replicates the Secret payload into the locations specified in Secret.replication.user_managed.replicas
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct UserManagedReplication {
    /// Required. The list of Replicas for this Secret.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replicas: Vec<Replica>,
}

/// Represents a Replica for this Secret.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Replica {
    /// The canonical IDs of the location to replicate data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,

    /// Optional. The customer-managed encryption configuration of the User-Managed Replica.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_managed_encryption: Option<CustomerManagedEncryption>,
}

/// Configuration for encrypting secret payloads using customer-managed encryption keys (CMEK).
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct CustomerManagedEncryption {
    /// Required. The resource name of the Cloud KMS CryptoKey used to encrypt secret payloads.
    pub kms_key_name: String,
}

/// A Pub/Sub topic which Secret Manager will publish to when control plane events occur on this secret.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Topic {
    /// Required. The resource name of the Pub/Sub topic.
    pub name: String,
}

/// The rotation time and period for a Secret.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Rotation {
    /// Optional. Timestamp in UTC at which the Secret is scheduled to rotate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_rotation_time: Option<String>,

    /// Input only. The Duration between rotation notifications.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation_period: Option<String>,
}
