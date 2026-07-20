use super::common::{Filter, TagSet, TagSpecification};
use bon::Builder;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Volume Request/Response Types
// ---------------------------------------------------------------------------

/// Request to create a volume.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateVolumeRequest {
    /// The availability zone.
    pub availability_zone: String,
    /// The size of the volume in GiBs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<i32>,
    /// The snapshot ID to create the volume from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    /// The volume type: gp2, gp3, io1, io2, st1, sc1, standard.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_type: Option<String>,
    /// IOPS for io1, io2, or gp3.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iops: Option<i32>,
    /// Throughput for gp3 in MiB/s.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throughput: Option<i32>,
    /// Whether to encrypt the volume.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted: Option<bool>,
    /// The KMS key ID for encryption.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kms_key_id: Option<String>,
    /// Tags for the volume.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Response from creating a volume.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateVolumeResponse {
    pub volume_id: Option<String>,
    pub size: Option<i32>,
    pub availability_zone: Option<String>,
    pub state: Option<String>,
    pub volume_type: Option<String>,
    pub iops: Option<i32>,
    pub throughput: Option<i32>,
    pub encrypted: Option<bool>,
    pub create_time: Option<String>,
    #[serde(rename = "tagSet")]
    pub tag_set: Option<TagSet>,
}

/// Request to describe volumes.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeVolumesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing volumes.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeVolumesResponse {
    #[serde(rename = "volumeSet")]
    pub volume_set: Option<VolumeSet>,
    #[serde(rename = "nextToken")]
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeSet {
    #[serde(rename = "item", default)]
    pub items: Vec<Volume>,
}

/// Represents an EBS volume.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Volume {
    pub volume_id: Option<String>,
    pub size: Option<i32>,
    pub availability_zone: Option<String>,
    pub state: Option<String>,
    pub volume_type: Option<String>,
    pub iops: Option<i32>,
    pub throughput: Option<i32>,
    pub encrypted: Option<bool>,
    pub snapshot_id: Option<String>,
    pub create_time: Option<String>,
    #[serde(rename = "attachmentSet")]
    pub attachment_set: Option<VolumeAttachmentSet>,
    #[serde(rename = "tagSet")]
    pub tag_set: Option<TagSet>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeAttachmentSet {
    #[serde(rename = "item", default)]
    pub items: Vec<VolumeAttachment>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeAttachment {
    pub volume_id: Option<String>,
    pub instance_id: Option<String>,
    pub device: Option<String>,
    pub state: Option<String>,
    pub attach_time: Option<String>,
    pub delete_on_termination: Option<bool>,
}

/// Request to attach a volume.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct AttachVolumeRequest {
    /// The volume ID.
    pub volume_id: String,
    /// The instance ID.
    pub instance_id: String,
    /// The device name (e.g., /dev/sdf).
    pub device: String,
}

/// Response from attaching a volume.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachVolumeResponse {
    pub volume_id: Option<String>,
    pub instance_id: Option<String>,
    pub device: Option<String>,
    pub state: Option<String>,
    pub attach_time: Option<String>,
}

/// Request to detach a volume.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct DetachVolumeRequest {
    /// The volume ID.
    pub volume_id: String,
    /// The instance ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    /// The device name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
    /// Whether to force detach.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force: Option<bool>,
}

/// Response from detaching a volume.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetachVolumeResponse {
    pub volume_id: Option<String>,
    pub instance_id: Option<String>,
    pub device: Option<String>,
    pub state: Option<String>,
    pub attach_time: Option<String>,
}
