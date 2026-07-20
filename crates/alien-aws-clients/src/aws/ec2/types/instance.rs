use super::common::{Filter, TagSet};
use bon::Builder;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Availability Zone Request/Response Types
// ---------------------------------------------------------------------------

/// Request to describe availability zones.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeAvailabilityZonesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone_names: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_availability_zones: Option<bool>,
}

/// Response from describing availability zones.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeAvailabilityZonesResponse {
    #[serde(rename = "availabilityZoneInfo")]
    pub availability_zone_info: Option<AvailabilityZoneSet>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilityZoneSet {
    #[serde(rename = "item", default)]
    pub items: Vec<AvailabilityZone>,
}

/// Represents an availability zone.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailabilityZone {
    pub zone_name: Option<String>,
    pub zone_id: Option<String>,
    pub zone_state: Option<String>,
    pub region_name: Option<String>,
    pub zone_type: Option<String>,
    pub opt_in_status: Option<String>,
}

// ---------------------------------------------------------------------------
// AMI Request/Response Types
// ---------------------------------------------------------------------------

/// Request to describe images (AMIs).
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeImagesRequest {
    /// The image IDs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_ids: Option<Vec<String>>,
    /// The owners (self, amazon, aws-marketplace, or account ID).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owners: Option<Vec<String>>,
    /// Users that have explicit launch permissions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable_users: Option<Vec<String>>,
    /// Filters for the images.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    /// Whether to include deprecated AMIs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_deprecated: Option<bool>,
    /// Maximum results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    /// Token for pagination.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing images.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeImagesResponse {
    #[serde(rename = "imagesSet")]
    pub images_set: Option<ImageSet>,
    #[serde(rename = "nextToken")]
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageSet {
    #[serde(rename = "item", default)]
    pub items: Vec<Image>,
}

/// Represents an AMI.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub image_id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub image_location: Option<String>,
    pub state: Option<String>,
    pub owner_id: Option<String>,
    pub creation_date: Option<String>,
    pub architecture: Option<String>,
    pub platform: Option<String>,
    pub platform_details: Option<String>,
    pub image_type: Option<String>,
    pub root_device_type: Option<String>,
    pub root_device_name: Option<String>,
    pub virtualization_type: Option<String>,
    pub hypervisor: Option<String>,
    pub is_public: Option<bool>,
    pub deprecation_time: Option<String>,
    #[serde(rename = "tagSet")]
    pub tag_set: Option<TagSet>,
    #[serde(rename = "blockDeviceMapping")]
    pub block_device_mapping: Option<BlockDeviceMappingSet>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockDeviceMappingSet {
    #[serde(rename = "item", default)]
    pub items: Vec<BlockDeviceMapping>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockDeviceMapping {
    pub device_name: Option<String>,
    pub ebs: Option<EbsBlockDevice>,
    pub virtual_name: Option<String>,
    pub no_device: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EbsBlockDevice {
    pub snapshot_id: Option<String>,
    pub volume_size: Option<i32>,
    pub volume_type: Option<String>,
    pub delete_on_termination: Option<bool>,
    pub encrypted: Option<bool>,
    pub iops: Option<i32>,
    pub throughput: Option<i32>,
}

// ---------------------------------------------------------------------------
// Instance Request/Response Types
// ---------------------------------------------------------------------------

/// Response from terminating instances.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminateInstancesResponse {
    #[serde(rename = "instancesSet")]
    pub instances_set: Option<TerminatingInstanceSet>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminatingInstanceSet {
    #[serde(rename = "item", default)]
    pub items: Vec<TerminatingInstance>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminatingInstance {
    pub instance_id: Option<String>,
    pub current_state: Option<InstanceState>,
    pub previous_state: Option<InstanceState>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceState {
    pub code: Option<i32>,
    pub name: Option<String>,
}

/// Request to describe instances.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeInstancesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing instances.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeInstancesResponse {
    #[serde(rename = "reservationSet")]
    pub reservation_set: Option<ReservationSet>,
    #[serde(rename = "nextToken")]
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReservationSet {
    #[serde(rename = "item", default)]
    pub items: Vec<Reservation>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reservation {
    pub reservation_id: Option<String>,
    pub owner_id: Option<String>,
    #[serde(rename = "instancesSet")]
    pub instances_set: Option<InstanceSet>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceSet {
    #[serde(rename = "item", default)]
    pub items: Vec<Instance>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub instance_id: Option<String>,
    pub image_id: Option<String>,
    pub instance_type: Option<String>,
    pub key_name: Option<String>,
    pub launch_time: Option<String>,
    pub placement: Option<Placement>,
    pub private_dns_name: Option<String>,
    pub private_ip_address: Option<String>,
    pub public_dns_name: Option<String>,
    pub public_ip_address: Option<String>,
    pub state_reason: Option<StateReason>,
    pub instance_state: Option<InstanceState>,
    pub subnet_id: Option<String>,
    pub vpc_id: Option<String>,
    pub architecture: Option<String>,
    pub root_device_type: Option<String>,
    pub root_device_name: Option<String>,
    #[serde(rename = "tagSet")]
    pub tag_set: Option<TagSet>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Placement {
    pub availability_zone: Option<String>,
    pub tenancy: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateReason {
    pub code: Option<String>,
    pub message: Option<String>,
}
