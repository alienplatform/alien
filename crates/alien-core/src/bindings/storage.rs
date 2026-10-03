//! Service-type based storage binding definitions

use super::BindingValue;
use serde::{Deserialize, Serialize};

/// S3 storage binding configuration.
///
/// Targets AWS S3 by default. Setting `endpoint` targets any S3-compatible
/// store instead (MinIO, Ceph RGW, NetApp StorageGRID, Cloudflare R2, ...),
/// which is how Kubernetes deployments outside a cloud account attach the
/// customer's existing object storage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct S3StorageBinding {
    /// The name of the S3 bucket
    pub bucket_name: BindingValue<String>,
    /// Endpoint of an S3-compatible store, e.g. `https://minio.internal:9000`.
    /// Unset means AWS S3.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<BindingValue<String>>,
    /// Region to sign requests for. Defaults to the ambient AWS region, or
    /// `us-east-1` for S3-compatible endpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<BindingValue<String>>,
    /// Use path-style URLs (`endpoint/bucket/key`) instead of virtual-hosted
    /// ones (`bucket.endpoint/key`). Defaults to `true` when `endpoint` is
    /// set, since most S3-compatible stores require it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub force_path_style: Option<bool>,
    /// Static access key ID. Unset means the ambient AWS credential chain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_key_id: Option<BindingValue<String>>,
    /// Static secret access key; required when `accessKeyId` is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_access_key: Option<BindingValue<String>>,
}

impl S3StorageBinding {
    /// AWS S3 bucket using the ambient credential chain.
    pub fn bucket(bucket_name: impl Into<BindingValue<String>>) -> Self {
        Self {
            bucket_name: bucket_name.into(),
            endpoint: None,
            region: None,
            force_path_style: None,
            access_key_id: None,
            secret_access_key: None,
        }
    }
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
        Self::S3(S3StorageBinding::bucket(bucket_name))
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
