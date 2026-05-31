use crate::kubernetes::kubernetes_client::KubernetesClient;
use crate::kubernetes::kubernetes_request_utils::sign_send_json;
use alien_client_core::Result;
use async_trait::async_trait;
use k8s_openapi::api::core::v1::Node;
use k8s_openapi::List;
#[cfg(feature = "test-utils")]
use mockall::automock;
use reqwest::Method;

#[cfg_attr(feature = "test-utils", automock)]
#[async_trait]
pub trait NodeApi: Send + Sync + std::fmt::Debug {
    async fn list_nodes(
        &self,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Node>>;
}

impl KubernetesClient {
    pub async fn list_nodes(
        &self,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Node>> {
        let mut url = format!("{}/api/v1/nodes", self.get_base_url());
        let mut query_params = Vec::new();

        if let Some(ls) = label_selector {
            query_params.push(("labelSelector", ls));
        }
        if let Some(fs) = field_selector {
            query_params.push(("fieldSelector", fs));
        }

        if !query_params.is_empty() {
            url.push('?');
            url.push_str(
                &query_params
                    .iter()
                    .map(|(key, value)| format!("{}={}", key, urlencoding::encode(value)))
                    .collect::<Vec<_>>()
                    .join("&"),
            );
        }

        let builder = self.client().request(Method::GET, &url);
        sign_send_json(builder, &self.auth_config()).await
    }
}

#[async_trait]
impl NodeApi for KubernetesClient {
    async fn list_nodes(
        &self,
        label_selector: Option<String>,
        field_selector: Option<String>,
    ) -> Result<List<Node>> {
        self.list_nodes(label_selector, field_selector).await
    }
}
