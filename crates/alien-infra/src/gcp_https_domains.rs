use alien_core::{
    CertificateStatus, DnsRecordStatus, ExposeProtocol, LoadBalancerEndpoint, PublicEndpoint,
    PublicEndpointOutput,
};
use alien_error::AlienError;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

use crate::{
    ErrorData, GcpHttpsIngressScope, GcpIngressCertificate, ResourceControllerContext, Result,
};

/// Runtime certificate material is borrowed from deployment configuration and
/// never serialized into controller state.
pub struct GcpHttpsDomains<'a> {
    endpoints: HashMap<String, PublicEndpointOutput>,
    certificate_chain: Option<&'a str>,
    private_key: Option<&'a str>,
    certificate_id: Option<&'a str>,
    issued_at: Option<&'a str>,
    custom_certificates: Vec<String>,
}

impl<'a> GcpHttpsDomains<'a> {
    /// None means certificate issuance is pending. Missing metadata, a failed
    /// issuance, and the wrong certificate region are explicit errors.
    pub fn resolve(
        ctx: &'a ResourceControllerContext<'_>,
        resource_id: &str,
        endpoints: &[PublicEndpoint],
        scope: GcpHttpsIngressScope,
    ) -> Result<Option<Self>> {
        let invalid = |message: &str| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: message.to_string(),
                resource_id: Some(resource_id.to_string()),
            })
        };
        let private = scope == GcpHttpsIngressScope::RegionalPrivate;
        let custom = ctx
            .deployment_config
            .stack_settings
            .domains
            .as_ref()
            .and_then(|domains| {
                if private {
                    domains.private_custom_domains.as_ref()
                } else {
                    domains.custom_domains.as_ref()
                }
            })
            .and_then(|domains| domains.get(resource_id));
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|domains| domains.resources.get(resource_id));
        let mut plan = Self {
            endpoints: HashMap::new(),
            certificate_chain: None,
            private_key: None,
            certificate_id: None,
            issued_at: None,
            custom_certificates: Vec::new(),
        };
        if let Some(custom) = custom {
            let certificate = custom
                .certificate
                .gcp
                .as_ref()
                .ok_or_else(|| invalid("Custom domain requires a GCP certificate reference"))?;
            let config = ctx.get_gcp_config()?;
            let reference = &certificate.certificate_name;
            let classic_collection = if private {
                format!("regions/{}", config.region)
            } else {
                "global".to_string()
            };
            let reference = if !reference.contains('/') {
                format!("https://www.googleapis.com/compute/v1/projects/{}/{classic_collection}/sslCertificates/{reference}", config.project_id)
            } else {
                reference.clone()
            };
            let classic = reference.contains(&format!("/{classic_collection}/sslCertificates/"));
            let manager = private
                && reference.contains("certificatemanager.googleapis.com/")
                && reference.contains(&format!("/locations/{}/certificates/", config.region));
            if !classic && !manager {
                return Err(invalid("Private custom certificates require a regional Compute SSL or Certificate Manager reference in the deployment region; public custom certificates require a global Compute SSL certificate"));
            }
            plan.custom_certificates.push(reference);
        }
        for endpoint in endpoints {
            if endpoint.protocol != ExposeProtocol::Http {
                return Err(invalid("HTTPS ingress requires an HTTP endpoint"));
            }
            let managed = metadata.and_then(|resource| {
                if private {
                    resource.private_endpoints.get(&endpoint.name)
                } else {
                    resource.endpoints.get(&endpoint.name)
                }
            });
            let hostname = if let Some(custom) = custom {
                custom.domain.clone()
            } else {
                let (hostname, id, status, chain, key, issued_at) = if let Some(domain) = managed {
                    (
                        &domain.fqdn,
                        &domain.certificate_id,
                        domain.certificate_status,
                        domain.certificate_chain.as_deref(),
                        domain.private_key.as_deref(),
                        domain.issued_at.as_deref(),
                    )
                } else if !private {
                    // Older managers sent the primary public certificate only.
                    let resource = metadata
                        .ok_or_else(|| invalid("Public endpoint domain metadata is missing"))?;
                    (
                        &resource.fqdn,
                        &resource.certificate_id,
                        resource.certificate_status,
                        resource.certificate_chain.as_deref(),
                        resource.private_key.as_deref(),
                        resource.issued_at.as_deref(),
                    )
                } else {
                    return Err(invalid("Private endpoint domain metadata is missing"));
                };
                if matches!(
                    status,
                    CertificateStatus::Failed | CertificateStatus::Deleting
                ) {
                    return Err(invalid(
                        "Endpoint certificate issuance failed or is being deleted",
                    ));
                }
                let (Some(chain), Some(key)) = (chain, key) else {
                    return Ok(None);
                };
                if plan.certificate_id.is_some()
                    && (plan.certificate_id != Some(id.as_str())
                        || plan.issued_at != issued_at
                        || plan.certificate_chain != Some(chain))
                {
                    return Err(invalid("Endpoint certificate metadata is inconsistent"));
                }
                plan.certificate_id = Some(id.as_str());
                plan.issued_at = issued_at;
                plan.certificate_chain = Some(chain);
                plan.private_key = Some(key);
                hostname.clone()
            };
            plan.endpoints.insert(
                endpoint.name.clone(),
                PublicEndpointOutput {
                    url: format!("https://{hostname}"),
                    wildcard_host: endpoint
                        .wildcard_subdomains
                        .then(|| format!("*.{hostname}")),
                    host: hostname,
                    protocol: ExposeProtocol::Http,
                    port: 443,
                    load_balancer_endpoint: None,
                },
            );
        }
        if plan.endpoints.is_empty() {
            return Err(invalid("HTTPS ingress requires an endpoint"));
        }
        Ok(Some(plan))
    }

    /// Frontend addresses must be published before managed DNS can converge.
    /// Call this after recording confirmed outputs, before retiring old access.
    pub fn dns_ready(
        ctx: &ResourceControllerContext<'_>,
        resource_id: &str,
        endpoints: &[PublicEndpoint],
        scope: GcpHttpsIngressScope,
    ) -> Result<bool> {
        let private = scope == GcpHttpsIngressScope::RegionalPrivate;
        let custom = ctx
            .deployment_config
            .stack_settings
            .domains
            .as_ref()
            .and_then(|domains| {
                if private {
                    domains.private_custom_domains.as_ref()
                } else {
                    domains.custom_domains.as_ref()
                }
            })
            .is_some_and(|domains| domains.contains_key(resource_id));
        if custom {
            return Ok(true);
        }
        let invalid = |message: String| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message,
                resource_id: Some(resource_id.to_string()),
            })
        };
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|metadata| metadata.resources.get(resource_id))
            .ok_or_else(|| invalid("Endpoint DNS metadata is missing".to_string()))?;
        for endpoint in endpoints {
            let domain = if private {
                metadata.private_endpoints.get(&endpoint.name)
            } else {
                metadata.endpoints.get(&endpoint.name)
            };
            let (status, error) = if let Some(domain) = domain {
                (domain.dns_status, domain.dns_error.as_deref())
            } else if !private {
                (metadata.dns_status, metadata.dns_error.as_deref())
            } else {
                return Err(invalid(
                    "Private endpoint DNS metadata is missing".to_string(),
                ));
            };
            match status {
                DnsRecordStatus::Active => (),
                DnsRecordStatus::Failed => {
                    return Err(invalid(format!(
                        "Endpoint DNS failed: {}",
                        error.unwrap_or("no details available")
                    )))
                }
                _ => return Ok(false),
            }
        }
        Ok(true)
    }

    pub fn certificate(&self) -> GcpIngressCertificate<'_> {
        if let (Some(certificate_pem), Some(private_key_pem)) =
            (self.certificate_chain, self.private_key)
        {
            GcpIngressCertificate::Managed {
                certificate_pem,
                private_key_pem,
            }
        } else {
            GcpIngressCertificate::Custom {
                references: &self.custom_certificates,
            }
        }
    }

    pub fn fingerprint(&self) -> String {
        let mut digest = Sha256::new();
        let mut hosts: Vec<_> = self
            .endpoints
            .values()
            .map(|endpoint| endpoint.host.as_str())
            .collect();
        hosts.sort_unstable();
        for host in hosts {
            digest.update(host.as_bytes());
            digest.update([0]);
        }
        if let Some(chain) = self.certificate_chain {
            digest.update(chain.as_bytes());
        }
        for reference in &self.custom_certificates {
            digest.update(reference.as_bytes());
            digest.update([0]);
        }
        format!("{:x}", digest.finalize())
    }

    /// Capture only after the reconciler verified the requested frontend.
    pub fn confirmed_endpoints(&self, ip: &str) -> HashMap<String, PublicEndpointOutput> {
        self.endpoints
            .iter()
            .map(|(name, endpoint)| {
                let mut endpoint = endpoint.clone();
                endpoint.load_balancer_endpoint = Some(LoadBalancerEndpoint {
                    dns_name: ip.to_string(),
                    hosted_zone_id: None,
                });
                (name.clone(), endpoint)
            })
            .collect()
    }
}
