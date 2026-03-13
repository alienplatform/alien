//! ServiceAccount binding definitions for identity management and impersonation
//!
//! This module defines the binding parameters for service account access:
//! - AWS IAM Roles (using role ARN for AssumeRole)
//! - GCP Service Accounts (using service account email for token generation)
//! - Azure User-Assigned Managed Identities (using client ID and resource ID)

use super::BindingValue;
use serde::{Deserialize, Serialize};

/// Represents a service account binding for identity management and impersonation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "service", rename_all = "lowercase")]
pub enum ServiceAccountBinding {
    /// AWS IAM Role binding
    AwsIam(AwsServiceAccountBinding),
    /// GCP Service Account binding
    GcpServiceAccount(GcpServiceAccountBinding),
    /// Azure User-Assigned Managed Identity binding
    AzureManagedIdentity(AzureServiceAccountBinding),
}

/// AWS IAM Role service account binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsServiceAccountBinding {
    /// The IAM role name
    pub role_name: BindingValue<String>,
    /// The IAM role ARN (for AssumeRole)
    pub role_arn: BindingValue<String>,
}

/// GCP Service Account binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GcpServiceAccountBinding {
    /// The service account email (for impersonation)
    pub email: BindingValue<String>,
    /// The service account unique ID
    pub unique_id: BindingValue<String>,
}

/// Azure User-Assigned Managed Identity binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AzureServiceAccountBinding {
    /// The managed identity client ID (for authentication)
    pub client_id: BindingValue<String>,
    /// The managed identity resource ID (ARM ID)
    pub resource_id: BindingValue<String>,
    /// The managed identity principal ID
    pub principal_id: BindingValue<String>,
}

impl ServiceAccountBinding {
    /// Creates an AWS IAM Role service account binding
    pub fn aws_iam(
        role_name: impl Into<BindingValue<String>>,
        role_arn: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::AwsIam(AwsServiceAccountBinding {
            role_name: role_name.into(),
            role_arn: role_arn.into(),
        })
    }

    /// Creates a GCP Service Account binding
    pub fn gcp_service_account(
        email: impl Into<BindingValue<String>>,
        unique_id: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::GcpServiceAccount(GcpServiceAccountBinding {
            email: email.into(),
            unique_id: unique_id.into(),
        })
    }

    /// Creates an Azure User-Assigned Managed Identity binding
    pub fn azure_managed_identity(
        client_id: impl Into<BindingValue<String>>,
        resource_id: impl Into<BindingValue<String>>,
        principal_id: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::AzureManagedIdentity(AzureServiceAccountBinding {
            client_id: client_id.into(),
            resource_id: resource_id.into(),
            principal_id: principal_id.into(),
        })
    }
}
