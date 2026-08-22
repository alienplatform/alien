use crate::kubernetes::kubernetes_client::KubernetesClient;
use crate::kubernetes::kubernetes_request_utils::sign_send_json;
use alien_client_core::Result;
use reqwest::Method;

use k8s_openapi::api::node::v1::RuntimeClass;
use k8s_openapi::List;

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;

#[cfg_attr(feature = "test-utils", automock)]
#[async_trait]
pub trait RuntimeClassApi: Send + Sync + std::fmt::Debug {
    async fn list_runtime_classes(&self) -> Result<List<RuntimeClass>>;
}

impl KubernetesClient {
    /// Lists the cluster's RuntimeClasses.
    ///
    /// A cluster-scoped object, so this answers "can this cluster run a sandboxed pod at all"
    /// without depending on nodes being present — which matters because node auto-provisioning
    /// creates them on demand, so an empty node list is not the same as an ineligible cluster.
    pub async fn list_runtime_classes(&self) -> Result<List<RuntimeClass>> {
        let url = format!("{}/apis/node.k8s.io/v1/runtimeclasses", self.get_base_url());
        let builder = self.client().request(Method::GET, &url);
        sign_send_json(builder, &self.auth_config()).await
    }
}

#[async_trait]
impl RuntimeClassApi for KubernetesClient {
    async fn list_runtime_classes(&self) -> Result<List<RuntimeClass>> {
        KubernetesClient::list_runtime_classes(self).await
    }
}
