use crate::kubernetes::kubernetes_client::KubernetesClient;
use crate::kubernetes::kubernetes_request_utils::sign_send_json;
use alien_client_core::Result;
use async_trait::async_trait;
use k8s_openapi::api::core::v1::Event;
use k8s_openapi::List;
#[cfg(feature = "test-utils")]
use mockall::automock;
use reqwest::Method;

#[cfg_attr(feature = "test-utils", automock)]
#[async_trait]
pub trait EventApi: Send + Sync + std::fmt::Debug {
    async fn list_events(
        &self,
        namespace: &str,
        field_selector: Option<String>,
    ) -> Result<List<Event>>;
}

impl KubernetesClient {
    pub async fn list_events(
        &self,
        namespace: &str,
        field_selector: Option<String>,
    ) -> Result<List<Event>> {
        let mut url = format!(
            "{}/api/v1/namespaces/{}/events",
            self.get_base_url(),
            urlencoding::encode(namespace)
        );

        if let Some(fs) = field_selector {
            url.push_str(&format!("?fieldSelector={}", urlencoding::encode(&fs)));
        }

        let builder = self.client().request(Method::GET, &url);
        sign_send_json(builder, &self.auth_config()).await
    }
}

#[async_trait]
impl EventApi for KubernetesClient {
    async fn list_events(
        &self,
        namespace: &str,
        field_selector: Option<String>,
    ) -> Result<List<Event>> {
        self.list_events(namespace, field_selector).await
    }
}
