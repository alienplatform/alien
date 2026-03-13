//! Service-type based storage binding definitions

use super::BindingValue;
use serde::{Deserialize, Serialize};

/// AWS S3 storage binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct S3StorageBinding {
    /// The name of the S3 bucket
    pub bucket_name: BindingValue<String>,
}

/// Azure Blob Storage binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct BlobStorageBinding {
    /// The name of the storage account
    pub account_name: BindingValue<String>,
    /// The name of the container
    pub container_name: BindingValue<String>,
}

/// Google Cloud Storage binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct GcsStorageBinding {
    /// The name of the GCS bucket
    pub bucket_name: BindingValue<String>,
}

/// Local filesystem storage binding configuration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct LocalStorageBinding {
    /// The storage directory path (file:// URL or absolute path)
    pub storage_path: BindingValue<String>,
}

/// Service-type based storage binding that supports multiple storage providers
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(tag = "service", rename_all = "lowercase")]
pub enum StorageBinding {
    /// AWS S3
    S3(S3StorageBinding),
    /// Azure Blob Storage
    Blob(BlobStorageBinding),
    /// Google Cloud Storage
    Gcs(GcsStorageBinding),
    /// Local filesystem storage
    #[serde(rename = "local-storage")]
    Local(LocalStorageBinding),
}

impl StorageBinding {
    /// Creates an S3 storage binding
    pub fn s3(bucket_name: impl Into<BindingValue<String>>) -> Self {
        Self::S3(S3StorageBinding {
            bucket_name: bucket_name.into(),
        })
    }

    /// Creates an Azure Blob storage binding
    pub fn blob(
        account_name: impl Into<BindingValue<String>>,
        container_name: impl Into<BindingValue<String>>,
    ) -> Self {
        Self::Blob(BlobStorageBinding {
            account_name: account_name.into(),
            container_name: container_name.into(),
        })
    }

    /// Creates a GCS storage binding
    pub fn gcs(bucket_name: impl Into<BindingValue<String>>) -> Self {
        Self::Gcs(GcsStorageBinding {
            bucket_name: bucket_name.into(),
        })
    }

    /// Creates a local storage binding
    pub fn local(storage_path: impl Into<BindingValue<String>>) -> Self {
        Self::Local(LocalStorageBinding {
            storage_path: storage_path.into(),
        })
    }
}
