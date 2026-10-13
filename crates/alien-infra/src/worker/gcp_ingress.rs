use std::collections::HashMap;

use alien_core::{
    EndpointAccess, ExposeProtocol, Network, PublicEndpoint, PublicEndpointOutput, ResourceRef,
    Worker,
};
use alien_error::{AlienError, Context};
use alien_gcp_clients::compute::{Backend, BackendService, BackendServiceProtocol};
use serde::{Deserialize, Serialize};

use crate::{
    ErrorData, GcpHttpsDomains, GcpHttpsIngressPlan, GcpHttpsIngressScope, GcpHttpsIngressState,
    GcpIngressCertificate, GcpNetworkController, GcpRecordedHttpsIngress, GcpRecordedIngressChild,
    ResourceControllerContext, Result,
};

pub(super) struct RecordedWorkerIngress<'a> {
    pub backend: &'a str,
    pub url_map: &'a str,
    pub proxy: &'a str,
    pub address: &'a str,
    pub frontend: &'a str,
    pub certificate: &'a str,
    pub custom_certificate: bool,
}

/// Frontends share a regional serverless NEG and never own the Cloud Run
/// service or its trigger-delivery URI.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GcpWorkerIngress {
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub primary_endpoint: Option<String>,
    #[serde(default)]
    pub public: Option<GcpHttpsIngressState>,
    #[serde(default)]
    pub private: Option<GcpHttpsIngressState>,
    #[serde(default)]
    pub public_endpoints: HashMap<String, PublicEndpointOutput>,
    #[serde(default)]
    pub private_endpoints: HashMap<String, PublicEndpointOutput>,
    #[serde(default)]
    pub applied_access: Option<EndpointAccess>,
    #[serde(default)]
    pub public_fingerprint: Option<String>,
    #[serde(default)]
    pub private_fingerprint: Option<String>,
    /// Keep the protocol of an explicitly handed-off public backend.
    #[serde(default)]
    pub recorded_public_protocol: Option<BackendServiceProtocol>,
}

impl GcpWorkerIngress {
    pub async fn validate_setup(
        ctx: &ResourceControllerContext<'_>,
        worker: &Worker,
    ) -> Result<()> {
        if !worker.public_endpoints.is_empty()
            && ctx
                .deployment_config
                .stack_settings
                .endpoint_access
                .private_enabled()
        {
            let network = ctx.require_dependency::<GcpNetworkController>(&ResourceRef::new(
                Network::RESOURCE_TYPE,
                "default-network".to_string(),
            ))?;
            network.require_proxy_subnet(ctx, &worker.id).await?;
        }
        Ok(())
    }

    pub fn domains_ready(ctx: &ResourceControllerContext<'_>, worker: &Worker) -> Result<bool> {
        let access = ctx.deployment_config.stack_settings.endpoint_access;
        if worker.public_endpoints.is_empty() {
            return Ok(true);
        }
        let endpoints = worker_endpoints(worker);
        for (scope, enabled) in [
            (
                GcpHttpsIngressScope::GlobalPublic,
                access.internet_enabled(),
            ),
            (
                GcpHttpsIngressScope::RegionalPrivate,
                access.private_enabled(),
            ),
        ] {
            if enabled && GcpHttpsDomains::resolve(ctx, &worker.id, &endpoints, scope)?.is_none() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub fn reset_poll_count(&mut self) {
        for state in [&mut self.public, &mut self.private].into_iter().flatten() {
            state.reset_poll_count();
        }
    }

    pub fn needs_update(
        &self,
        ctx: &ResourceControllerContext<'_>,
        worker: &Worker,
    ) -> Result<bool> {
        let access = ctx.deployment_config.stack_settings.endpoint_access;
        if self.applied_access != Some(access) {
            return Ok(true);
        }
        if worker.public_endpoints.is_empty() {
            return Ok(self.public.is_some() || self.private.is_some());
        }
        let endpoints = worker_endpoints(worker);
        for (scope, enabled, fingerprint) in [
            (
                GcpHttpsIngressScope::GlobalPublic,
                access.internet_enabled(),
                &self.public_fingerprint,
            ),
            (
                GcpHttpsIngressScope::RegionalPrivate,
                access.private_enabled(),
                &self.private_fingerprint,
            ),
        ] {
            if enabled {
                let Some(domains) = GcpHttpsDomains::resolve(ctx, &worker.id, &endpoints, scope)?
                else {
                    return Ok(true);
                };
                if fingerprint.as_deref() != Some(domains.fingerprint().as_str()) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    pub fn primary_url<'a>(
        &self,
        endpoints: &'a HashMap<String, PublicEndpointOutput>,
    ) -> Option<&'a str> {
        self.primary_endpoint
            .as_ref()
            .and_then(|name| endpoints.get(name))
            .map(|endpoint| endpoint.url.as_str())
    }

    /// Read-only handoff of the exact recorded graph. The caller checkpoints
    /// this owner together with clearing its previous ownership fields.
    pub async fn handoff(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        worker: &Worker,
        neg_name: &str,
        recorded: RecordedWorkerIngress<'_>,
    ) -> Result<()> {
        if self.public.is_some() {
            return Err(invalid(
                &worker.id,
                "Worker HTTPS graph has two recorded owners",
            ));
        }
        let config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_compute_client(config)?;
        let references = vec![format!(
            "https://www.googleapis.com/compute/v1/projects/{}/global/sslCertificates/{}",
            config.project_id, recorded.certificate
        )];
        let backend = BackendService {
            protocol: Some(BackendServiceProtocol::Https),
            backends: vec![Backend {
                group: Some(format!(
                    "projects/{}/regions/{}/networkEndpointGroups/{neg_name}",
                    config.project_id, config.region
                )),
                ..Default::default()
            }],
            ..Default::default()
        };
        let plan = GcpHttpsIngressPlan {
            scope: GcpHttpsIngressScope::GlobalPublic,
            resource_id: &worker.id,
            project: &config.project_id,
            region: &config.region,
            network: "",
            subnetwork: "",
            firewall: None,
            health_check: None,
            backend,
            frontend: crate::GcpIngressFrontend::Https(GcpIngressCertificate::Custom {
                references: &references,
            }),
        };
        let certificate_description = if recorded.custom_certificate {
            None
        } else {
            let certificate = client
                .get_ssl_certificate(recorded.certificate.to_string())
                .await
                .context(ErrorData::CloudPlatformError {
                    resource_id: Some(worker.id.clone()),
                    message: "Read recorded worker certificate before handoff".to_string(),
                })?;
            let description = certificate.description.ok_or_else(|| {
                invalid(
                    &worker.id,
                    "Recorded worker certificate has no ownership description",
                )
            })?;
            if description != format!("SSL certificate for worker {}", worker.id)
                && description != format!("Renewed SSL certificate for worker {}", worker.id)
            {
                return Err(invalid(
                    &worker.id,
                    "Refusing a foreign recorded worker certificate",
                ));
            }
            Some(description)
        };
        let backend_description = format!("Backend service for worker {}", worker.id);
        let map_description = format!("URL map for worker {}", worker.id);
        let proxy_description = format!("HTTPS proxy for worker {}", worker.id);
        let address_description = format!("Global IP for worker {}", worker.id);
        let frontend_description = format!("Forwarding rule for worker {}", worker.id);
        let child = |name, description| GcpRecordedIngressChild {
            name,
            description,
            provider_id: None,
        };
        self.public = Some(
            GcpHttpsIngressState::from_recorded_global(
                client.as_ref(),
                &plan,
                GcpRecordedHttpsIngress {
                    health_check: None,
                    backend: child(recorded.backend, &backend_description),
                    url_map: Some(child(recorded.url_map, &map_description)),
                    proxy: child(recorded.proxy, &proxy_description),
                    address: child(recorded.address, &address_description),
                    frontend: child(recorded.frontend, &frontend_description),
                    managed_certificate: certificate_description
                        .as_deref()
                        .map(|description| child(recorded.certificate, description)),
                    certificate_references: &references,
                },
            )
            .await?,
        );
        self.recorded_public_protocol = Some(BackendServiceProtocol::Https);
        Ok(())
    }

    /// Prepare the requested graphs before updating service ingress. Retirement
    /// is allowed only after the caller confirms the service's ingress update.
    /// Each invocation issues at most one durable provider mutation.
    pub async fn reconcile(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        worker: &Worker,
        neg_name: &str,
        retire: bool,
    ) -> Result<bool> {
        let config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_compute_client(config)?;
        let access = ctx.deployment_config.stack_settings.endpoint_access;
        self.primary_endpoint = worker
            .public_endpoints
            .first()
            .map(|endpoint| endpoint.name.clone());
        let endpoints = worker_endpoints(worker);
        let neg = format!(
            "https://www.googleapis.com/compute/v1/projects/{}/regions/{}/networkEndpointGroups/{neg_name}",
            config.project_id, config.region
        );
        for (scope, enabled) in [
            (
                GcpHttpsIngressScope::RegionalPrivate,
                access.private_enabled(),
            ),
            (
                GcpHttpsIngressScope::GlobalPublic,
                access.internet_enabled(),
            ),
        ] {
            if !enabled || worker.public_endpoints.is_empty() {
                continue;
            }
            let Some(domains) = GcpHttpsDomains::resolve(ctx, &worker.id, &endpoints, scope)?
            else {
                return Ok(false);
            };
            let (network, subnet) = if scope == GcpHttpsIngressScope::RegionalPrivate {
                let network = ctx.require_dependency::<GcpNetworkController>(&ResourceRef::new(
                    Network::RESOURCE_TYPE,
                    "default-network".to_string(),
                ))?;
                network.require_proxy_subnet(ctx, &worker.id).await?;
                (
                    network
                        .network_self_link
                        .ok_or_else(|| invalid(&worker.id, "Worker ingress VPC is missing"))?,
                    network
                        .subnetwork_self_link
                        .ok_or_else(|| invalid(&worker.id, "Worker ingress subnet is missing"))?,
                )
            } else {
                (String::new(), String::new())
            };
            // Serverless NEGs reject VM-only health checks, balancing modes,
            // capacity settings, and backend timeouts.
            let backend = BackendService {
                protocol: Some(if scope == GcpHttpsIngressScope::GlobalPublic {
                    self.recorded_public_protocol
                        .clone()
                        .unwrap_or(BackendServiceProtocol::Http)
                } else {
                    BackendServiceProtocol::Http
                }),
                backends: vec![Backend {
                    group: Some(neg.clone()),
                    ..Default::default()
                }],
                ..Default::default()
            };
            let plan = GcpHttpsIngressPlan {
                scope,
                resource_id: &worker.id,
                project: &config.project_id,
                region: &config.region,
                network: &network,
                subnetwork: &subnet,
                firewall: None,
                health_check: None,
                backend,
                frontend: crate::GcpIngressFrontend::Https(domains.certificate()),
            };
            let (state, outputs, fingerprint) = match scope {
                GcpHttpsIngressScope::RegionalPrivate => (
                    &mut self.private,
                    &mut self.private_endpoints,
                    &mut self.private_fingerprint,
                ),
                GcpHttpsIngressScope::GlobalPublic => (
                    &mut self.public,
                    &mut self.public_endpoints,
                    &mut self.public_fingerprint,
                ),
            };
            let state = state.get_or_insert_with(|| GcpHttpsIngressState::new(&plan));
            if !state.reconcile(client.as_ref(), plan).await? {
                return Ok(false);
            }
            let address = state
                .address
                .as_deref()
                .ok_or_else(|| invalid(&worker.id, "Confirmed worker ingress has no address"))?;
            let confirmed = domains.confirmed_endpoints(address);
            if *outputs != confirmed {
                *outputs = confirmed;
                // DNS metadata predates this target publication. Checkpoint it
                // before narrowing the service or retiring its retained path.
                return Ok(false);
            }
            *fingerprint = Some(domains.fingerprint());
        }
        // Publish confirmed addresses and wait for DNS before narrowing Cloud
        // Run ingress; otherwise a mode switch can close the retained path
        // before clients can resolve the replacement.
        for (scope, enabled) in [
            (
                GcpHttpsIngressScope::RegionalPrivate,
                access.private_enabled(),
            ),
            (
                GcpHttpsIngressScope::GlobalPublic,
                access.internet_enabled(),
            ),
        ] {
            if enabled
                && !worker.public_endpoints.is_empty()
                && !GcpHttpsDomains::dns_ready(ctx, &worker.id, &endpoints, scope)?
            {
                return Ok(false);
            }
        }
        if !retire {
            return Ok(true);
        }
        for (state, outputs, enabled) in [
            (
                &mut self.private,
                &mut self.private_endpoints,
                access.private_enabled(),
            ),
            (
                &mut self.public,
                &mut self.public_endpoints,
                access.internet_enabled(),
            ),
        ] {
            if enabled && !worker.public_endpoints.is_empty() {
                continue;
            }
            if let Some(ingress) = state {
                if !ingress.delete(client.as_ref(), &worker.id).await? {
                    return Ok(false);
                }
                *state = None;
            }
            outputs.clear();
        }
        if self.public.is_none() {
            self.recorded_public_protocol = None;
        }
        self.applied_access = Some(access);
        Ok(true)
    }

    pub async fn delete(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
    ) -> Result<bool> {
        let config = ctx.get_gcp_config()?;
        let client = ctx.service_provider.get_gcp_compute_client(config)?;
        for state in [&mut self.public, &mut self.private] {
            if let Some(ingress) = state {
                if !ingress.delete(client.as_ref(), resource_id).await.context(
                    ErrorData::CloudPlatformError {
                        resource_id: Some(resource_id.to_string()),
                        message: "Delete worker HTTPS ingress".to_string(),
                    },
                )? {
                    return Ok(false);
                }
                *state = None;
            }
        }
        self.public_endpoints.clear();
        self.private_endpoints.clear();
        Ok(true)
    }
}

fn invalid(resource_id: &str, message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        resource_id: Some(resource_id.to_string()),
        message: message.to_string(),
    })
}

fn worker_endpoints(worker: &Worker) -> Vec<PublicEndpoint> {
    worker
        .public_endpoints
        .iter()
        .map(|endpoint| PublicEndpoint {
            name: endpoint.name.clone(),
            port: 8080,
            protocol: ExposeProtocol::Http,
            host_label: endpoint.host_label.clone(),
            wildcard_subdomains: endpoint.wildcard_subdomains,
        })
        .collect()
}
