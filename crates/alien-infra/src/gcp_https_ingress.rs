//! Global public and regional private HTTPS ingress. Public ingress and workload replica registration are
//! independent: both frontends can reference the same zonal NEGs.

use alien_client_core::ErrorData as ClientErrorData;
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use alien_gcp_clients::gcp::compute::{
    Address, AddressType, BackendService, BackendServiceProtocol, ComputeApi, Firewall,
    ForwardingRule, ForwardingRuleProtocol, HealthCheck, LoadBalancingScheme, Operation,
    SslCertificate, SslCertificateSelfManaged, TargetHttpsProxy, TargetTcpProxy, UrlMap,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

use crate::{ErrorData, GcpIngressOperation, GcpIngressOperationProgress, Result};

/// Compute uses identical HTTPS resources in two different API collections.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GcpHttpsIngressScope {
    #[default]
    RegionalPrivate,
    GlobalPublic,
}

macro_rules! scoped {
    ($state:expr, $client:expr, $regional:ident, $global:ident, $($argument:expr),+ $(,)?) => {
        match $state.scope {
            GcpHttpsIngressScope::RegionalPrivate => $client.$regional($state.region.clone(), $($argument),+).await,
            GcpHttpsIngressScope::GlobalPublic => $client.$global($($argument),+).await,
        }
    };
}

/// The caller obtains certificate material at runtime; it is never saved in
/// controller state. Customer certificate references are never deleted.
pub enum GcpIngressCertificate<'a> {
    Managed {
        certificate_pem: &'a str,
        private_key_pem: &'a str,
    },
    Custom {
        references: &'a [String],
    },
}

/// Listener protocol and its runtime-only TLS material.
pub enum GcpIngressFrontend<'a> {
    Https(GcpIngressCertificate<'a>),
    Tcp { port: u16 },
}

impl GcpIngressFrontend<'_> {
    fn tcp_port(&self) -> Option<u16> {
        match self {
            Self::Https(_) => None,
            Self::Tcp { port } => Some(*port),
        }
    }
}

/// Health checks and a proxy-subnet firewall are needed by VM NEGs, but not by
/// serverless NEGs. Backend membership stays with the workload controller.
pub struct GcpProxyIngressPlan<'a> {
    pub scope: GcpHttpsIngressScope,
    pub resource_id: &'a str,
    pub project: &'a str,
    pub region: &'a str,
    pub network: &'a str,
    pub subnetwork: &'a str,
    pub firewall: Option<Firewall>,
    pub health_check: Option<HealthCheck>,
    pub backend: BackendService,
    pub frontend: GcpIngressFrontend<'a>,
}

/// Compatibility name for callers preparing HTTPS frontends.
pub type GcpHttpsIngressPlan<'a> = GcpProxyIngressPlan<'a>;

/// A child recorded by the previous controller, never a name discovered by a
/// broad listing. A saved provider ID, when available, must still match.
pub struct GcpRecordedIngressChild<'a> {
    pub name: &'a str,
    pub description: &'a str,
    pub provider_id: Option<&'a str>,
}

/// Explicit handoff of a previously recorded global HTTPS graph. The graph's
/// references, ownership descriptions and immutable IDs are verified before
/// this controller can change or delete any of its children.
pub struct GcpRecordedProxyIngress<'a> {
    pub health_check: Option<GcpRecordedIngressChild<'a>>,
    pub backend: GcpRecordedIngressChild<'a>,
    pub url_map: Option<GcpRecordedIngressChild<'a>>,
    pub proxy: GcpRecordedIngressChild<'a>,
    pub address: GcpRecordedIngressChild<'a>,
    pub frontend: GcpRecordedIngressChild<'a>,
    /// Only owned imports belong here. Customer certificate references remain
    /// external and are checked against the desired custom certificate list.
    pub managed_certificate: Option<GcpRecordedIngressChild<'a>>,
    pub certificate_references: &'a [String],
}

/// Compatibility name for the recorded HTTPS handoff.
pub type GcpRecordedHttpsIngress<'a> = GcpRecordedProxyIngress<'a>;

/// Saved intent covers partial creates and lost responses. A random lifetime
/// token distinguishes this instance from same-named leftovers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GcpProxyIngressState {
    #[serde(default)]
    tcp_port: Option<u16>,
    #[serde(default)]
    scope: GcpHttpsIngressScope,
    project: String,
    region: String,
    network: String,
    subnetwork: String,
    token: Uuid,
    #[serde(default)]
    children: BTreeSet<Child>,
    #[serde(default)]
    provider_ids: BTreeMap<String, String>,
    /// Recorded children from an older controller retain their original names
    /// and ownership descriptions after a verified handoff.
    #[serde(default)]
    legacy_names: BTreeMap<String, String>,
    #[serde(default)]
    legacy_descriptions: BTreeMap<String, String>,
    #[serde(default)]
    operation: Option<GcpIngressOperation>,
    #[serde(default, rename = "privateIp")]
    pub address: Option<String>,
}

/// Compatibility name for previously persisted HTTPS controllers.
pub type GcpHttpsIngressState = GcpProxyIngressState;

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
    TcpProxy(String),
    Address(String),
    Frontend(String),
}

impl Child {
    fn kind(&self) -> &'static str {
        match self {
            Self::Firewall(_) => "firewall",
            Self::HealthCheck(_) => "healthCheck",
            Self::Backend(_) => "backend",
            Self::UrlMap(_) => "urlMap",
            Self::Certificate(_) => "certificate",
            Self::HttpsProxy(_) => "httpsProxy",
            Self::TcpProxy(_) => "tcpProxy",
            Self::Address(_) => "address",
            Self::Frontend(_) => "frontend",
        }
    }

    fn identity(&self) -> String {
        format!("{}/{}", self.kind(), self.name())
    }

    fn name(&self) -> &str {
        match self {
            Self::Firewall(name)
            | Self::HealthCheck(name)
            | Self::Backend(name)
            | Self::UrlMap(name)
            | Self::Certificate(name)
            | Self::HttpsProxy(name)
            | Self::TcpProxy(name)
            | Self::Address(name)
            | Self::Frontend(name) => name,
        }
    }

    async fn read(
        &self,
        client: &dyn ComputeApi,
        state: &GcpHttpsIngressState,
    ) -> alien_client_core::Result<Value> {
        let name = self.name().to_string();
        let value = match self {
            Self::Firewall(_) => serde_json::to_value(client.get_firewall(name).await?),
            Self::HealthCheck(_) => serde_json::to_value(scoped!(
                state,
                client,
                get_region_health_check,
                get_health_check,
                name
            )?),
            Self::Backend(_) => serde_json::to_value(scoped!(
                state,
                client,
                get_region_backend_service,
                get_backend_service,
                name
            )?),
            Self::UrlMap(_) => serde_json::to_value(scoped!(
                state,
                client,
                get_region_url_map,
                get_url_map,
                name
            )?),
            Self::Certificate(_) => serde_json::to_value(scoped!(
                state,
                client,
                get_region_ssl_certificate,
                get_ssl_certificate,
                name
            )?),
            Self::HttpsProxy(_) => serde_json::to_value(scoped!(
                state,
                client,
                get_region_target_https_proxy,
                get_target_https_proxy,
                name
            )?),
            Self::TcpProxy(_) => serde_json::to_value(scoped!(
                state,
                client,
                get_region_target_tcp_proxy,
                get_target_tcp_proxy,
                name
            )?),
            Self::Address(_) => serde_json::to_value(scoped!(
                state,
                client,
                get_address,
                get_global_address,
                name
            )?),
            Self::Frontend(_) => serde_json::to_value(scoped!(
                state,
                client,
                get_forwarding_rule,
                get_global_forwarding_rule,
                name
            )?),
        };
        value
            .into_alien_error()
            .context(ClientErrorData::SerializationError {
                message: "Serialize ingress metadata".to_string(),
            })
    }

    async fn delete(
        &self,
        client: &dyn ComputeApi,
        state: &GcpHttpsIngressState,
    ) -> alien_client_core::Result<Operation> {
        let name = self.name().to_string();
        match self {
            Self::Firewall(_) => client.delete_firewall(name).await,
            Self::HealthCheck(_) => scoped!(
                state,
                client,
                delete_region_health_check,
                delete_health_check,
                name
            ),
            Self::Backend(_) => scoped!(
                state,
                client,
                delete_region_backend_service,
                delete_backend_service,
                name
            ),
            Self::UrlMap(_) => scoped!(state, client, delete_region_url_map, delete_url_map, name),
            Self::Certificate(_) => scoped!(
                state,
                client,
                delete_region_ssl_certificate,
                delete_ssl_certificate,
                name
            ),
            Self::HttpsProxy(_) => scoped!(
                state,
                client,
                delete_region_target_https_proxy,
                delete_target_https_proxy,
                name
            ),
            Self::TcpProxy(_) => scoped!(
                state,
                client,
                delete_region_target_tcp_proxy,
                delete_target_tcp_proxy,
                name
            ),
            Self::Address(_) => scoped!(state, client, delete_address, delete_global_address, name),
            Self::Frontend(_) => scoped!(
                state,
                client,
                delete_forwarding_rule,
                delete_global_forwarding_rule,
                name
            ),
        }
    }
}

impl GcpProxyIngressState {
    fn load_balancing_scheme(&self) -> LoadBalancingScheme {
        match (self.scope, self.tcp_port.is_some()) {
            (GcpHttpsIngressScope::RegionalPrivate, _) => LoadBalancingScheme::InternalManaged,
            (GcpHttpsIngressScope::GlobalPublic, true) => LoadBalancingScheme::ExternalManaged,
            (GcpHttpsIngressScope::GlobalPublic, false) => LoadBalancingScheme::External,
        }
    }

    pub fn new(plan: &GcpHttpsIngressPlan<'_>) -> Self {
        Self {
            tcp_port: plan.frontend.tcp_port(),
            scope: plan.scope,
            project: plan.project.to_string(),
            region: plan.region.to_string(),
            network: plan.network.to_string(),
            subnetwork: plan.subnetwork.to_string(),
            token: Uuid::new_v4(),
            children: BTreeSet::new(),
            provider_ids: BTreeMap::new(),
            legacy_names: BTreeMap::new(),
            legacy_descriptions: BTreeMap::new(),
            operation: None,
            address: None,
        }
    }

    /// Read-only migration from explicit saved child names. Callers must save
    /// the returned state before retiring their old ownership fields. A failed
    /// proof performs no provider mutation and leaves the old owner intact.
    pub async fn from_recorded_global(
        client: &dyn ComputeApi,
        plan: &GcpHttpsIngressPlan<'_>,
        recorded: GcpRecordedHttpsIngress<'_>,
    ) -> Result<Self> {
        let mut state = Self::new(plan);
        let tcp = plan.frontend.tcp_port().is_some();
        if plan.scope != GcpHttpsIngressScope::GlobalPublic
            || plan.firewall.is_some()
            || plan.health_check.is_some() != recorded.health_check.is_some()
            || (!tcp && (recorded.certificate_references.is_empty() || recorded.url_map.is_none()))
            || (tcp
                && (recorded.url_map.is_some()
                    || recorded.managed_certificate.is_some()
                    || !recorded.certificate_references.is_empty()
                    || plan.backend.protocol != Some(BackendServiceProtocol::Tcp)
                    || plan.health_check.is_none()
                    || state.tcp_port == Some(0)))
        {
            return Err(AlienError::new(
                state.error(plan.resource_id, "Invalid recorded global proxy graph"),
            ));
        }
        let references = recorded.certificate_references;
        let mut children = vec![
            (
                Child::Backend(recorded.backend.name.to_string()),
                recorded.backend,
            ),
            (
                if tcp {
                    Child::TcpProxy(recorded.proxy.name.to_string())
                } else {
                    Child::HttpsProxy(recorded.proxy.name.to_string())
                },
                recorded.proxy,
            ),
            (
                Child::Address(recorded.address.name.to_string()),
                recorded.address,
            ),
            (
                Child::Frontend(recorded.frontend.name.to_string()),
                recorded.frontend,
            ),
        ];
        if let Some(child) = recorded.url_map {
            children.push((Child::UrlMap(child.name.to_string()), child));
        }
        if let Some(child) = recorded.health_check {
            children.push((Child::HealthCheck(child.name.to_string()), child));
        }
        if let Some(child) = recorded.managed_certificate {
            if !contains_desired(
                &serde_json::json!(references),
                &serde_json::json!([state.url("sslCertificates", child.name)]),
            ) {
                return Err(AlienError::new(state.error(
                    plan.resource_id,
                    "Recorded certificate is not the proxy's sole certificate",
                )));
            }
            children.push((Child::Certificate(child.name.to_string()), child));
        }
        let mut live = BTreeMap::new();
        for (child, recorded) in children {
            if recorded.name.is_empty() || recorded.description.is_empty() {
                return Err(AlienError::new(state.error(
                    plan.resource_id,
                    "Recorded ingress child requires a name and ownership description",
                )));
            }
            if !matches!(child, Child::Certificate(_)) {
                state
                    .legacy_names
                    .insert(child.kind().to_string(), recorded.name.to_string());
            }
            state
                .legacy_descriptions
                .insert(child.identity(), recorded.description.to_string());
            if let Some(id) = recorded.provider_id {
                state.provider_ids.insert(child.identity(), id.to_string());
            }
            let object = child.read(client, &state).await.context(state.error(
                plan.resource_id,
                format!("Read recorded ingress child '{}'", child.name()),
            ))?;
            state.check_owner(&object, plan.resource_id, &child)?;
            live.insert(child.kind().to_string(), object);
            state.children.insert(child);
        }
        let expected_backend = state.url("backendServices", &state.child_name("backend"));
        let health_checks = if plan.health_check.is_some() {
            vec![state.url("healthChecks", &state.child_name("healthCheck"))]
        } else {
            Vec::new()
        };
        let address = live["address"]
            .get("address")
            .and_then(Value::as_str)
            .filter(|address| !address.is_empty())
            .ok_or_else(|| {
                AlienError::new(state.error(plan.resource_id, "Recorded global address has no IP"))
            })?;
        let groups = live["backend"]
            .get("backends")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                AlienError::new(
                    state.error(plan.resource_id, "Recorded backend has no NEG membership"),
                )
            })?;
        let mut seen = BTreeSet::new();
        let known_groups =
            groups.iter().all(|backend| {
                let Some(group) = backend
                    .get("group")
                    .and_then(Value::as_str)
                    .and_then(resource_path)
                else {
                    return false;
                };
                seen.insert(group)
                    && plan.backend.backends.iter().any(|desired| {
                        desired.group.as_deref().and_then(resource_path) == Some(group)
                    })
            });
        let proxy_matches = if tcp {
            contains_desired(
                &live["tcpProxy"],
                &serde_json::json!({"service": expected_backend, "proxyHeader": "NONE"}),
            )
        } else {
            contains_desired(
                &live["urlMap"],
                &serde_json::json!({"defaultService":expected_backend}),
            ) && only_recorded_backend(&live["urlMap"], &expected_backend)
                && contains_desired(
                    &live["httpsProxy"],
                    &serde_json::json!({"urlMap":state.url("urlMaps", &state.child_name("urlMap")), "sslCertificates":references}),
                )
        };
        let proxy_url = if tcp {
            state.url("targetTcpProxies", &state.child_name("tcpProxy"))
        } else {
            state.url("targetHttpsProxies", &state.child_name("httpsProxy"))
        };
        let port = state.tcp_port.unwrap_or(443).to_string();
        let port_range = format!("{port}-{port}");
        if !known_groups
            || groups.is_empty()
            || !contains_desired(
                &live["backend"],
                &serde_json::json!({
                    "protocol": plan.backend.protocol.as_ref().unwrap_or(&BackendServiceProtocol::Http),
                    "loadBalancingScheme":state.load_balancing_scheme()
                }),
            )
            || !live["backend"]
                .get("healthChecks")
                .map(|live| contains_desired(live, &serde_json::json!(health_checks)))
                .unwrap_or(health_checks.is_empty())
            || !proxy_matches
            || !contains_desired(
                &live["address"],
                &serde_json::json!({"addressType":"EXTERNAL"}),
            )
            || !contains_desired(
                &live["frontend"],
                &serde_json::json!({"target":proxy_url, "IPAddress":address, "IPProtocol":"TCP", "loadBalancingScheme":state.load_balancing_scheme()}),
            )
            || !live["frontend"]
                .get("portRange")
                .and_then(Value::as_str)
                .is_some_and(|value| value == port || value == port_range)
        {
            return Err(AlienError::new(state.error(
                plan.resource_id,
                "Recorded proxy graph has foreign or inconsistent references",
            )));
        }
        state.address = Some(address.to_string());
        Ok(state)
    }

    fn name(&self) -> String {
        format!(
            "{}-{}",
            if self.scope == GcpHttpsIngressScope::RegionalPrivate {
                "private"
            } else {
                "public"
            },
            self.token.simple()
        )
    }
    fn owner(&self) -> String {
        match self.scope {
            GcpHttpsIngressScope::RegionalPrivate => {
                format!("Alien private ingress {}", self.token)
            }
            GcpHttpsIngressScope::GlobalPublic => format!("Alien public ingress {}", self.token),
        }
    }
    fn url(&self, collection: &str, name: &str) -> String {
        let scope = match self.scope {
            GcpHttpsIngressScope::RegionalPrivate => format!("regions/{}", self.region),
            GcpHttpsIngressScope::GlobalPublic => "global".to_string(),
        };
        format!(
            "https://www.googleapis.com/compute/v1/projects/{}/{scope}/{collection}/{name}",
            self.project
        )
    }

    pub fn project_id(&self) -> &str {
        &self.project
    }

    pub fn backend_name(&self) -> String {
        self.child_name("backend")
    }

    fn record_operation(
        &mut self,
        operation: Operation,
        resource_id: &str,
        child: &Child,
    ) -> Result<()> {
        let mut operation = GcpIngressOperation::from_command(operation, resource_id)?;
        operation.region = if self.scope == GcpHttpsIngressScope::RegionalPrivate
            && !matches!(child, Child::Firewall(_))
        {
            Some(self.region.clone())
        } else {
            None
        };
        self.operation = Some(operation);
        Ok(())
    }

    fn error(&self, resource_id: &str, message: impl Into<String>) -> ErrorData {
        ErrorData::CloudPlatformError {
            message: message.into(),
            resource_id: Some(resource_id.to_string()),
        }
    }

    fn check_owner(&mut self, live: &Value, resource_id: &str, child: &Child) -> Result<()> {
        if live.get("description").and_then(Value::as_str) != Some(self.child_owner(child).as_str())
        {
            return Err(AlienError::new(self.error(
                resource_id,
                format!("Refusing foreign private ingress child '{}'", child.name()),
            )));
        }
        let id = live
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                AlienError::new(self.error(
                    resource_id,
                    format!("Ingress child '{}' has no provider ID", child.name()),
                ))
            })?;
        let identity = child.identity();
        if self
            .provider_ids
            .get(&identity)
            .is_some_and(|expected| expected != id)
        {
            return Err(AlienError::new(self.error(
                resource_id,
                format!("Refusing replaced ingress child '{}'", child.name()),
            )));
        }
        self.provider_ids.insert(identity, id.to_string());
        Ok(())
    }

    fn child_name(&self, kind: &str) -> String {
        self.legacy_names
            .get(kind)
            .cloned()
            .unwrap_or_else(|| self.name())
    }

    fn child_owner(&self, child: &Child) -> String {
        self.legacy_descriptions
            .get(&child.identity())
            .cloned()
            .unwrap_or_else(|| self.owner())
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
        plan: GcpHttpsIngressPlan<'_>,
    ) -> Result<bool> {
        if self.tcp_port != plan.frontend.tcp_port() || self.tcp_port == Some(0) {
            return Err(AlienError::new(self.error(
                plan.resource_id,
                "Ingress listener protocol or port changes require replacement",
            )));
        }
        if self.tcp_port.is_some()
            && (plan.backend.protocol != Some(BackendServiceProtocol::Tcp)
                || plan.health_check.is_none())
        {
            return Err(AlienError::new(self.error(
                plan.resource_id,
                "TCP ingress requires a TCP backend and health check",
            )));
        }
        if self.scope == GcpHttpsIngressScope::GlobalPublic && plan.firewall.is_some() {
            return Err(AlienError::new(self.error(
                plan.resource_id,
                "A proxy subnet firewall belongs to regional private ingress",
            )));
        }
        if plan.backend.backends.is_empty() {
            return Err(AlienError::new(
                self.error(plan.resource_id, "Ingress requires at least one NEG"),
            ));
        }
        if self.scope != plan.scope
            || self.project != plan.project
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
        let certificate_name = match &plan.frontend {
            GcpIngressFrontend::Https(GcpIngressCertificate::Managed {
                certificate_pem, ..
            }) => {
                let hash = format!("{:x}", Sha256::digest(certificate_pem.as_bytes()));
                Some(format!("{name}-{}", &hash[..12]))
            }
            GcpIngressFrontend::Https(GcpIngressCertificate::Custom { references }) => {
                if references.is_empty() {
                    return Err(AlienError::new(
                        self.error(plan.resource_id, "Private ingress requires a certificate"),
                    ));
                }
                None
            }
            GcpIngressFrontend::Tcp { .. } => None,
        };
        let certificates = match &plan.frontend {
            GcpIngressFrontend::Https(GcpIngressCertificate::Custom { references }) => {
                references.to_vec()
            }
            GcpIngressFrontend::Https(GcpIngressCertificate::Managed { .. }) => {
                vec![self.url("sslCertificates", certificate_name.as_ref().unwrap())]
            }
            GcpIngressFrontend::Tcp { .. } => Vec::new(),
        };
        let mut children = Vec::new();
        if plan.firewall.is_some() {
            children.push(Child::Firewall(self.child_name("firewall")));
        }
        if plan.health_check.is_some() {
            children.push(Child::HealthCheck(self.child_name("healthCheck")));
        }
        children.push(Child::Backend(self.child_name("backend")));
        if self.tcp_port.is_none() {
            children.push(Child::UrlMap(self.child_name("urlMap")));
        }
        if let Some(certificate_name) = &certificate_name {
            children.push(Child::Certificate(certificate_name.clone()));
        }
        children.extend([
            if self.tcp_port.is_some() {
                Child::TcpProxy(self.child_name("tcpProxy"))
            } else {
                Child::HttpsProxy(self.child_name("httpsProxy"))
            },
            Child::Address(self.child_name("address")),
            Child::Frontend(self.child_name("frontend")),
        ]);

        for child in &children {
            let name = child.name().to_string();
            let description = self.child_owner(child);
            // Save intent before a create can succeed with a lost response.
            self.children.insert(child.clone());
            let live = match child.read(client, self).await {
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
                    if self.provider_ids.contains_key(&child.identity()) {
                        return Err(AlienError::new(self.error(
                            plan.resource_id,
                            format!(
                                "Previously confirmed ingress child '{}' is missing",
                                child.name()
                            ),
                        )));
                    }
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
                        scoped!(self, client, patch_region_health_check, patch_health_check, name.clone(), desired)
                    } else {
                        scoped!(self, client, insert_region_health_check, insert_health_check, desired)
                    }
                }
                Child::Backend(_) => {
                    let mut desired = plan.backend.clone();
                    desired.name = Some(name.clone());
                    desired.description = Some(description.clone());
                    // A handed-off serverless graph can already serve HTTPS.
                    // Access changes must preserve its recorded backend protocol.
                    desired.protocol.get_or_insert(BackendServiceProtocol::Http);
                    desired.load_balancing_scheme = Some(self.load_balancing_scheme());
                    desired.health_checks = if plan.health_check.is_some() {
                        vec![self.url("healthChecks", &self.child_name("healthCheck"))]
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
                        scoped!(self, client, patch_region_backend_service, patch_backend_service, name.clone(),
                                desired)
                    } else {
                        scoped!(self, client, insert_region_backend_service, insert_backend_service, desired)
                    }
                }
                Child::UrlMap(_) => {
                    if let Some(live) = &live {
                        if !contains_desired(live, &serde_json::json!({"defaultService": self.url("backendServices", &self.child_name("backend"))})) {
                            return Err(AlienError::new(self.error(plan.resource_id, "Private URL map points to an unexpected backend")));
                        }
                        continue;
                    }
                    scoped!(self, client, insert_region_url_map, insert_url_map, UrlMap {
                                name: Some(name.clone()),
                                description: Some(description.clone()),
                                default_service: Some(self.url("backendServices", &self.child_name("backend"))),
                                ..Default::default()
                            })
                }
                Child::Certificate(certificate_name) => {
                    if live.is_some() {
                        continue;
                    }
                    let GcpIngressFrontend::Https(GcpIngressCertificate::Managed {
                        certificate_pem,
                        private_key_pem,
                    }) = &plan.frontend
                    else {
                        unreachable!()
                    };
                    scoped!(self, client, insert_region_ssl_certificate, insert_ssl_certificate, SslCertificate {
                                name: Some(certificate_name.clone()),
                                description: Some(description.clone()),
                                r#type: Some("SELF_MANAGED".to_string()),
                                self_managed: Some(SslCertificateSelfManaged {
                                    certificate: Some(certificate_pem.to_string()),
                                    private_key: Some(private_key_pem.to_string()),
                                }),
                                ..Default::default()
                            })
                }
                Child::HttpsProxy(_) => {
                    if let Some(live) = &live {
                        if !contains_desired(live, &serde_json::json!({"urlMap": self.url("urlMaps", &self.child_name("urlMap"))})) {
                            return Err(AlienError::new(self.error(plan.resource_id, "Private HTTPS proxy points to an unexpected URL map")));
                        }
                        if live.get("sslCertificates").is_some_and(|live| contains_desired(live, &serde_json::json!(certificates))) {
                            continue;
                        }
                        scoped!(self, client, set_region_target_https_proxy_ssl_certificates, set_target_https_proxy_ssl_certificates, name.clone(),
                                certificates.clone())
                    } else {
                        scoped!(self, client, insert_region_target_https_proxy, insert_target_https_proxy, TargetHttpsProxy {
                                    name: Some(name.clone()),
                                    description: Some(description.clone()),
                                    url_map: Some(self.url("urlMaps", &self.child_name("urlMap"))),
                                    ssl_certificates: Some(certificates.clone()),
                                    ..Default::default()
                                })
                    }
                }
                Child::TcpProxy(_) => {
                    let desired = TargetTcpProxy {
                        name: Some(name.clone()), description: Some(description.clone()),
                        service: self.url("backendServices", &self.child_name("backend")),
                        proxy_header: Some("NONE".to_string()), ..Default::default()
                    };
                    if let Some(live) = &live {
                        if !contains_desired(live, &as_json(&desired, plan.resource_id)?) {
                            return Err(AlienError::new(self.error(plan.resource_id, "TCP proxy points to an unexpected backend")));
                        }
                        continue;
                    }
                    scoped!(self, client, insert_region_target_tcp_proxy, insert_target_tcp_proxy, desired)
                }
                Child::Address(_) => {
                    if let Some(live) = &live {
                        if self.scope == GcpHttpsIngressScope::RegionalPrivate && !contains_desired(live, &serde_json::json!({"addressType": "INTERNAL", "subnetwork": self.subnetwork})) {
                            return Err(AlienError::new(self.error(plan.resource_id, "Private address is outside its reserved subnet")));
                        }
                        self.address = Some(
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
                    scoped!(self, client, insert_address, insert_global_address, Address {
                                name: Some(name.clone()),
                                description: Some(description.clone()),
                                address_type: Some(if self.scope == GcpHttpsIngressScope::RegionalPrivate { AddressType::Internal } else { AddressType::External }),
                                subnetwork: (self.scope == GcpHttpsIngressScope::RegionalPrivate).then(|| self.subnetwork.clone()),
                                ..Default::default()
                            })
                }
                Child::Frontend(_) => {
                    let desired = ForwardingRule {
                        name: Some(name.clone()),
                        description: Some(description.clone()),
                        ip_address: self.address.clone(),
                        ip_protocol: Some(ForwardingRuleProtocol::Tcp),
                        port_range: Some({
                            let port = self.tcp_port.unwrap_or(443).to_string();
                            let range = format!("{port}-{port}");
                            live.as_ref().and_then(|live| live.get("portRange")).and_then(Value::as_str)
                                .filter(|value| *value == port || *value == range).unwrap_or(&port).to_string()
                        }),
                        load_balancing_scheme: Some(self.load_balancing_scheme()),
                        network: (self.scope == GcpHttpsIngressScope::RegionalPrivate).then(|| self.network.clone()),
                        subnetwork: (self.scope == GcpHttpsIngressScope::RegionalPrivate).then(|| self.subnetwork.clone()),
                        target: Some(if self.tcp_port.is_some() { self.url("targetTcpProxies", &self.child_name("tcpProxy")) } else { self.url("targetHttpsProxies", &self.child_name("httpsProxy")) }),
                        allow_global_access: (self.scope == GcpHttpsIngressScope::RegionalPrivate).then_some(true),
                        ..Default::default()
                    };
                    if let Some(live) = &live {
                        if !contains_desired(live, &as_json(&desired, plan.resource_id)?) {
                            return Err(AlienError::new(self.error(
                                plan.resource_id,
                                "Forwarding rule differs from its reserved configuration",
                            )));
                        }
                        continue;
                    }
                    scoped!(self, client, insert_forwarding_rule, insert_global_forwarding_rule, desired)
                }
            }
            .context(self.error(
                plan.resource_id,
                format!("Reconcile proxy ingress '{}'", child.name()),
            ))?;
            self.record_operation(operation, plan.resource_id, child)?;
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
        match child.read(client, self).await {
            Ok(live) => self.check_owner(&live, resource_id, child)?,
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                self.children.remove(child);
                self.provider_ids.remove(&child.identity());
                self.legacy_descriptions.remove(&child.identity());
                return Ok(());
            }
            Err(error) => {
                return Err(error.context(self.error(
                    resource_id,
                    format!("Read private ingress '{}' before delete", child.name()),
                )))
            }
        }
        match child.delete(client, self).await {
            Ok(operation) => {
                self.record_operation(operation, resource_id, child)?;
            }
            Err(error)
                if matches!(
                    error.error,
                    Some(ClientErrorData::RemoteResourceNotFound { .. })
                ) =>
            {
                self.children.remove(child);
                self.provider_ids.remove(&child.identity());
                self.legacy_descriptions.remove(&child.identity());
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
        self.address = None;
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

fn only_recorded_backend(value: &Value, backend: &str) -> bool {
    match value {
        Value::Object(fields) => fields.iter().all(|(key, value)| {
            if matches!(key.as_str(), "service" | "defaultService") && !value.is_null() {
                value.as_str().and_then(resource_path) == resource_path(backend)
            } else if matches!(key.as_str(), "urlRedirect" | "defaultUrlRedirect")
                && !value.is_null()
            {
                false
            } else {
                only_recorded_backend(value, backend)
            }
        }),
        Value::Array(values) => values
            .iter()
            .all(|value| only_recorded_backend(value, backend)),
        _ => true,
    }
}

#[cfg(test)]
#[path = "gcp_https_ingress_tests.rs"]
pub(crate) mod tests;
