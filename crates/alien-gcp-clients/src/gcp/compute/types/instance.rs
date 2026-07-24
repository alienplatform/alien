use super::network::{NetworkTier, StackType};
use bon::Builder;
use serde::{Deserialize, Serialize};

mod runtime;
pub use runtime::*;

// =============================================================================================
// Data Structures - Instance Template
// =============================================================================================

/// Represents an instance template resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/instanceTemplates
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceTemplate {
    /// Unique identifier; defined by the server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Name of the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Server-defined URL for the resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub self_link: Option<String>,

    /// Instance properties for instances created from this template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<InstanceProperties>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#instanceTemplate").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Properties for instances created from a template.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceProperties {
    /// Machine type for instances.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_type: Option<String>,

    /// Description of instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Disks attached to instances.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disks: Vec<AttachedDisk>,

    /// Network interfaces for instances.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network_interfaces: Vec<NetworkInterface>,

    /// Metadata for instances.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,

    /// Service accounts for instances.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub service_accounts: Vec<ServiceAccount>,

    /// Tags for instances.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Tags>,

    /// Scheduling configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduling: Option<Scheduling>,

    /// Labels for instances.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub labels: std::collections::HashMap<String, String>,

    /// Whether to allow stopping for update.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_ip_forward: Option<bool>,

    /// Guest accelerators for instances.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guest_accelerators: Vec<AcceleratorConfig>,

    /// Shielded instance configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shielded_instance_config: Option<ShieldedInstanceConfig>,

    /// Confidential instance configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidential_instance_config: Option<ConfidentialInstanceConfig>,
}

/// Attached disk configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AttachedDisk {
    /// Type of attachment (PERSISTENT, SCRATCH).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<AttachedDiskType>,

    /// Mode of disk (READ_WRITE, READ_ONLY).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<DiskMode>,

    /// Source disk URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,

    /// Device name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,

    /// Boot disk indicator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boot: Option<bool>,

    /// Initialize parameters for new disks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initialize_params: Option<AttachedDiskInitializeParams>,

    /// Whether to auto-delete the disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_delete: Option<bool>,

    /// Index of the disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<i32>,

    /// Disk interface (SCSI, NVME).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface: Option<DiskInterface>,
}

/// Attached disk type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AttachedDiskType {
    /// Persistent disk.
    Persistent,
    /// Scratch disk.
    Scratch,
}

/// Disk mode.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DiskMode {
    /// Read-write mode.
    ReadWrite,
    /// Read-only mode.
    ReadOnly,
}

/// Disk interface.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DiskInterface {
    /// SCSI interface.
    Scsi,
    /// NVMe interface.
    Nvme,
}

/// Parameters for initializing a new disk.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AttachedDiskInitializeParams {
    /// Name for the disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk_name: Option<String>,

    /// Source image URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_image: Option<String>,

    /// Disk size in GB.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk_size_gb: Option<String>,

    /// Disk type URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk_type: Option<String>,

    /// Source snapshot URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_snapshot: Option<String>,

    /// Labels for the disk.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub labels: std::collections::HashMap<String, String>,
}

/// Network interface configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInterface {
    /// Network URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<String>,

    /// Subnetwork URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnetwork: Option<String>,

    /// Network IP address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_i_p: Option<String>,

    /// Name of the interface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Access configurations for external IPs.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub access_configs: Vec<AccessConfig>,

    /// Alias IP ranges.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alias_ip_ranges: Vec<AliasIpRange>,

    /// Fingerprint for optimistic locking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,

    /// Stack type for this interface.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_type: Option<StackType>,

    /// Network interface card type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nic_type: Option<NicType>,
}

/// Access configuration for external IP.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AccessConfig {
    /// Type of access config.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<AccessConfigType>,

    /// Name of the access config.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// External IP address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nat_i_p: Option<String>,

    /// Network tier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_tier: Option<NetworkTier>,
}

/// Access config type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AccessConfigType {
    /// One-to-one NAT.
    OneToOneNat,
    /// Direct IPv6 access.
    DirectIpv6,
}

/// Alias IP range.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AliasIpRange {
    /// IP CIDR range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_cidr_range: Option<String>,

    /// Subnetwork range name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnetwork_range_name: Option<String>,
}

/// NIC type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NicType {
    /// Virtio NET.
    VirtioNet,
    /// gVNIC.
    Gvnic,
}

/// Metadata configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    /// Fingerprint for optimistic locking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,

    /// Metadata items.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<MetadataItem>,

    /// Type of resource.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Metadata item.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct MetadataItem {
    /// Key of the metadata item.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,

    /// Value of the metadata item.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// Service account configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ServiceAccount {
    /// Email address of the service account.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// OAuth scopes.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
}

/// Tags configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Tags {
    /// Fingerprint for optimistic locking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,

    /// Tag items.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<String>,
}

/// Scheduling configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Scheduling {
    /// On host maintenance behavior.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on_host_maintenance: Option<OnHostMaintenance>,

    /// Automatic restart enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automatic_restart: Option<bool>,

    /// Whether this is a preemptible instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preemptible: Option<bool>,

    /// Provisioning model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisioning_model: Option<ProvisioningModel>,
}

/// On host maintenance behavior.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OnHostMaintenance {
    /// Migrate during maintenance.
    Migrate,
    /// Terminate during maintenance.
    Terminate,
}

/// Provisioning model.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProvisioningModel {
    /// Standard provisioning.
    Standard,
    /// Spot provisioning.
    Spot,
}

/// Accelerator configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct AcceleratorConfig {
    /// Type of accelerator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accelerator_type: Option<String>,

    /// Number of accelerators.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accelerator_count: Option<i32>,
}

/// Shielded instance configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ShieldedInstanceConfig {
    /// Enable secure boot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_secure_boot: Option<bool>,

    /// Enable vTPM.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_vtpm: Option<bool>,

    /// Enable integrity monitoring.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_integrity_monitoring: Option<bool>,
}

/// Confidential instance configuration.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ConfidentialInstanceConfig {
    /// Enable confidential compute.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_confidential_compute: Option<bool>,
}
