use bon::Builder;
use serde::{Deserialize, Serialize};

// =============================================================================================
// Data Structures - Operation
// =============================================================================================

/// Represents a Compute Engine operation.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/globalOperations
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Type of the operation (e.g., "insert", "delete").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation_type: Option<String>,

    /// URL of the resource the operation modifies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_link: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// User who requested the operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,

    /// Status of the operation: PENDING, RUNNING, or DONE.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<OperationStatus>,

    /// Optional progress indicator (0-100).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<i32>,

    /// Time the operation was started (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,

    /// Time the operation was completed (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,

    /// Time the operation was requested (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_time: Option<String>,

    /// URL of the zone where the operation resides (for zonal operations).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,

    /// URL of the region where the operation resides (for regional operations).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,

    /// Description of the operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// HTTP error status code returned if the operation failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_error_status_code: Option<i32>,

    /// HTTP error message returned if the operation failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_error_message: Option<String>,

    /// Error information if the operation failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<OperationError>,

    /// Type of resource (always "compute#operation").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

impl Operation {
    /// Returns true if the operation has completed (status == DONE).
    pub fn is_done(&self) -> bool {
        matches!(self.status, Some(OperationStatus::Done))
    }

    /// Returns true if the operation completed with an error.
    pub fn has_error(&self) -> bool {
        self.error.is_some() && !self.error.as_ref().unwrap().errors.is_empty()
    }
}

/// Status of an operation.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OperationStatus {
    /// Operation is pending.
    Pending,
    /// Operation is running.
    Running,
    /// Operation is complete.
    Done,
}

/// Error information for a failed operation.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct OperationError {
    /// Array of errors.
    #[builder(default)]
    #[serde(default)]
    pub errors: Vec<OperationErrorItem>,
}

/// Individual error item in an operation error.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct OperationErrorItem {
    /// Error code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    /// Location in the request that caused the error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,

    /// Human-readable error message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// =============================================================================================
// Data Structures - Zone
// =============================================================================================

/// Represents a Compute Engine zone resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/zones
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Zone {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the zone.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// Region URL this zone belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,

    /// Zone status, commonly "UP" for usable zones.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,

    /// Type of resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// List of Compute Engine zones.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ZoneList {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// List of zones.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Zone>,

    /// Server-defined URL for this resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// Token for next page of results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,

    /// Type of resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}
