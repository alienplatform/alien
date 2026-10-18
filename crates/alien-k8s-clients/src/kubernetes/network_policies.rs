//! Namespaced networking.k8s.io/v1 NetworkPolicy operations.
use crate::kubernetes::kubernetes_client::KubernetesClient;
use crate::kubernetes::kubernetes_request_utils::{sign_send_json, sign_send_no_response};
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;
use k8s_openapi::api::networking::v1::NetworkPolicy;
use reqwest::Method;

#[cfg_attr(feature = "test-utils", mockall::automock)]
#[async_trait]
pub trait NetworkPolicyApi: Send + Sync + std::fmt::Debug {
    async fn create_network_policy(
        &self,
        namespace: &str,
        policy: &NetworkPolicy,
    ) -> Result<NetworkPolicy>;
    async fn get_network_policy(&self, namespace: &str, name: &str) -> Result<NetworkPolicy>;
    async fn update_network_policy(
        &self,
        namespace: &str,
        name: &str,
        policy: &NetworkPolicy,
    ) -> Result<NetworkPolicy>;
    async fn delete_network_policy(&self, namespace: &str, name: &str) -> Result<()>;
}

#[async_trait]
impl NetworkPolicyApi for KubernetesClient {
    async fn create_network_policy(
        &self,
        namespace: &str,
        policy: &NetworkPolicy,
    ) -> Result<NetworkPolicy> {
        let body = serde_json::to_string(policy).into_alien_error().context(
            ErrorData::SerializationError {
                message: "Failed to serialize NetworkPolicy".to_string(),
            },
        )?;
        let url = format!(
            "{}/apis/networking.k8s.io/v1/namespaces/{}/networkpolicies",
            self.get_base_url(),
            urlencoding::encode(namespace)
        );
        let builder = self
            .client()
            .request(Method::POST, &url)
            .header("Content-Type", "application/json")
            .body(body);
        sign_send_json(builder, &self.auth_config()).await
    }
    async fn get_network_policy(&self, namespace: &str, name: &str) -> Result<NetworkPolicy> {
        let url = format!(
            "{}/apis/networking.k8s.io/v1/namespaces/{}/networkpolicies/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            urlencoding::encode(name)
        );
        let builder = self.client().request(Method::GET, &url);
        sign_send_json(builder, &self.auth_config()).await
    }
    async fn update_network_policy(
        &self,
        namespace: &str,
        name: &str,
        policy: &NetworkPolicy,
    ) -> Result<NetworkPolicy> {
        let body = serde_json::to_string(policy).into_alien_error().context(
            ErrorData::SerializationError {
                message: "Failed to serialize NetworkPolicy".to_string(),
            },
        )?;
        let url = format!(
            "{}/apis/networking.k8s.io/v1/namespaces/{}/networkpolicies/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            urlencoding::encode(name)
        );
        let builder = self
            .client()
            .request(Method::PUT, &url)
            .header("Content-Type", "application/json")
            .body(body);
        sign_send_json(builder, &self.auth_config()).await
    }
    async fn delete_network_policy(&self, namespace: &str, name: &str) -> Result<()> {
        let url = format!(
            "{}/apis/networking.k8s.io/v1/namespaces/{}/networkpolicies/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            urlencoding::encode(name)
        );
        let builder = self.client().request(Method::DELETE, &url);
        sign_send_no_response(builder, &self.auth_config()).await
    }
}
