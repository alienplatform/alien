use crate::kubernetes::kubernetes_client::KubernetesClient;
use crate::kubernetes::kubernetes_request_utils::sign_send_json;
use alien_client_core::Result;
use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;
use reqwest::Method;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KubernetesVersion {
    pub major: Option<String>,
    pub minor: Option<String>,
    #[serde(rename = "gitVersion")]
    pub git_version: Option<String>,
}

#[cfg_attr(feature = "test-utils", automock)]
#[async_trait]
pub trait VersionApi: Send + Sync + std::fmt::Debug {
    async fn get_version(&self) -> Result<KubernetesVersion>;
}

impl KubernetesClient {
    pub async fn get_version(&self) -> Result<KubernetesVersion> {
        let url = format!("{}/version", self.get_base_url());
        let builder = self.client().request(Method::GET, &url);
        sign_send_json(builder, &self.auth_config()).await
    }
}

#[async_trait]
impl VersionApi for KubernetesClient {
    async fn get_version(&self) -> Result<KubernetesVersion> {
        self.get_version().await
    }
}
