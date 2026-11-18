//! Domain, certificate, and DNS metadata for auto-managed public resources.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Certificate status in the certificate lifecycle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
pub enum CertificateStatus {
    Pending,
    Issued,
    Renewing,
    RenewalFailed,
    Failed,
    Deleting,
}

/// DNS record status in the DNS lifecycle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum DnsRecordStatus {
    Pending,
    Active,
    Updating,
    Deleting,
    Failed,
}

/// Certificate and DNS metadata for a managed hostname.
///
/// Includes decrypted certificate data for issued certificates.
/// Private keys are deployment-scoped secrets (like environment variables).
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ManagedDomainInfo {
    /// Fully qualified domain name.
    pub fqdn: String,
    /// Certificate ID (for tracking/logging).
    pub certificate_id: String,
    /// Current certificate status
    pub certificate_status: CertificateStatus,
    /// Current DNS record status
    pub dns_status: DnsRecordStatus,
    /// Last DNS error message. Present when DNS previously failed, even if status
    /// was reset to pending for retry. Used to surface actionable error context
    /// in WaitingForDns failure messages.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns_error: Option<String>,
    /// Full PEM certificate chain (only present if status is "issued").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate_chain: Option<String>,
    /// Decrypted private key (only present if status is "issued").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    /// ISO 8601 timestamp when certificate was issued (for renewal detection).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issued_at: Option<String>,
}

/// Certificate and DNS metadata for a public endpoint.
pub type PublicEndpointDomainInfo = ManagedDomainInfo;

/// Certificate and DNS metadata for a public resource.
///
/// The direct fields describe the primary endpoint hostname. `endpoints`
/// contains endpoint-scoped metadata keyed by endpoint name. `aliases` contains
/// additional managed hostnames that route directly to the primary endpoint.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ResourceDomainInfo {
    /// Fully qualified domain name.
    pub fqdn: String,
    /// Certificate ID (for tracking/logging).
    pub certificate_id: String,
    /// Current certificate status
    pub certificate_status: CertificateStatus,
    /// Current DNS record status
    pub dns_status: DnsRecordStatus,
    /// Last DNS error message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns_error: Option<String>,
    /// Full PEM certificate chain (only present if status is "issued").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate_chain: Option<String>,
    /// Decrypted private key (only present if status is "issued").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    /// ISO 8601 timestamp when certificate was issued (for renewal detection).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issued_at: Option<String>,
    /// Endpoint-scoped metadata keyed by endpoint name.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub endpoints: HashMap<String, PublicEndpointDomainInfo>,
    /// Additional managed hostnames for the resource.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<ManagedDomainInfo>,
}

/// Domain metadata for auto-managed public resources (no private keys).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct DomainMetadata {
    /// Base domain for auto-generated domains (e.g., "vpc.direct").
    pub base_domain: String,
    /// Deployment public subdomain (e.g., "k8f2j3").
    pub public_subdomain: String,
    /// Hosted zone ID for DNS records.
    pub hosted_zone_id: String,
    /// Metadata per resource ID.
    pub resources: HashMap<String, ResourceDomainInfo>,
}

/// `Some("<redacted>")` or `None`: the private key never reaches a log.
fn redacted(key: &Option<String>) -> Option<&'static str> {
    key.as_ref().map(|_| "<redacted>")
}

impl std::fmt::Debug for ManagedDomainInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManagedDomainInfo")
            .field("fqdn", &self.fqdn)
            .field("certificate_id", &self.certificate_id)
            .field("certificate_status", &self.certificate_status)
            .field("dns_status", &self.dns_status)
            .field("dns_error", &self.dns_error)
            .field("certificate_chain", &self.certificate_chain)
            .field("private_key", &redacted(&self.private_key))
            .field("issued_at", &self.issued_at)
            .finish()
    }
}

impl std::fmt::Debug for ResourceDomainInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResourceDomainInfo")
            .field("fqdn", &self.fqdn)
            .field("certificate_id", &self.certificate_id)
            .field("certificate_status", &self.certificate_status)
            .field("dns_status", &self.dns_status)
            .field("dns_error", &self.dns_error)
            .field("certificate_chain", &self.certificate_chain)
            .field("private_key", &redacted(&self.private_key))
            .field("issued_at", &self.issued_at)
            .field("endpoints", &self.endpoints)
            .field("aliases", &self.aliases)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn managed(key: Option<&str>) -> ManagedDomainInfo {
        ManagedDomainInfo {
            fqdn: "api.example.test".into(),
            certificate_id: "cert_1".into(),
            certificate_status: CertificateStatus::Issued,
            dns_status: DnsRecordStatus::Active,
            dns_error: None,
            certificate_chain: Some("CHAIN".into()),
            private_key: key.map(Into::into),
            issued_at: None,
        }
    }

    #[test]
    fn debug_output_has_no_private_key() {
        let alias = managed(Some("SECRET-KEY-MATERIAL"));
        let text = format!("{alias:?}");
        assert!(!text.contains("SECRET-KEY-MATERIAL"), "{text}");
        assert!(text.contains("private_key: Some(\"<redacted>\")"), "{text}");
        assert!(format!("{:?}", managed(None)).contains("private_key: None"));

        let resource = ResourceDomainInfo {
            fqdn: "api.example.test".into(),
            certificate_id: "cert_1".into(),
            certificate_status: CertificateStatus::Issued,
            dns_status: DnsRecordStatus::Active,
            dns_error: None,
            certificate_chain: Some("CHAIN".into()),
            private_key: Some("SECRET-KEY-MATERIAL".into()),
            issued_at: None,
            endpoints: HashMap::from([("web".to_string(), managed(Some("SECRET-KEY-MATERIAL")))]),
            aliases: vec![alias],
        };
        let text = format!("{resource:?}");
        assert!(!text.contains("SECRET-KEY-MATERIAL"), "{text}");
        assert!(text.contains("api.example.test"));
    }
}
