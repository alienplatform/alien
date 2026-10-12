//! Regional HTTPS ingress. Public ingress and workload replica registration are
//! independent: both frontends can reference the same zonal NEGs.

use alien_client_core::ErrorData as ClientErrorData;
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_gcp_clients::gcp::compute::{
    Address, AddressType, BackendService, BackendServiceProtocol, ComputeApi, Firewall,
    ForwardingRule, ForwardingRuleProtocol, HealthCheck, LoadBalancingScheme, Operation,
    SslCertificate, SslCertificateSelfManaged, TargetHttpsProxy, UrlMap,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use uuid::Uuid;

use crate::{ErrorData, GcpIngressOperation, GcpIngressOperationProgress, Result};

/// The caller obtains certificate material at runtime; it is never saved in
/// controller state. Customer certificate references are never deleted.
pub enum GcpPrivateIngressCertificate<'a> {
    Managed {
        certificate_pem: &'a str,
        private_key_pem: &'a str,
    },
    Custom {
        references: &'a [String],
    },
}

/// Health checks and a proxy-subnet firewall are needed by VM NEGs, but not by
/// serverless NEGs. Backend membership stays with the workload controller.
pub struct GcpPrivateHttpsPlan<'a> {
    pub resource_id: &'a str,
    pub project: &'a str,
    pub region: &'a str,
    pub network: &'a str,
    pub subnetwork: &'a str,
    pub firewall: Option<Firewall>,
    pub health_check: Option<HealthCheck>,
    pub backend: BackendService,
    pub certificate: GcpPrivateIngressCertificate<'a>,
}

/// Saved intent covers partial creates and lost responses. A random lifetime
/// token distinguishes this instance from same-named leftovers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GcpPrivateIngressState {
    project: String,
    region: String,
    network: String,
    subnetwork: String,
    token: Uuid,
    #[serde(default)]
    children: BTreeSet<Child>,
    #[serde(default)]
    operation: Option<GcpIngressOperation>,
    #[serde(default)]
    pub private_ip: Option<String>,
}

/// Declaration order is dependency order; reverse order is safe teardown.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "name", rename_all = "camelCase")]
enum Child {
    Firewall(String),
    HealthCheck(String),
    Backend(String),
    UrlMap(String),
    Certificate(String),
    HttpsProxy(String),
    Address(String),
    Frontend(String),
}

impl Child {
    fn name(&self) -> &str {
        match self {
            Self::Firewall(name)
            | Self::HealthCheck(name)
            | Self::Backend(name)
            | Self::UrlMap(name)
            | Self::Certificate(name)
            | Self::HttpsProxy(name)
            | Self::Address(name)
            | Self::Frontend(name) => name,
        }
    }

    async fn read(
        &self,
        client: &dyn ComputeApi,
        region: &str,
    ) -> alien_client_core::Result<Value> {
        // Provider models contain only JSON values. Serialization is infallible
        // for these types; keep failures structured if that contract changes.
        macro_rules! read {
            ($request:expr) => {
                serde_json::to_value($request.await?)
                    .into_alien_error()
                    .context(ClientErrorData::SerializationError {
                        message: "serialize ingress metadata".to_string(),
                    })
            };
        }
        let name = self.name().to_string();
        let region = region.to_string();
        match self {
            Self::Firewall(_) => read!(client.get_firewall(name)),
            Self::HealthCheck(_) => read!(client.get_region_health_check(region, name)),
            Self::Backend(_) => read!(client.get_region_backend_service(region, name)),
            Self::UrlMap(_) => read!(client.get_region_url_map(region, name)),
            Self::Certificate(_) => read!(client.get_region_ssl_certificate(region, name)),
            Self::HttpsProxy(_) => read!(client.get_region_target_https_proxy(region, name)),
            Self::Address(_) => read!(client.get_address(region, name)),
            Self::Frontend(_) => read!(client.get_forwarding_rule(region, name)),
        }
    }

    async fn delete(
        &self,
        client: &dyn ComputeApi,
        region: &str,
    ) -> alien_client_core::Result<Operation> {
        let name = self.name().to_string();
        let region = region.to_string();
        match self {
            Self::Firewall(_) => client.delete_firewall(name).await,
            Self::HealthCheck(_) => client.delete_region_health_check(region, name).await,
            Self::Backend(_) => client.delete_region_backend_service(region, name).await,
            Self::UrlMap(_) => client.delete_region_url_map(region, name).await,
            Self::Certificate(_) => client.delete_region_ssl_certificate(region, name).await,
            Self::HttpsProxy(_) => client.delete_region_target_https_proxy(region, name).await,
            Self::Address(_) => client.delete_address(region, name).await,
            Self::Frontend(_) => client.delete_forwarding_rule(region, name).await,
        }
    }
}

impl GcpPrivateIngressState {
    pub fn new(plan: &GcpPrivateHttpsPlan<'_>) -> Self {
        Self {
            project: plan.project.to_string(),
            region: plan.region.to_string(),
            network: plan.network.to_string(),
            subnetwork: plan.subnetwork.to_string(),
            token: Uuid::new_v4(),
            children: BTreeSet::new(),
            operation: None,
            private_ip: None,
        }
    }

    fn name(&self) -> String {
        format!("private-{}", self.token.simple())
    }
    fn owner(&self) -> String {
        format!("Alien private ingress {}", self.token)
    }
    fn url(&self, collection: &str, name: &str) -> String {
        format!(
            "https://www.googleapis.com/compute/v1/projects/{}/regions/{}/{collection}/{name}",
            self.project, self.region
        )
    }

    fn error(&self, resource_id: &str, message: impl Into<String>) -> ErrorData {
        ErrorData::CloudPlatformError {
            message: message.into(),
            resource_id: Some(resource_id.to_string()),
        }
    }

    fn check_owner(&self, live: &Value, resource_id: &str, child: &Child) -> Result<()> {
        if live.get("description").and_then(Value::as_str) != Some(self.owner().as_str()) {
            return Err(AlienError::new(self.error(
                resource_id,
                format!("Refusing foreign private ingress child '{}'", child.name()),
            )));
        }
        Ok(())
    }

    async fn poll(&mut self, client: &dyn ComputeApi, resource_id: &str) -> Result<bool> {
        let Some(operation) = &mut self.operation else {
            return Ok(false);
        };
        match operation.poll(client).await? {
            GcpIngressOperationProgress::Pending => Ok(true),
            GcpIngressOperationProgress::Complete | GcpIngressOperationProgress::Rediscover => {
                self.operation = None;
                // End this step: the next one confirms the child through GET.
                Ok(true)
            }
            GcpIngressOperationProgress::Failed(message) => {
                self.operation = None;
                Err(AlienError::new(self.error(resource_id, message)))
            }
        }
    }

    pub fn reset_poll_count(&mut self) {
        if let Some(operation) = &mut self.operation {
            operation.reset_poll_count();
        }
    }

    /// One mutation at most. False means the caller should return a delay and
    /// step again; true means the private frontend was read and verified.
    pub async fn reconcile(
        &mut self,
        client: &dyn ComputeApi,
        plan: GcpPrivateHttpsPlan<'_>,
    ) -> Result<bool> {
        if plan.backend.backends.is_empty() {
            return Err(AlienError::new(self.error(
                plan.resource_id,
                "Private ingress requires at least one NEG",
            )));
        }
        if self.project != plan.project
            || self.region != plan.region
            || self.network != plan.network
            || self.subnetwork != plan.subnetwork
        {
            return Err(AlienError::new(self.error(
                plan.resource_id,
                "Private ingress network/project/region changes require replacement",
            )));
        }
        if self.poll(client, plan.resource_id).await? {
            return Ok(false);
        }
        let name = self.name();
        let description = self.owner();
        let certificate_name = match &plan.certificate {
            GcpPrivateIngressCertificate::Managed {
                certificate_pem, ..
            } => {
                let hash = format!("{:x}", Sha256::digest(certificate_pem.as_bytes()));
                Some(format!("{name}-{}", &hash[..12]))
            }
            GcpPrivateIngressCertificate::Custom { references } => {
                if references.is_empty() {
                    return Err(AlienError::new(
                        self.error(plan.resource_id, "Private ingress requires a certificate"),
                    ));
                }
                None
            }
        };
        let certificates = match &plan.certificate {
            GcpPrivateIngressCertificate::Custom { references } => references.to_vec(),
            GcpPrivateIngressCertificate::Managed { .. } => {
                vec![self.url("sslCertificates", certificate_name.as_ref().unwrap())]
            }
        };
        let mut children = Vec::new();
        if plan.firewall.is_some() {
            children.push(Child::Firewall(name.clone()));
        }
        if plan.health_check.is_some() {
            children.push(Child::HealthCheck(name.clone()));
        }
        children.extend([Child::Backend(name.clone()), Child::UrlMap(name.clone())]);
        if let Some(certificate_name) = &certificate_name {
            children.push(Child::Certificate(certificate_name.clone()));
        }
        children.extend([
            Child::HttpsProxy(name.clone()),
            Child::Address(name.clone()),
            Child::Frontend(name.clone()),
        ]);

        for child in &children {
            // Save intent before a create can succeed with a lost response.
            self.children.insert(child.clone());
            let live = match child.read(client, &self.region).await {
                Ok(live) => {
                    self.check_owner(&live, plan.resource_id, child)?;
                    Some(live)
                }
                Err(error)
                    if matches!(
                        error.error,
                        Some(ClientErrorData::RemoteResourceNotFound { .. })
                    ) =>
                {
                    None
                }
                Err(error) => {
                    return Err(error.context(self.error(
                        plan.resource_id,
                        format!("Read private ingress '{}'", child.name()),
                    )))
                }
            };
            let operation = match child {
                Child::Firewall(_) => {
                    let mut desired = plan.firewall.clone().unwrap();
                    desired.name = Some(name.clone());
                    desired.description = Some(description.clone());
                    if let Some(live) = &live {
                        if !contains_desired(live, &as_json(&desired, plan.resource_id)?) {
                            return Err(AlienError::new(self.error(
                                plan.resource_id,
                                "Private proxy firewall changes require replacement",
                            )));
                        }
                        continue;
                    }
                    client.insert_firewall(desired).await
                }
                Child::HealthCheck(_) => {
                    let mut desired = plan.health_check.clone().unwrap();
                    desired.name = Some(name.clone());
                    desired.description = Some(description.clone());
                    if let Some(live) = &live {
                        if contains_desired(live, &as_json(&desired, plan.resource_id)?) {
                            continue;
                        }
                        client
                            .patch_region_health_check(self.region.clone(), name.clone(), desired)
                            .await
                    } else {
                        client
                            .insert_region_health_check(self.region.clone(), desired)
                            .await
                    }
                }
                Child::Backend(_) => {
                    let mut desired = plan.backend.clone();
                    desired.name = Some(name.clone());
                    desired.description = Some(description.clone());
                    desired.protocol = Some(BackendServiceProtocol::Http);
                    desired.load_balancing_scheme = Some(LoadBalancingScheme::InternalManaged);
                    desired.health_checks = if plan.health_check.is_some() {
                        vec![self.url("healthChecks", &name)]
                    } else {
                        Vec::new()
                    };
                    if let Some(live) = &live {
                        if contains_desired(live, &as_json(&desired, plan.resource_id)?) {
                            continue;
                        }
                        desired.fingerprint = live
                            .get("fingerprint")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        client
                            .patch_region_backend_service(
                                self.region.clone(),
                                name.clone(),
                                desired,
                            )
                            .await
                    } else {
                        client
                            .insert_region_backend_service(self.region.clone(), desired)
                            .await
                    }
                }
                Child::UrlMap(_) => {
                    if let Some(live) = &live {
                        if !contains_desired(live, &serde_json::json!({"defaultService": self.url("backendServices", &name)})) {
                            return Err(AlienError::new(self.error(plan.resource_id, "Private URL map points to an unexpected backend")));
                        }
                        continue;
                    }
                    client
                        .insert_region_url_map(
                            self.region.clone(),
                            UrlMap {
                                name: Some(name.clone()),
                                description: Some(description.clone()),
                                default_service: Some(self.url("backendServices", &name)),
                                ..Default::default()
                            },
                        )
                        .await
                }
                Child::Certificate(certificate_name) => {
                    if live.is_some() {
                        continue;
                    }
                    let GcpPrivateIngressCertificate::Managed {
                        certificate_pem,
                        private_key_pem,
                    } = &plan.certificate
                    else {
                        unreachable!()
                    };
                    client
                        .insert_region_ssl_certificate(
                            self.region.clone(),
                            SslCertificate {
                                name: Some(certificate_name.clone()),
                                description: Some(description.clone()),
                                r#type: Some("SELF_MANAGED".to_string()),
                                self_managed: Some(SslCertificateSelfManaged {
                                    certificate: Some(certificate_pem.to_string()),
                                    private_key: Some(private_key_pem.to_string()),
                                }),
                                ..Default::default()
                            },
                        )
                        .await
                }
                Child::HttpsProxy(_) => {
                    if let Some(live) = &live {
                        if !contains_desired(live, &serde_json::json!({"urlMap": self.url("urlMaps", &name)})) {
                            return Err(AlienError::new(self.error(plan.resource_id, "Private HTTPS proxy points to an unexpected URL map")));
                        }
                        if live.get("sslCertificates").is_some_and(|live| contains_desired(live, &serde_json::json!(certificates))) {
                            continue;
                        }
                        client
                            .set_region_target_https_proxy_ssl_certificates(
                                self.region.clone(),
                                name.clone(),
                                certificates.clone(),
                            )
                            .await
                    } else {
                        client
                            .insert_region_target_https_proxy(
                                self.region.clone(),
                                TargetHttpsProxy {
                                    name: Some(name.clone()),
                                    description: Some(description.clone()),
                                    url_map: Some(self.url("urlMaps", &name)),
                                    ssl_certificates: Some(certificates.clone()),
                                    ..Default::default()
                                },
                            )
                            .await
                    }
                }
                Child::Address(_) => {
                    if let Some(live) = &live {
                        if !contains_desired(live, &serde_json::json!({"addressType": "INTERNAL", "subnetwork": self.subnetwork})) {
                            return Err(AlienError::new(self.error(plan.resource_id, "Private address is outside its reserved subnet")));
                        }
                        self.private_ip = Some(
                            live.get("address")
                                .and_then(Value::as_str)
                                .ok_or_else(|| {
                                    AlienError::new(self.error(
                                        plan.resource_id,
                                        "Reserved private address missing IP",
                                    ))
                                })?
                                .to_string(),
                        );
                        continue;
                    }
                    client
                        .insert_address(
                            self.region.clone(),
                            Address {
                                name: Some(name.clone()),
                                description: Some(description.clone()),
                                address_type: Some(AddressType::Internal),
                                subnetwork: Some(self.subnetwork.clone()),
                                ..Default::default()
                            },
                        )
                        .await
                }
                Child::Frontend(_) => {
                    let desired = ForwardingRule {
                        name: Some(name.clone()),
                        description: Some(description.clone()),
                        ip_address: self.private_ip.clone(),
                        ip_protocol: Some(ForwardingRuleProtocol::Tcp),
                        port_range: Some("443".to_string()),
                        load_balancing_scheme: Some(LoadBalancingScheme::InternalManaged),
                        network: Some(self.network.clone()),
                        subnetwork: Some(self.subnetwork.clone()),
                        target: Some(self.url("targetHttpsProxies", &name)),
                        allow_global_access: Some(true),
                        ..Default::default()
                    };
                    if let Some(live) = &live {
                        if !contains_desired(live, &as_json(&desired, plan.resource_id)?) {
                            return Err(AlienError::new(self.error(
                                plan.resource_id,
                                "Private forwarding rule differs from its reserved configuration",
                            )));
                        }
                        continue;
                    }
                    client
                        .insert_forwarding_rule(self.region.clone(), desired)
                        .await
                }
            }
            .context(self.error(
                plan.resource_id,
                format!("Reconcile private ingress '{}'", child.name()),
            ))?;
            self.operation = Some(GcpIngressOperation::from_command(
                operation,
                plan.resource_id,
            )?);
            return Ok(false);
        }
        // Certificate switching was confirmed by GET above. Only now retire
        // obsolete owned imports, including abandoned lost-response creates.
        if let Some(retired) = self
            .children
            .iter()
            .find(|child| matches!(child, Child::Certificate(_)) && !children.contains(child))
            .cloned()
        {
            self.delete_child(client, plan.resource_id, &retired)
                .await?;
            return Ok(false);
        }
        Ok(true)
    }

    async fn delete_child(
        &mut self,
        client: &dyn ComputeApi,
        resource_id: &str,
        child: &Child,
    ) -> Result<()> {
        match child.read(client, &self.region).await {
            Ok(live) => self.check_owner(&live, resource_id, child)?,
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                self.children.remove(child);
                return Ok(());
            }
            Err(error) => {
                return Err(error.context(self.error(
                    resource_id,
                    format!("Read private ingress '{}' before delete", child.name()),
                )))
            }
        }
        match child.delete(client, &self.region).await {
            Ok(operation) => {
                self.operation = Some(GcpIngressOperation::from_command(operation, resource_id)?);
            }
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                self.children.remove(child);
            }
            Err(error) => {
                return Err(error.context(self.error(
                    resource_id,
                    format!("Delete private ingress '{}'", child.name()),
                )))
            }
        }
        Ok(())
    }

    /// Delete from any checkpoint, including a create intent without a response.
    /// The caller must supply credentials scoped to the saved project.
    pub async fn delete(&mut self, client: &dyn ComputeApi, resource_id: &str) -> Result<bool> {
        self.delete_step(client, resource_id)
            .await
            .map_err(|error| {
                // A denied lookup does not prove that the composite ingress was
                // deleted. The executor otherwise accepts nested 401/403 and skips
                // every remaining child, so retain the message without that cause.
                let mut denied = matches!(error.http_status_code, Some(401 | 403))
                    || error.code == "REMOTE_ACCESS_DENIED";
                let mut cause = error.source.as_deref();
                while let Some(current) = cause {
                    denied |= matches!(current.http_status_code, Some(401 | 403))
                        || current.code == "REMOTE_ACCESS_DENIED";
                    cause = current.source.as_deref();
                }
                if denied {
                    AlienError::new(self.error(resource_id, error.message))
                } else {
                    error
                }
            })
    }

    async fn delete_step(&mut self, client: &dyn ComputeApi, resource_id: &str) -> Result<bool> {
        if self.poll(client, resource_id).await? {
            return Ok(false);
        }
        if let Some(child) = self.children.iter().next_back().cloned() {
            self.delete_child(client, resource_id, &child).await?;
            return Ok(false);
        }
        self.private_ip = None;
        Ok(true)
    }
}

fn as_json(value: &impl Serialize, resource_id: &str) -> Result<Value> {
    serde_json::to_value(value)
        .into_alien_error()
        .context(ErrorData::CloudPlatformError {
            message: "Serialize private ingress metadata".to_string(),
            resource_id: Some(resource_id.to_string()),
        })
}

/// Compare only explicitly managed fields; preserve provider defaults and
/// unrelated fields instead of replacing a provider response wholesale.
fn contains_desired(live: &Value, desired: &Value) -> bool {
    match (live, desired) {
        (Value::Object(live), Value::Object(desired)) => desired.iter().all(|(key, value)| {
            live.get(key)
                .is_some_and(|live| contains_desired(live, value))
        }),
        (Value::Array(live), Value::Array(desired)) => {
            live.len() == desired.len()
                && desired
                    .iter()
                    .all(|desired| live.iter().any(|live| contains_desired(live, desired)))
        }
        (Value::String(live), Value::String(desired)) => {
            if let (Some(live), Some(desired)) = (resource_path(live), resource_path(desired)) {
                live == desired
            } else {
                live == desired
            }
        }
        _ => live == desired,
    }
}

fn resource_path(value: &str) -> Option<&str> {
    value.find("projects/").map(|index| &value[index..])
}

#[cfg(test)]
#[path = "gcp_private_ingress_tests.rs"]
mod tests;
