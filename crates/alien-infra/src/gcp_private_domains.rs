use alien_core::{
    CertificateStatus, ExposeProtocol, LoadBalancerEndpoint, ManagedDomainInfo, PublicEndpoint,
    PublicEndpointOutput,
};
use alien_error::AlienError;
use std::collections::HashMap;

use crate::{ErrorData, GcpIngressCertificate, ResourceControllerContext, Result};

/// Resolved runtime certificate material and endpoint hostnames. This value is
/// deliberately not serializable: only the confirmed endpoint outputs persist.
pub struct GcpPrivateHttpsDomains<'a> {
    endpoints: HashMap<String, PublicEndpointOutput>,
    managed_certificate: Option<&'a ManagedDomainInfo>,
    custom_certificates: Vec<String>,
}

impl<'a> GcpPrivateHttpsDomains<'a> {
    /// None means certificate issuance is still pending. Failed issuance and
    /// missing setup metadata are explicit errors, rather than public fallback.
    pub fn resolve(
        ctx: &'a ResourceControllerContext<'_>,
        resource_id: &str,
        endpoints: &[PublicEndpoint],
    ) -> Result<Option<Self>> {
        let invalid = |message: &str| {
            AlienError::new(ErrorData::ResourceConfigInvalid {
                message: message.to_string(),
                resource_id: Some(resource_id.to_string()),
            })
        };
        let custom = ctx
            .deployment_config
            .stack_settings
            .domains
            .as_ref()
            .and_then(|domains| domains.private_custom_domains.as_ref())
            .and_then(|domains| domains.get(resource_id));
        let metadata = ctx
            .deployment_config
            .domain_metadata
            .as_ref()
            .and_then(|domains| domains.resources.get(resource_id));
        let mut plan = Self {
            endpoints: HashMap::new(),
            managed_certificate: None,
            custom_certificates: Vec::new(),
        };
        if let Some(custom) = custom {
            let certificate = custom.certificate.gcp.as_ref().ok_or_else(|| {
                invalid("Private custom domain requires a regional GCP certificate reference")
            })?;
            let reference = &certificate.certificate_name;
            let region = &ctx.get_gcp_config()?.region;
            let classic = reference.contains(&format!("/regions/{region}/sslCertificates/"));
            let manager = reference.contains("certificatemanager.googleapis.com/")
                && reference.contains(&format!("/locations/{region}/certificates/"));
            if !classic && !manager {
                return Err(invalid("Private custom certificates must reference a regional Compute SSL certificate or Certificate Manager certificate in the deployment region"));
            }
            plan.custom_certificates.push(reference.clone());
        }
        for endpoint in endpoints {
            if endpoint.protocol != ExposeProtocol::Http {
                return Err(invalid("Regional HTTPS ingress requires an HTTP endpoint"));
            }
            let managed =
                metadata.and_then(|resource| resource.private_endpoints.get(&endpoint.name));
            let hostname = match custom {
                Some(custom) => custom.domain.clone(),
                None => {
                    let domain = managed
                        .ok_or_else(|| invalid("Private endpoint domain metadata is missing"))?;
                    if domain.certificate_status == CertificateStatus::Failed {
                        return Err(invalid("Private endpoint certificate issuance failed"));
                    }
                    if domain.certificate_chain.is_none() || domain.private_key.is_none() {
                        return Ok(None);
                    }
                    if let Some(previous) = plan.managed_certificate {
                        if previous.certificate_id != domain.certificate_id
                            || previous.issued_at != domain.issued_at
                            || previous.certificate_chain != domain.certificate_chain
                        {
                            return Err(invalid(
                                "Private endpoint certificate metadata is inconsistent",
                            ));
                        }
                    }
                    plan.managed_certificate = Some(domain);
                    domain.fqdn.clone()
                }
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
            return Err(invalid("Private ingress requires an endpoint"));
        }
        Ok(Some(plan))
    }

    pub fn certificate(&self) -> GcpIngressCertificate<'_> {
        if let Some(managed) = self.managed_certificate {
            GcpIngressCertificate::Managed {
                // resolve proved material exists before constructing this plan.
                certificate_pem: managed.certificate_chain.as_deref().unwrap(),
                private_key_pem: managed.private_key.as_deref().unwrap(),
            }
        } else {
            GcpIngressCertificate::Custom {
                references: &self.custom_certificates,
            }
        }
    }

    /// Capture only after the ingress reconciler verified the frontend.
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
