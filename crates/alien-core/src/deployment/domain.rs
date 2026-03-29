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

/// Certificate and DNS metadata for a public resource.
///
/// Includes decrypted certificate data for issued certificates.
/// Private keys are deployment-scoped secrets (like environment variables).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
