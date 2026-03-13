//! GCP Certificate Manager client for managing SSL/TLS certificates.
//!
//! This module provides APIs for:
//! - Creating and managing SSL certificates
//! - Certificate maps for mapping certificates to domains
//!
//! See: https://cloud.google.com/certificate-manager/docs/reference/rest

use crate::gcp::api_client::{GcpClientBase, GcpServiceConfig};
use crate::gcp::GcpClientConfig;
use alien_client_core::Result;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

#[cfg(feature = "test-utils")]
use mockall::automock;

// =============================================================================================
// Service Configuration
// =============================================================================================

/// Certificate Manager service configuration
#[derive(Debug)]
pub struct CertificateManagerServiceConfig;

impl GcpServiceConfig for CertificateManagerServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://certificatemanager.googleapis.com/v1"
    }

    fn default_audience(&self) -> &'static str {
        "https://certificatemanager.googleapis.com/"
    }

    fn service_name(&self) -> &'static str {
        "Certificate Manager"
    }

    fn service_key(&self) -> &'static str {
        "certificatemanager"
    }
}

// =============================================================================================
// API Trait
// =============================================================================================

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait CertificateManagerApi: Send + Sync + Debug {
    /// Creates a Certificate resource.
    /// See: https://cloud.google.com/certificate-manager/docs/reference/rest/v1/projects.locations.certificates/create
    async fn create_certificate(
        &self,
        parent: String,
        certificate_id: String,
        certificate: Certificate,
    ) -> Result<Operation>;

    /// Gets a Certificate resource.
    /// See: https://cloud.google.com/certificate-manager/docs/reference/rest/v1/projects.locations.certificates/get
    async fn get_certificate(&self, name: String) -> Result<Certificate>;

    /// Deletes a Certificate resource.
    /// See: https://cloud.google.com/certificate-manager/docs/reference/rest/v1/projects.locations.certificates/delete
    async fn delete_certificate(&self, name: String) -> Result<Operation>;

    /// Gets the status of a long-running operation.
    /// See: https://cloud.google.com/certificate-manager/docs/reference/rest/v1/projects.locations.operations/get
    async fn get_operation(&self, name: String) -> Result<Operation>;
}

// =============================================================================================
// Data Structures
// =============================================================================================

/// Represents a Certificate resource.
/// See: https://cloud.google.com/certificate-manager/docs/reference/rest/v1/projects.locations.certificates#Certificate
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Certificate {
    /// Resource name (output only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Human-readable description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The certificate type (self-managed or managed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,

    /// Self-managed certificate configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_managed: Option<SelfManagedCertificate>,

    /// Resource labels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<std::collections::HashMap<String, String>>,

    /// Creation timestamp (output only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time: Option<String>,

    /// Update timestamp (output only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_time: Option<String>,

    /// Expiration timestamp (output only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expire_time: Option<String>,

    /// Subject Alternative Names (output only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub san_dnsnames: Option<Vec<String>>,

    /// Certificate PEM format (output only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pem_certificate: Option<String>,
}

/// Self-managed certificate configuration.
/// See: https://cloud.google.com/certificate-manager/docs/reference/rest/v1/projects.locations.certificates#SelfManagedCertificate
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SelfManagedCertificate {
    /// PEM-encoded certificate chain (required for creation).
    /// Should include the leaf certificate and any intermediate certificates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pem_certificate: Option<String>,

    /// PEM-encoded private key (required for creation, never returned).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pem_private_key: Option<String>,
}

/// Represents a long-running operation.
/// See: https://cloud.google.com/certificate-manager/docs/reference/rest/v1/projects.locations.operations#Operation
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    /// Resource name.
    pub name: String,

    /// Operation metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,

    /// Whether the operation is done.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done: Option<bool>,

    /// Error information if operation failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<OperationError>,

    /// Response if operation succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<serde_json::Value>,
}

/// Error information for a failed operation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OperationError {
    /// Error code.
    pub code: i32,

    /// Error message.
    pub message: String,

    /// Additional error details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Vec<serde_json::Value>>,
}

// =============================================================================================
// Client Implementation
// =============================================================================================

/// Certificate Manager client implementation.
#[derive(Debug)]
pub struct CertificateManagerClient {
    base: GcpClientBase,
}

impl CertificateManagerClient {
    /// Creates a new Certificate Manager client.
    pub fn new(config: GcpClientConfig, http_client: Client) -> Self {
        Self {
            base: GcpClientBase::new(
                http_client,
                config,
                Box::new(CertificateManagerServiceConfig),
            ),
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl CertificateManagerApi for CertificateManagerClient {
    async fn create_certificate(
        &self,
        parent: String,
        certificate_id: String,
        certificate: Certificate,
    ) -> Result<Operation> {
        let url = format!("{}/certificates?certificateId={}", parent, certificate_id);
        let resource_name = certificate
            .name
            .clone()
            .unwrap_or_else(|| certificate_id.clone());
        self.base
            .execute_request(Method::POST, &url, None, Some(certificate), &resource_name)
            .await
    }

    async fn get_certificate(&self, name: String) -> Result<Certificate> {
        self.base
            .execute_request::<Certificate, ()>(Method::GET, &name, None, None, &name)
            .await
    }

    async fn delete_certificate(&self, name: String) -> Result<Operation> {
        self.base
            .execute_request::<Operation, ()>(Method::DELETE, &name, None, None, &name)
            .await
    }

    async fn get_operation(&self, name: String) -> Result<Operation> {
        self.base
            .execute_request::<Operation, ()>(Method::GET, &name, None, None, &name)
            .await
    }
}
