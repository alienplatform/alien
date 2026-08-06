#[cfg(any(feature = "aws", feature = "azure", feature = "gcp", feature = "local"))]
use crate::error::{ErrorData, Result};
#[cfg(any(feature = "aws", feature = "azure", feature = "gcp", feature = "local"))]
use alien_error::{AlienError, Context, IntoAlienError};
#[cfg(any(feature = "aws", feature = "azure", feature = "gcp", feature = "local"))]
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
#[cfg(any(feature = "aws", feature = "azure", feature = "gcp", feature = "local"))]
use chrono::Utc;
#[cfg(any(feature = "aws", feature = "azure", feature = "gcp", feature = "local"))]
use serde::{Deserialize, Serialize};

#[cfg(any(feature = "aws", feature = "azure", feature = "gcp", feature = "local"))]
const VERSION_TOKEN_FORMAT: u8 = 1;

#[cfg(any(feature = "aws", feature = "azure", feature = "gcp", feature = "local"))]
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VersionToken {
    format: u8,
    key: String,
    backend_version: String,
    expires_at_millis: Option<i64>,
}

#[cfg(any(feature = "aws", feature = "azure", feature = "gcp", feature = "local"))]
pub(crate) struct DecodedVersion {
    pub backend_version: String,
    pub expired: bool,
}

#[cfg(any(feature = "aws", feature = "azure", feature = "gcp", feature = "local"))]
pub(crate) fn encode_version(
    key: &str,
    backend_version: String,
    expires_at_millis: Option<i64>,
) -> Result<String> {
    let bytes = serde_json::to_vec(&VersionToken {
        format: VERSION_TOKEN_FORMAT,
        key: key.to_string(),
        backend_version,
        expires_at_millis,
    })
    .into_alien_error()
    .context(ErrorData::InvalidInput {
        operation_context: "KV version encoding".to_string(),
        details: "Failed to serialize the version token".to_string(),
        field_name: Some("version".to_string()),
    })?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

#[cfg(any(feature = "aws", feature = "azure", feature = "gcp", feature = "local"))]
pub(crate) fn decode_version(key: &str, encoded: &str) -> Result<DecodedVersion> {
    let bytes =
        URL_SAFE_NO_PAD
            .decode(encoded)
            .into_alien_error()
            .context(ErrorData::InvalidInput {
                operation_context: "KV version decoding".to_string(),
                details: "Invalid version token encoding".to_string(),
                field_name: Some("ifVersion".to_string()),
            })?;
    let token: VersionToken =
        serde_json::from_slice(&bytes)
            .into_alien_error()
            .context(ErrorData::InvalidInput {
                operation_context: "KV version decoding".to_string(),
                details: "Invalid version token data".to_string(),
                field_name: Some("ifVersion".to_string()),
            })?;
    if token.format != VERSION_TOKEN_FORMAT || token.key != key {
        return Err(AlienError::new(ErrorData::InvalidInput {
            operation_context: "KV version validation".to_string(),
            details: "Version token does not belong to this key".to_string(),
            field_name: Some("ifVersion".to_string()),
        }));
    }

    Ok(DecodedVersion {
        backend_version: token.backend_version,
        expired: token
            .expires_at_millis
            .is_some_and(|expires_at| expires_at <= Utc::now().timestamp_millis()),
    })
}

/// Maximum value size in bytes for KV storage (24 KiB = 24,576 bytes)
///
/// This limit ensures compatibility across all KV backends, accounting for encoding overhead:
/// - **AWS DynamoDB**: 400KB item limit (much higher, not constraining)
/// - **GCP Firestore**: 1MiB document limit (much higher, not constraining)  
/// - **Azure Table Storage**: 64KB UTF-16 string limit, accounting for base64 + UTF-16 encoding
///
/// The 24KB limit accounts for Azure Table Storage's most restrictive constraint:
/// - 24KB raw data → ~32KB base64 → ~64KB UTF-16, fitting within Azure's 64KB limit
/// - Still supports reasonably sized data structures and JSON payloads
/// - Ensures fast network transfer and low latency
/// - Maintains consistent behavior across all cloud providers
///
/// Applications needing larger values should consider:
/// 1. Compressing data before storage (e.g., gzip JSON)
/// 2. Splitting data across multiple keys with a common prefix
/// 3. Using the Storage API for large objects (designed for multi-MB/GB files)
pub const MAX_VALUE_BYTES: usize = 24_576; // 24 KiB

/// Maximum key size in bytes (512 bytes)
///
/// This is a safe floor across all KV backends:
/// - **AWS DynamoDB**: Sort Key ≤ 1024 bytes  
/// - **GCP Firestore**: Document ID ≤ 1500 bytes
/// - **Azure Table Storage**: RowKey ≤ 1024 bytes
///
/// The 512-byte limit ensures:
/// - Universal compatibility across all backends
/// - Efficient indexing and query performance
/// - Reasonable prefix scanning capabilities
/// - Safe URL encoding when needed for REST APIs
pub const MAX_KEY_BYTES: usize = 512;

/// Global key validation for all KV providers
///
/// This ensures consistent behavior across all backends by using the most restrictive
/// character set that works universally:
///
/// **Allowed characters**: `a-z A-Z 0-9 - _ : .`
///
/// **Rationale for restrictions**:
/// - **Forward slash (`/`)**: Disallowed in Azure Table Storage PartitionKey/RowKey
/// - **Backslash (`\`)**: Disallowed in Azure Table Storage, problematic in URLs
/// - **Hash (`#`)**: Disallowed in Azure Table Storage, has special meaning in URLs
/// - **Question mark (`?`)**: Disallowed in Azure Table Storage, query parameter separator
/// - **Control characters**: Disallowed in Azure Table Storage, unsafe for transmission
/// - **Space and other special chars**: Can cause encoding issues across backends
///
/// **Platform-specific notes**:
/// - **AWS DynamoDB**: More permissive, but we follow the global restriction
/// - **GCP Firestore**: More permissive, but we follow the global restriction  
/// - **Azure Table Storage**: Most restrictive, sets the global standard
pub fn validate_key(key: &str) -> crate::error::Result<()> {
    use crate::error::ErrorData;
    use alien_error::AlienError;

    if key.is_empty() {
        return Err(AlienError::new(ErrorData::InvalidInput {
            operation_context: "KV key validation".to_string(),
            details: "Key cannot be empty".to_string(),
            field_name: Some("key".to_string()),
        }));
    }

    if key.len() > MAX_KEY_BYTES {
        return Err(AlienError::new(ErrorData::InvalidInput {
            operation_context: "KV key validation".to_string(),
            details: format!("Key exceeds {} bytes", MAX_KEY_BYTES),
            field_name: Some("key".to_string()),
        }));
    }

    // Global character set: most restrictive common denominator across all providers
    // Specifically excludes: / \ # ? and control characters (Azure Table Storage restrictions)
    if !key.chars().all(|c| {
        matches!(c, 'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | ':' | '.') && !c.is_control()
    }) {
        return Err(AlienError::new(ErrorData::InvalidInput {
            operation_context: "KV key validation".to_string(),
            details: "Key contains invalid characters. Allowed: a-z A-Z 0-9 - _ : . (no spaces, slashes, or special characters)".to_string(),
            field_name: Some("key".to_string()),
        }));
    }

    Ok(())
}

/// Global value validation for all KV providers
pub fn validate_value(value: &[u8]) -> crate::error::Result<()> {
    use crate::error::ErrorData;
    use alien_error::AlienError;

    if value.len() > MAX_VALUE_BYTES {
        return Err(AlienError::new(ErrorData::InvalidInput {
            operation_context: "KV value validation".to_string(),
            details: format!("Value exceeds {} bytes", MAX_VALUE_BYTES),
            field_name: Some("value".to_string()),
        }));
    }

    Ok(())
}

#[cfg(feature = "aws")]
pub mod aws_dynamodb;
#[cfg(feature = "azure")]
pub mod azure_table_storage;
#[cfg(feature = "gcp")]
pub mod gcp_firestore;
#[cfg(feature = "local")]
pub mod local;

#[cfg(feature = "aws")]
pub use aws_dynamodb::AwsDynamodbKv;
#[cfg(feature = "azure")]
pub use azure_table_storage::AzureTableStorageKv;
#[cfg(feature = "gcp")]
pub use gcp_firestore::GcpFirestoreKv;
#[cfg(feature = "local")]
pub use local::LocalKv;
