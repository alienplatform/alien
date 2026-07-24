use super::common::{Filter, TagSet, TagSpecification};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use bon::Builder;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Launch Template Request/Response Types
// ---------------------------------------------------------------------------

/// Request to create a launch template.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct CreateLaunchTemplateRequest {
    /// The name for the launch template.
    pub launch_template_name: String,
    /// A description for the launch template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_description: Option<String>,
    /// The launch template data.
    pub launch_template_data: RequestLaunchTemplateData,
    /// Tags for the launch template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// Launch template data for the request.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct RequestLaunchTemplateData {
    /// The ID of the AMI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_id: Option<String>,
    /// The instance type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_type: Option<String>,
    /// The key name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_name: Option<String>,
    /// The user data (base64-encoded).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_data: Option<String>,
    /// The security group IDs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_group_ids: Option<Vec<String>>,
    /// The IAM instance profile.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iam_instance_profile: Option<LaunchTemplateIamInstanceProfileSpecification>,
    /// Block device mappings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_device_mappings: Option<Vec<LaunchTemplateBlockDeviceMapping>>,
    /// Network interfaces.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network_interfaces: Option<Vec<LaunchTemplateNetworkInterface>>,
    /// Metadata options.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_options: Option<LaunchTemplateInstanceMetadataOptions>,
    /// CPU options (currently only `NestedVirtualization`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_options: Option<LaunchTemplateCpuOptions>,
    /// Tags to apply to resources created from the launch template.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_specifications: Option<Vec<TagSpecification>>,
}

/// IAM instance profile specification for launch template.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct LaunchTemplateIamInstanceProfileSpecification {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Block device mapping for launch template.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct LaunchTemplateBlockDeviceMapping {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ebs: Option<LaunchTemplateEbsBlockDevice>,
}

/// EBS block device for launch template.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct LaunchTemplateEbsBlockDevice {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_size: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete_on_termination: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iops: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub throughput: Option<i32>,
}

/// Network interface for launch template.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct LaunchTemplateNetworkInterface {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_index: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub associate_public_ip_address: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subnet_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<String>>,
}

/// Instance metadata options for launch template.
#[derive(Debug, Clone, Serialize, Builder)]
pub struct LaunchTemplateInstanceMetadataOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_tokens: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_put_response_hop_limit: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_metadata_tags: Option<String>,
}

/// CPU options for launch template.
///
/// AWS only accepts `nested_virtualization = "enabled"` on 8th-generation
/// Intel instance types (c8i, m8i, r8i, and their flex variants). Other
/// instance types will reject the launch with a clear error — we let AWS
/// be the authority on supported types rather than maintaining our own
/// allowlist.
///
/// See: https://docs.aws.amazon.com/AWSEC2/latest/APIReference/API_LaunchTemplateCpuOptionsRequest.html
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct LaunchTemplateCpuOptions {
    /// "enabled" or "disabled". Omit to leave at AWS default ("disabled").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nested_virtualization: Option<String>,
}

/// Response from creating a launch template.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateLaunchTemplateResponse {
    pub launch_template: Option<LaunchTemplate>,
}

/// Request to create a new version of an existing launch template.
/// See: https://docs.aws.amazon.com/AWSEC2/latest/APIReference/API_CreateLaunchTemplateVersion.html
#[derive(Debug, Clone, Builder, Default)]
pub struct CreateLaunchTemplateVersionRequest {
    pub launch_template_id: Option<String>,
    pub launch_template_name: Option<String>,
    pub source_version: Option<String>,
    pub version_description: Option<String>,
    pub launch_template_data: RequestLaunchTemplateData,
}

/// Response from creating a launch template version.
#[derive(Debug, Deserialize)]
pub struct CreateLaunchTemplateVersionResponse {
    #[serde(rename = "launchTemplateVersion")]
    pub launch_template_version: Option<LaunchTemplateVersion>,
}

/// A version of a launch template.
#[derive(Debug, Clone, Deserialize)]
pub struct LaunchTemplateVersion {
    #[serde(rename = "launchTemplateId")]
    pub launch_template_id: Option<String>,
    #[serde(rename = "versionNumber")]
    pub version_number: Option<i64>,
}

/// Request to delete a launch template.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DeleteLaunchTemplateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_template_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_template_name: Option<String>,
}

/// Response from deleting a launch template.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteLaunchTemplateResponse {
    pub launch_template: Option<LaunchTemplate>,
}

/// Request to describe launch templates.
#[derive(Debug, Clone, Serialize, Builder, Default)]
pub struct DescribeLaunchTemplatesRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_template_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_template_names: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Vec<Filter>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_token: Option<String>,
}

/// Response from describing launch templates.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DescribeLaunchTemplatesResponse {
    #[serde(rename = "launchTemplates")]
    pub launch_templates: Option<LaunchTemplateSet>,
    #[serde(rename = "nextToken")]
    pub next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchTemplateSet {
    #[serde(rename = "item", default)]
    pub items: Vec<LaunchTemplate>,
}

/// Represents a launch template.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchTemplate {
    pub launch_template_id: Option<String>,
    pub launch_template_name: Option<String>,
    pub create_time: Option<String>,
    pub created_by: Option<String>,
    pub default_version_number: Option<i64>,
    pub latest_version_number: Option<i64>,
    #[serde(rename = "tagSet")]
    pub tag_set: Option<TagSet>,
}

/// Response from the GetConsoleOutput API.
/// See: https://docs.aws.amazon.com/AWSEC2/latest/APIReference/API_GetConsoleOutput.html
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GetConsoleOutputResponse {
    /// The ID of the instance.
    #[serde(rename = "instanceId")]
    pub instance_id: Option<String>,
    /// The console output, base64-encoded.
    pub output: Option<String>,
    /// The time at which the output was last updated.
    pub timestamp: Option<String>,
}

impl GetConsoleOutputResponse {
    /// Decodes the base64-encoded output to a UTF-8 string.
    pub fn decode_output(&self) -> Option<String> {
        self.output.as_ref().and_then(|b64| {
            STANDARD
                .decode(b64.trim())
                .ok()
                .and_then(|bytes| String::from_utf8(bytes).ok())
        })
    }
}
