use super::*;

// =============================================================================================
// Data Structures - Instance Group Manager
// =============================================================================================

/// Represents an instance group manager resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/instanceGroupManagers
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceGroupManager {
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

    /// URL of the managed instance group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_group: Option<String>,

    /// URL of the instance template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_template: Option<String>,

    /// Target size of the managed instance group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_size: Option<i32>,

    /// Base instance name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_instance_name: Option<String>,

    /// Current actions summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_actions: Option<InstanceGroupManagerActionsSummary>,

    /// Status of the managed instance group.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<InstanceGroupManagerStatus>,

    /// Target pools for this manager.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_pools: Vec<String>,

    /// Named ports for this manager.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub named_ports: Vec<NamedPort>,

    /// Fingerprint for optimistic locking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,

    /// Zone URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,

    /// Auto healing policies.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub auto_healing_policies: Vec<InstanceGroupManagerAutoHealingPolicy>,

    /// Update policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_policy: Option<InstanceGroupManagerUpdatePolicy>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#instanceGroupManager").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Summary of instance group manager actions.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceGroupManagerActionsSummary {
    /// Number of instances currently being created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creating: Option<i32>,

    /// Number of instances currently being deleted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleting: Option<i32>,

    /// Number of instances that exist and are running.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub none: Option<i32>,

    /// Number of instances currently being recreated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recreating: Option<i32>,

    /// Number of instances currently being refreshed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refreshing: Option<i32>,

    /// Number of instances currently being restarted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restarting: Option<i32>,

    /// Number of instances currently being verified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verifying: Option<i32>,

    /// Number of instances currently being abandoned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub abandoning: Option<i32>,

    /// Number of instances in a creating without retries state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creating_without_retries: Option<i32>,
}

/// Status of an instance group manager.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceGroupManagerStatus {
    /// Whether the group is stable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_stable: Option<bool>,

    /// Stateful status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stateful: Option<InstanceGroupManagerStatusStateful>,

    /// Version target status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_target: Option<InstanceGroupManagerStatusVersionTarget>,
}

/// Stateful status for instance group manager.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceGroupManagerStatusStateful {
    /// Whether there are stateful instances.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_stateful_config: Option<bool>,

    /// Whether per-instance configs exist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_instance_configs: Option<InstanceGroupManagerStatusStatefulPerInstanceConfigs>,
}

/// Per-instance configs status.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceGroupManagerStatusStatefulPerInstanceConfigs {
    /// Whether all configs are effective.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_effective: Option<bool>,
}

/// Version target status.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceGroupManagerStatusVersionTarget {
    /// Whether the version target has been reached.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_reached: Option<bool>,
}

/// Named port for instance group.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct NamedPort {
    /// Name of the port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Port number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,
}

/// Auto healing policy for instance group manager.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceGroupManagerAutoHealingPolicy {
    /// Health check URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check: Option<String>,

    /// Initial delay in seconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_delay_sec: Option<i32>,
}

/// Update policy for instance group manager.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceGroupManagerUpdatePolicy {
    /// Type of update.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<UpdatePolicyType>,

    /// Minimal action for updates.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimal_action: Option<MinimalAction>,

    /// Most disruptive action allowed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub most_disruptive_allowed_action: Option<MinimalAction>,

    /// Maximum surge instances (fixed or percent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_surge: Option<FixedOrPercent>,

    /// Maximum unavailable instances (fixed or percent).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_unavailable: Option<FixedOrPercent>,

    /// Replacement method.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replacement_method: Option<ReplacementMethod>,
}

/// Update policy type.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UpdatePolicyType {
    /// Opportunistic update.
    Opportunistic,
    /// Proactive update.
    Proactive,
}

/// Minimal action for updates.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MinimalAction {
    /// No action.
    None,
    /// Refresh instance.
    Refresh,
    /// Restart instance.
    Restart,
    /// Replace instance.
    Replace,
}

/// Fixed or percent value.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct FixedOrPercent {
    /// Fixed value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fixed: Option<i32>,

    /// Percentage value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<i32>,

    /// Calculated value (output only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calculated: Option<i32>,
}

/// Replacement method.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReplacementMethod {
    /// Substitute replacement.
    Substitute,
    /// Recreate replacement.
    Recreate,
}

/// Response for list managed instances.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceGroupManagersListManagedInstancesResponse {
    /// List of managed instances.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub managed_instances: Vec<ManagedInstance>,

    /// Next page token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

/// Request to delete selected managed instances from an instance group manager.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct InstanceGroupManagersDeleteInstancesRequest {
    /// Instance URLs to delete from the managed instance group.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instances: Vec<String>,

    /// Continue when valid instances are mixed with already-deleting or non-member instances.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_instances_on_validation_error: Option<bool>,
}

/// Managed instance in an instance group.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ManagedInstance {
    /// URL of the instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,

    /// Instance status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_status: Option<ManagedInstanceStatus>,

    /// Current action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_action: Option<ManagedInstanceCurrentAction>,

    /// Last attempt status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attempt: Option<ManagedInstanceLastAttempt>,

    /// Unique identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Version of the instance template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<ManagedInstanceVersion>,

    /// Health check results for this instance (populated when a health check is attached to the MIG).
    /// JSON field: instanceHealth
    #[serde(
        default,
        rename = "instanceHealth",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub instance_health: Vec<ManagedInstanceHealth>,
}

/// Health state of a managed instance as reported by a health check.
/// Returned in `ManagedInstance.instanceHealth[]` by listManagedInstances.
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ManagedInstanceHealth {
    /// URL of the health check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health_check: Option<String>,

    /// Detailed health state of the instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detailed_health_state: Option<ManagedInstanceDetailedHealthState>,
}

/// Detailed health state values for a managed instance.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManagedInstanceDetailedHealthState {
    /// The instance is reachable and health check responded with HEALTHY.
    Healthy,
    /// The health check responded with UNHEALTHY.
    Unhealthy,
    /// The instance is being drained and will not accept new connections.
    Draining,
    /// The health check timed out.
    Timeout,
    /// The health state is unknown (e.g., health check not yet run).
    Unknown,
}

/// Status of a managed instance.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManagedInstanceStatus {
    /// Instance is running.
    Running,
    /// Instance is pending.
    Pending,
    /// Instance is provisioning.
    Provisioning,
    /// Instance is staging.
    Staging,
    /// Instance is stopped.
    Stopped,
    /// Instance is stopping.
    Stopping,
    /// Instance is suspended.
    Suspended,
    /// Instance is suspending.
    Suspending,
    /// Instance is terminated.
    Terminated,
}

/// Current action on a managed instance.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManagedInstanceCurrentAction {
    /// No action.
    None,
    /// Creating instance.
    Creating,
    /// Creating without retries.
    CreatingWithoutRetries,
    /// Recreating instance.
    Recreating,
    /// Deleting instance.
    Deleting,
    /// Abandoning instance.
    Abandoning,
    /// Restarting instance.
    Restarting,
    /// Refreshing instance.
    Refreshing,
    /// Verifying instance.
    Verifying,
}

/// Last attempt status for a managed instance.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ManagedInstanceLastAttempt {
    /// Errors from last attempt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<ManagedInstanceLastAttemptErrors>,
}

/// Errors from last attempt.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ManagedInstanceLastAttemptErrors {
    /// List of errors.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<ManagedInstanceLastAttemptErrorsErrors>,
}

/// Individual error from last attempt.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ManagedInstanceLastAttemptErrorsErrors {
    /// Error code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    /// Error message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// Structured error details, including quota information when present.
    #[builder(default)]
    #[serde(
        default,
        rename = "errorDetails",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub error_details: Vec<ManagedInstanceLastAttemptErrorDetail>,
}

/// Structured details for a managed instance last-attempt error.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ManagedInstanceLastAttemptErrorDetail {
    /// Quota details for quota-related errors.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quota_info: Option<ManagedInstanceLastAttemptQuotaInfo>,
    /// Localized error message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub localized_message: Option<ManagedInstanceLastAttemptLocalizedMessage>,
}

/// Quota details for a managed instance last-attempt error.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ManagedInstanceLastAttemptQuotaInfo {
    /// Compute Engine quota metric name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metric_name: Option<String>,
    /// Compute Engine quota limit name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_name: Option<String>,
    /// Current effective quota limit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<f64>,
    /// Future quota limit being rolled out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub future_limit: Option<f64>,
}

/// Localized message for a managed instance last-attempt error.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ManagedInstanceLastAttemptLocalizedMessage {
    /// Message locale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    /// Localized message text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Version information for a managed instance.
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ManagedInstanceVersion {
    /// Instance template URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_template: Option<String>,

    /// Version name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

// =============================================================================================
// Data Structures - Instance
// =============================================================================================

/// Represents a compute instance resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/instances
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
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

    /// Machine type URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_type: Option<String>,

    /// Status of the instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<InstanceStatus>,

    /// Zone URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,

    /// Disks attached to this instance.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disks: Vec<AttachedDisk>,

    /// Network interfaces for this instance.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub network_interfaces: Vec<NetworkInterface>,

    /// Metadata for this instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,

    /// Service accounts for this instance.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub service_accounts: Vec<ServiceAccount>,

    /// Tags for this instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Tags>,

    /// Scheduling configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduling: Option<Scheduling>,

    /// Labels for this instance.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub labels: std::collections::HashMap<String, String>,

    /// Whether IP forwarding is allowed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_ip_forward: Option<bool>,

    /// CPU platform.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_platform: Option<String>,

    /// Guest accelerators for this instance.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guest_accelerators: Vec<AcceleratorConfig>,

    /// Shielded instance configuration.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shielded_instance_config: Option<ShieldedInstanceConfig>,

    /// Fingerprint for optimistic locking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Last start timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_start_timestamp: Option<String>,

    /// Last stop timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_stop_timestamp: Option<String>,

    /// Type of resource (always "compute#instance").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Instance status.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InstanceStatus {
    /// Instance is running.
    Running,
    /// Instance is provisioning.
    Provisioning,
    /// Instance is staging.
    Staging,
    /// Instance is stopped.
    Stopped,
    /// Instance is stopping.
    Stopping,
    /// Instance is suspended.
    Suspended,
    /// Instance is suspending.
    Suspending,
    /// Instance is terminated.
    Terminated,
    /// Instance is pending.
    Pending,
}

// =============================================================================================
// Data Structures - Disk
// =============================================================================================

/// Represents a persistent disk resource.
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/disks
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct Disk {
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

    /// Size of the disk in GB.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_gb: Option<String>,

    /// Zone URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,

    /// Status of the disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<DiskStatus>,

    /// Source snapshot URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_snapshot: Option<String>,

    /// Source snapshot ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_snapshot_id: Option<String>,

    /// Source image URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_image: Option<String>,

    /// Source image ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_image_id: Option<String>,

    /// Disk type URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,

    /// Users of this disk (instance URLs).
    #[builder(default)]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub users: Vec<String>,

    /// Labels for this disk.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub labels: std::collections::HashMap<String, String>,

    /// Label fingerprint for optimistic locking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_fingerprint: Option<String>,

    /// Physical block size in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub physical_block_size_bytes: Option<String>,

    /// Provisioned IOPS.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisioned_iops: Option<i64>,

    /// Provisioned throughput.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisioned_throughput: Option<i64>,

    /// Last attach timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attach_timestamp: Option<String>,

    /// Last detach timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_detach_timestamp: Option<String>,

    /// Creation timestamp (RFC3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creation_timestamp: Option<String>,

    /// Type of resource (always "compute#disk").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// Disk status.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DiskStatus {
    /// Disk is being created.
    Creating,
    /// Disk is restoring from snapshot.
    Restoring,
    /// Disk creation failed.
    Failed,
    /// Disk is ready.
    Ready,
    /// Disk is being deleted.
    Deleting,
}

// =============================================================================================
// Data Structures - Serial Port Output
// =============================================================================================

/// Serial port output from a GCP compute instance (port 1 = main console).
/// See: https://cloud.google.com/compute/docs/reference/rest/v1/instances/getSerialPortOutput
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct SerialPortOutput {
    /// The contents of the serial port output.
    pub contents: Option<String>,
    /// The starting byte position of the output that was returned.
    pub start: Option<String>,
    /// The byte position of the next byte to read (pagination cursor).
    pub next: Option<String>,
}
