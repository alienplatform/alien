use crate::kubernetes::kubernetes_client::KubernetesClient;
use crate::kubernetes::kubernetes_request_utils::{sign_send_json, sign_send_no_response};
use alien_client_core::{redact_request_body, redact_response_body, ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use reqwest::Method;

use k8s_openapi::api::core::v1::Secret;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
use k8s_openapi::List;
use serde::Deserialize;

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;

#[cfg_attr(feature = "test-utils", automock)]
#[async_trait]
pub trait SecretsApi: Send + Sync + std::fmt::Debug {
    async fn create_secret(&self, namespace: &str, secret: &Secret) -> Result<Secret>;
    async fn get_secret(&self, namespace: &str, name: &str) -> Result<Secret>;
    /// A Secret's object metadata, without its data.
    async fn get_secret_metadata(&self, namespace: &str, name: &str) -> Result<ObjectMeta>;
    async fn list_secrets(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Secret>>;
    async fn update_secret(&self, namespace: &str, name: &str, secret: &Secret) -> Result<Secret>;
    async fn delete_secret(&self, namespace: &str, name: &str) -> Result<()>;
    /// Deletes the Secret only while it is still the object with this uid, so a Secret that was
    /// deleted and created again under the same name is left alone.
    async fn delete_secret_if_uid(&self, namespace: &str, name: &str, uid: &str) -> Result<()>;
}

impl KubernetesClient {
    /// Create a secret in the specified namespace
    pub async fn create_secret(&self, namespace: &str, secret: &Secret) -> Result<Secret> {
        let body = serde_json::to_string(secret).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize Secret '{}'",
                    secret.metadata.name.as_deref().unwrap_or("unknown")
                ),
            },
        )?;

        let url = format!(
            "{}/api/v1/namespaces/{}/secrets",
            self.get_base_url(),
            urlencoding::encode(namespace)
        );
        let builder = self
            .client()
            .request(Method::POST, &url)
            .header("Content-Type", "application/json")
            .body(body);

        // The body and a 200 answer both hold the Secret's data; an error carries neither.
        redact_response_body(redact_request_body(
            sign_send_json(builder, &self.auth_config()).await,
        ))
    }

    /// Get a secret by name in the specified namespace
    pub async fn get_secret(&self, namespace: &str, name: &str) -> Result<Secret> {
        let url = format!(
            "{}/api/v1/namespaces/{}/secrets/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            urlencoding::encode(name)
        );
        let builder = self.client().request(Method::GET, &url);

        // A 200 answer that cannot be read holds the Secret's data; an error must not carry it.
        redact_response_body(sign_send_json(builder, &self.auth_config()).await)
    }

    /// Get a secret's object metadata without its data. The API server answers
    /// with a `PartialObjectMetadata`, so the secret's values never leave it.
    pub async fn get_secret_metadata(&self, namespace: &str, name: &str) -> Result<ObjectMeta> {
        #[derive(Deserialize)]
        struct PartialObjectMetadata {
            metadata: ObjectMeta,
        }

        let url = format!(
            "{}/api/v1/namespaces/{}/secrets/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            urlencoding::encode(name)
        );
        let builder = self.client().request(Method::GET, &url).header(
            "Accept",
            "application/json;as=PartialObjectMetadata;g=meta.k8s.io;v=v1",
        );

        let partial: PartialObjectMetadata = sign_send_json(builder, &self.auth_config()).await?;
        Ok(partial.metadata)
    }

    /// List secrets in the specified namespace with optional selectors
    pub async fn list_secrets(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Secret>> {
        let mut url = format!(
            "{}/api/v1/namespaces/{}/secrets",
            self.get_base_url(),
            urlencoding::encode(namespace)
        );
        let mut query_params = Vec::new();

        if let Some(ls) = &label_selector {
            query_params.push(("labelSelector", ls.as_str()));
        }
        if let Some(fs) = &field_selector {
            query_params.push(("fieldSelector", fs.as_str()));
        }

        if !query_params.is_empty() {
            url.push('?');
            url.push_str(
                &query_params
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
                    .collect::<Vec<_>>()
                    .join("&"),
            );
        }

        let builder = self.client().request(Method::GET, &url);

        redact_response_body(sign_send_json(builder, &self.auth_config()).await)
    }

    /// Update a secret in the specified namespace
    pub async fn update_secret(
        &self,
        namespace: &str,
        name: &str,
        secret: &Secret,
    ) -> Result<Secret> {
        let body = serde_json::to_string(secret).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize Secret '{}'", name),
            },
        )?;

        let url = format!(
            "{}/api/v1/namespaces/{}/secrets/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            urlencoding::encode(name)
        );
        let builder = self
            .client()
            .request(Method::PUT, &url)
            .header("Content-Type", "application/json")
            .body(body);

        // The body and a 200 answer both hold the Secret's data; an error carries neither.
        redact_response_body(redact_request_body(
            sign_send_json(builder, &self.auth_config()).await,
        ))
    }

    /// Delete the observed secret without deleting a replacement with the same name.
    pub async fn delete_secret_if_uid(&self, namespace: &str, name: &str, uid: &str) -> Result<()> {
        let url = format!(
            "{}/api/v1/namespaces/{}/secrets/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            urlencoding::encode(name)
        );
        let builder = self
            .client()
            .request(Method::DELETE, &url)
            .header("Content-Type", "application/json")
            .body(
                serde_json::json!({
                    "apiVersion": "v1",
                    "kind": "DeleteOptions",
                    "preconditions": { "uid": uid }
                })
                .to_string(),
            );
        sign_send_no_response(builder, &self.auth_config()).await
    }

    /// Delete a secret in the specified namespace
    pub async fn delete_secret(&self, namespace: &str, name: &str) -> Result<()> {
        let url = format!(
            "{}/api/v1/namespaces/{}/secrets/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            urlencoding::encode(name)
        );
        let builder = self.client().request(Method::DELETE, &url);

        sign_send_no_response(builder, &self.auth_config()).await
    }
}

#[async_trait]
impl SecretsApi for KubernetesClient {
    async fn create_secret(&self, namespace: &str, secret: &Secret) -> Result<Secret> {
        self.create_secret(namespace, secret).await
    }

    async fn get_secret(&self, namespace: &str, name: &str) -> Result<Secret> {
        self.get_secret(namespace, name).await
    }

    async fn get_secret_metadata(&self, namespace: &str, name: &str) -> Result<ObjectMeta> {
        self.get_secret_metadata(namespace, name).await
    }

    async fn list_secrets(
        &self,
        namespace: &str,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Secret>> {
        self.list_secrets(namespace, label_selector, field_selector)
            .await
    }

    async fn update_secret(&self, namespace: &str, name: &str, secret: &Secret) -> Result<Secret> {
        self.update_secret(namespace, name, secret).await
    }

    async fn delete_secret(&self, namespace: &str, name: &str) -> Result<()> {
        self.delete_secret(namespace, name).await
    }

    async fn delete_secret_if_uid(&self, namespace: &str, name: &str, uid: &str) -> Result<()> {
        self.delete_secret_if_uid(namespace, name, uid).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kubernetes::kubernetes_client::KubernetesClient;
    use alien_core::KubernetesClientConfig;
    use httpmock::{Method::PUT, MockServer};
    use k8s_openapi::ByteString;

    async fn client_for(server: &MockServer) -> KubernetesClient {
        KubernetesClient::new(KubernetesClientConfig::Manual {
            server_url: server.base_url(),
            certificate_authority_data: None,
            insecure_skip_tls_verify: None,
            client_certificate_data: None,
            client_key_data: None,
            token: None,
            username: None,
            password: None,
            namespace: Some("ns".into()),
            additional_headers: Default::default(),
        })
        .await
        .unwrap()
    }

    fn secret_with_data() -> Secret {
        let mut secret = Secret::default();
        secret.data = Some(
            [(
                "identity".to_string(),
                ByteString(b"ax_day_v1_sensitive".to_vec()),
            )]
            .into(),
        );
        secret
    }

    fn assert_no_secret(error: &alien_error::AlienError<alien_client_core::ErrorData>) {
        let shown = format!(
            "{error:?} {error} {}",
            serde_json::to_string(error).unwrap()
        );
        assert!(!shown.contains("ax_day_v1_sensitive"), "{shown}");
        let encoded = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            b"ax_day_v1_sensitive",
        );
        assert!(!shown.contains(&encoded), "{shown}");
    }

    #[tokio::test]
    async fn a_refused_secret_update_does_not_carry_the_secret_data_in_its_error() {
        let server = MockServer::start_async().await;
        server.mock(|when, then| {
            when.method(PUT)
                .path("/api/v1/namespaces/ns/secrets/identity");
            then.status(422).json_body(serde_json::json!({
                "kind": "Status", "apiVersion": "v1", "status": "Failure",
                "reason": "Invalid", "code": 422,
                "details": {"name": "identity", "kind": "secrets"},
            }));
        });
        let client = client_for(&server).await;
        let error = client
            .update_secret("ns", "identity", &secret_with_data())
            .await
            .unwrap_err();
        assert_no_secret(&error);
    }

    /// A 200 that does not parse as a Secret still holds the Secret's data (the identity and the
    /// database key); the error from reading or updating it must not repeat that text.
    #[tokio::test]
    async fn a_malformed_200_from_the_secret_api_does_not_carry_the_secret_body() {
        let server = MockServer::start_async().await;
        // `metadata` must be an object, so this fails to parse, with the data beside it.
        let body = serde_json::json!({
            "metadata": "not-an-object",
            "data": {"identity": "YXhfZGF5X3YxX3NlbnNpdGl2ZQ=="},
            "stringData": {"identity": "ax_day_v1_sensitive"},
        });
        server.mock(|when, then| {
            when.method(httpmock::Method::GET)
                .path("/api/v1/namespaces/ns/secrets/identity");
            then.status(200).json_body(body.clone());
        });
        server.mock(|when, then| {
            when.method(PUT)
                .path("/api/v1/namespaces/ns/secrets/identity");
            then.status(200).json_body(body.clone());
        });
        let client = client_for(&server).await;
        let error = client.get_secret("ns", "identity").await.unwrap_err();
        assert_no_secret(&error);
        let error = client
            .update_secret("ns", "identity", &secret_with_data())
            .await
            .unwrap_err();
        assert_no_secret(&error);
    }
}
