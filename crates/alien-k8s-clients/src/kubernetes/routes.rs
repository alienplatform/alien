use crate::kubernetes::kubernetes_client::KubernetesClient;
use crate::kubernetes::kubernetes_request_utils::{sign_send_json, sign_send_no_response};
use alien_client_core::{ErrorData, Result};
use alien_error::{Context, IntoAlienError};
use async_trait::async_trait;
use k8s_openapi::api::networking::v1::Ingress;
use reqwest::Method;
use serde_json::Value;

#[async_trait]
pub trait RouteApi: Send + Sync + std::fmt::Debug {
    async fn create_ingress(&self, namespace: &str, ingress: &Ingress) -> Result<Ingress>;
    async fn get_ingress(&self, namespace: &str, name: &str) -> Result<Ingress>;
    async fn update_ingress(
        &self,
        namespace: &str,
        name: &str,
        ingress: &Ingress,
    ) -> Result<Ingress>;
    async fn delete_ingress(&self, namespace: &str, name: &str) -> Result<()>;

    async fn create_gateway(&self, namespace: &str, gateway: &Value) -> Result<Value>;
    async fn get_gateway(&self, namespace: &str, name: &str) -> Result<Value>;
    async fn update_gateway(&self, namespace: &str, name: &str, gateway: &Value) -> Result<Value>;
    async fn delete_gateway(&self, namespace: &str, name: &str) -> Result<()>;

    async fn create_http_route(&self, namespace: &str, route: &Value) -> Result<Value>;
    async fn get_http_route(&self, namespace: &str, name: &str) -> Result<Value>;
    async fn update_http_route(&self, namespace: &str, name: &str, route: &Value) -> Result<Value>;
    async fn delete_http_route(&self, namespace: &str, name: &str) -> Result<()>;
}

impl KubernetesClient {
    pub async fn create_ingress(&self, namespace: &str, ingress: &Ingress) -> Result<Ingress> {
        let body = serde_json::to_string(ingress).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!(
                    "Failed to serialize Ingress '{}'",
                    ingress.metadata.name.as_deref().unwrap_or("unknown")
                ),
            },
        )?;
        let url = format!(
            "{}/apis/networking.k8s.io/v1/namespaces/{}/ingresses",
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

    pub async fn get_ingress(&self, namespace: &str, name: &str) -> Result<Ingress> {
        let url = format!(
            "{}/apis/networking.k8s.io/v1/namespaces/{}/ingresses/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            urlencoding::encode(name)
        );
        let builder = self.client().request(Method::GET, &url);
        sign_send_json(builder, &self.auth_config()).await
    }

    pub async fn update_ingress(
        &self,
        namespace: &str,
        name: &str,
        ingress: &Ingress,
    ) -> Result<Ingress> {
        let body = serde_json::to_string(ingress).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize Ingress '{}'", name),
            },
        )?;
        let url = format!(
            "{}/apis/networking.k8s.io/v1/namespaces/{}/ingresses/{}",
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

    pub async fn delete_ingress(&self, namespace: &str, name: &str) -> Result<()> {
        let url = format!(
            "{}/apis/networking.k8s.io/v1/namespaces/{}/ingresses/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            urlencoding::encode(name)
        );
        let builder = self.client().request(Method::DELETE, &url);
        sign_send_no_response(builder, &self.auth_config()).await
    }

    pub async fn create_gateway(&self, namespace: &str, gateway: &Value) -> Result<Value> {
        self.create_gateway_api_resource(namespace, "gateways", gateway)
            .await
    }

    pub async fn get_gateway(&self, namespace: &str, name: &str) -> Result<Value> {
        self.get_gateway_api_resource(namespace, "gateways", name)
            .await
    }

    pub async fn update_gateway(
        &self,
        namespace: &str,
        name: &str,
        gateway: &Value,
    ) -> Result<Value> {
        self.update_gateway_api_resource(namespace, "gateways", name, gateway)
            .await
    }

    pub async fn delete_gateway(&self, namespace: &str, name: &str) -> Result<()> {
        self.delete_gateway_api_resource(namespace, "gateways", name)
            .await
    }

    pub async fn create_http_route(&self, namespace: &str, route: &Value) -> Result<Value> {
        self.create_gateway_api_resource(namespace, "httproutes", route)
            .await
    }

    pub async fn get_http_route(&self, namespace: &str, name: &str) -> Result<Value> {
        self.get_gateway_api_resource(namespace, "httproutes", name)
            .await
    }

    pub async fn update_http_route(
        &self,
        namespace: &str,
        name: &str,
        route: &Value,
    ) -> Result<Value> {
        self.update_gateway_api_resource(namespace, "httproutes", name, route)
            .await
    }

    pub async fn delete_http_route(&self, namespace: &str, name: &str) -> Result<()> {
        self.delete_gateway_api_resource(namespace, "httproutes", name)
            .await
    }

    async fn create_gateway_api_resource(
        &self,
        namespace: &str,
        plural: &str,
        value: &Value,
    ) -> Result<Value> {
        let body = serde_json::to_string(value).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize Gateway API resource '{}'", plural),
            },
        )?;
        let url = format!(
            "{}/apis/gateway.networking.k8s.io/v1/namespaces/{}/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            plural
        );
        let builder = self
            .client()
            .request(Method::POST, &url)
            .header("Content-Type", "application/json")
            .body(body);
        sign_send_json(builder, &self.auth_config()).await
    }

    async fn get_gateway_api_resource(
        &self,
        namespace: &str,
        plural: &str,
        name: &str,
    ) -> Result<Value> {
        let url = format!(
            "{}/apis/gateway.networking.k8s.io/v1/namespaces/{}/{}/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            plural,
            urlencoding::encode(name)
        );
        let builder = self.client().request(Method::GET, &url);
        sign_send_json(builder, &self.auth_config()).await
    }

    async fn update_gateway_api_resource(
        &self,
        namespace: &str,
        plural: &str,
        name: &str,
        value: &Value,
    ) -> Result<Value> {
        let body = serde_json::to_string(value).into_alien_error().context(
            ErrorData::SerializationError {
                message: format!("Failed to serialize Gateway API resource '{}'", name),
            },
        )?;
        let url = format!(
            "{}/apis/gateway.networking.k8s.io/v1/namespaces/{}/{}/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            plural,
            urlencoding::encode(name)
        );
        let builder = self
            .client()
            .request(Method::PUT, &url)
            .header("Content-Type", "application/json")
            .body(body);
        sign_send_json(builder, &self.auth_config()).await
    }

    async fn delete_gateway_api_resource(
        &self,
        namespace: &str,
        plural: &str,
        name: &str,
    ) -> Result<()> {
        let url = format!(
            "{}/apis/gateway.networking.k8s.io/v1/namespaces/{}/{}/{}",
            self.get_base_url(),
            urlencoding::encode(namespace),
            plural,
            urlencoding::encode(name)
        );
        let builder = self.client().request(Method::DELETE, &url);
        sign_send_no_response(builder, &self.auth_config()).await
    }
}

#[async_trait]
impl RouteApi for KubernetesClient {
    async fn create_ingress(&self, namespace: &str, ingress: &Ingress) -> Result<Ingress> {
        self.create_ingress(namespace, ingress).await
    }

    async fn get_ingress(&self, namespace: &str, name: &str) -> Result<Ingress> {
        self.get_ingress(namespace, name).await
    }

    async fn update_ingress(
        &self,
        namespace: &str,
        name: &str,
        ingress: &Ingress,
    ) -> Result<Ingress> {
        self.update_ingress(namespace, name, ingress).await
    }

    async fn delete_ingress(&self, namespace: &str, name: &str) -> Result<()> {
        self.delete_ingress(namespace, name).await
    }

    async fn create_gateway(&self, namespace: &str, gateway: &Value) -> Result<Value> {
        self.create_gateway(namespace, gateway).await
    }

    async fn get_gateway(&self, namespace: &str, name: &str) -> Result<Value> {
        self.get_gateway(namespace, name).await
    }

    async fn update_gateway(&self, namespace: &str, name: &str, gateway: &Value) -> Result<Value> {
        self.update_gateway(namespace, name, gateway).await
    }

    async fn delete_gateway(&self, namespace: &str, name: &str) -> Result<()> {
        self.delete_gateway(namespace, name).await
    }

    async fn create_http_route(&self, namespace: &str, route: &Value) -> Result<Value> {
        self.create_http_route(namespace, route).await
    }

    async fn get_http_route(&self, namespace: &str, name: &str) -> Result<Value> {
        self.get_http_route(namespace, name).await
    }

    async fn update_http_route(&self, namespace: &str, name: &str, route: &Value) -> Result<Value> {
        self.update_http_route(namespace, name, route).await
    }

    async fn delete_http_route(&self, namespace: &str, name: &str) -> Result<()> {
        self.delete_http_route(namespace, name).await
    }
}
