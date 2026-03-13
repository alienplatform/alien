//! # Cloud Resource Manager Client
//!
//! This module provides a client for interacting with Google Cloud Resource Manager API,
//! specifically for managing IAM policies on projects.
//!
//! ## Example Usage
//!
//! ```rust,no_run
//! use alien_gcp_clients::{ResourceManagerClient, IamPolicy, Binding};
//! use alien_infra::core::GcpClientConfig;
//! use reqwest::Client;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = Client::new();
//! let config = GcpClientConfig::new("my-project-id".to_string(), /* auth config */);
//! let rm_client = ResourceManagerClient::new(client, config);
//!
//! // Get project metadata
//! let project = rm_client.get_project_metadata("my-project-id".to_string()).await?;
//! println!("Project name: {:?}", project.name);
//! println!("Project number: {:?}", project.project_number);
//! println!("Lifecycle state: {:?}", project.lifecycle_state);
//!
//! // Get current IAM policy for a project
//! let policy = rm_client.get_project_iam_policy("my-project-id".to_string(), None).await?;
//!
//! // Add a new binding to the policy
//! let mut updated_policy = policy;
//! updated_policy.bindings.push(Binding {
//!     role: "roles/viewer".to_string(),
//!     members: vec!["user:example@example.com".to_string()],
//!     condition: None,
//! });
//!
//! // Set the updated policy
//! let result = rm_client.set_project_iam_policy("my-project-id".to_string(), updated_policy, None).await?;
//! # Ok(())
//! # }
//!

use crate::gcp::api_client::{GcpClientBase, GcpServiceConfig};
use crate::gcp::iam::IamPolicy;
use crate::gcp::GcpClientConfig;
use alien_client_core::Result;
use bon::Builder;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};

use async_trait::async_trait;
#[cfg(feature = "test-utils")]
use mockall::automock;
use std::fmt::Debug;

/// Cloud Resource Manager service configuration
#[derive(Debug)]
pub struct ResourceManagerServiceConfig;

impl GcpServiceConfig for ResourceManagerServiceConfig {
    fn base_url(&self) -> &'static str {
        "https://cloudresourcemanager.googleapis.com/v1"
    }

    fn default_audience(&self) -> &'static str {
        "https://cloudresourcemanager.googleapis.com/"
    }

    fn service_name(&self) -> &'static str {
        "Cloud Resource Manager"
    }

    fn service_key(&self) -> &'static str {
        "resourcemanager"
    }
}

#[cfg_attr(feature = "test-utils", automock)]
#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
pub trait ResourceManagerApi: Send + Sync + Debug {
    async fn get_project_iam_policy(
        &self,
        project_id: String,
        options: Option<GetPolicyOptions>,
    ) -> Result<IamPolicy>;

    async fn set_project_iam_policy(
        &self,
        project_id: String,
        policy: IamPolicy,
        update_mask: Option<String>,
    ) -> Result<IamPolicy>;

    async fn get_project_metadata(&self, project_id: String) -> Result<Project>;
}

/// Cloud Resource Manager client for managing project IAM policies
#[derive(Debug)]
pub struct ResourceManagerClient {
    base: GcpClientBase,
}

impl ResourceManagerClient {
    pub fn new(client: Client, config: GcpClientConfig) -> Self {
        Self {
            base: GcpClientBase::new(client, config, Box::new(ResourceManagerServiceConfig)),
        }
    }
}

#[cfg_attr(target_arch = "wasm32", async_trait::async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait::async_trait)]
impl ResourceManagerApi for ResourceManagerClient {
    /// Gets the IAM policy for a project.
    ///
    /// # Arguments
    ///
    /// * `project_id` - The ID of the GCP project (e.g., "my-project-123")
    /// * `options` - Optional policy options to control the format of the returned policy
    ///
    /// # Returns
    ///
    /// Returns the current IAM policy for the specified project.
    ///
    /// See: https://cloud.google.com/resource-manager/reference/rest/v1/projects/getIamPolicy
    async fn get_project_iam_policy(
        &self,
        project_id: String,
        options: Option<GetPolicyOptions>,
    ) -> Result<IamPolicy> {
        let path = format!("projects/{}:getIamPolicy", project_id);
        let request_body = options.map(|opts| GetIamPolicyRequest {
            options: Some(opts),
        });

        self.base
            .execute_request(Method::POST, &path, None, request_body, &project_id)
            .await
    }

    /// Sets the IAM policy for a project.
    ///
    /// # Arguments
    ///
    /// * `project_id` - The ID of the GCP project (e.g., "my-project-123")
    /// * `policy` - The complete IAM policy to apply to the project
    /// * `update_mask` - Optional field mask specifying which fields to modify.
    ///   If not provided, defaults to "bindings, etag"
    ///
    /// # Returns
    ///
    /// Returns the updated IAM policy as applied to the project.
    ///
    /// # Warning
    ///
    /// This method will replace the existing policy and cannot be used to append
    /// additional IAM settings. It's important to get the current policy first,
    /// modify it, and then set the complete policy.
    ///
    /// See: https://cloud.google.com/resource-manager/reference/rest/v1/projects/setIamPolicy
    async fn set_project_iam_policy(
        &self,
        project_id: String,
        policy: IamPolicy,
        update_mask: Option<String>,
    ) -> Result<IamPolicy> {
        let path = format!("projects/{}:setIamPolicy", project_id);
        let request = ResourceManagerSetIamPolicyRequest {
            policy,
            update_mask: update_mask.map(|s| s.to_string()),
        };

        self.base
            .execute_request(Method::POST, &path, None, Some(request), &project_id)
            .await
    }

    /// Gets metadata for a project.
    ///
    /// # Arguments
    ///
    /// * `project_id` - The ID of the GCP project (e.g., "my-project-123")
    ///
    /// # Returns
    ///
    /// Returns project metadata including project number, name, and other details.
    ///
    /// See: https://cloud.google.com/resource-manager/reference/rest/v1/projects/get
    async fn get_project_metadata(&self, project_id: String) -> Result<Project> {
        let path = format!("projects/{}", project_id);

        self.base
            .execute_request(Method::GET, &path, None, Option::<()>::None, &project_id)
            .await
    }
}

// --- Data Structures ---

/// Request message for getting IAM policy.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct GetIamPolicyRequest {
    /// Optional policy options for the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<GetPolicyOptions>,
}

/// Encapsulates settings provided to GetIamPolicy.
/// Based on: https://cloud.google.com/resource-manager/reference/rest/v1/projects/getIamPolicy#GetPolicyOptions
#[derive(Debug, Serialize, Deserialize, Clone, Default, Builder)]
#[serde(rename_all = "camelCase")]
pub struct GetPolicyOptions {
    /// Optional. The maximum policy version that will be used to format the policy.
    /// Valid values are 0, 1, and 3. Requests specifying an invalid value will be rejected.
    /// Requests for policies with any conditional role bindings must specify version 3.
    /// Policies with no conditional role bindings may specify any valid value or leave the field unset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requested_policy_version: Option<i32>,
}

/// Request message for setting IAM policy in Resource Manager.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[serde(rename_all = "camelCase")]
pub struct ResourceManagerSetIamPolicyRequest {
    /// The policy to be applied.
    pub policy: IamPolicy,

    /// Optional. A FieldMask specifying which fields of the policy to modify.
    /// Only the fields in the mask will be modified. If no mask is provided,
    /// the following default mask is used: "bindings, etag"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_mask: Option<String>,
}

/// Project metadata response from the Resource Manager API.
/// Based on: https://cloud.google.com/resource-manager/reference/rest/v1/projects#Project
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// The unique, user-assigned ID of the project. Example: "my-project-123"
    /// Read-only after creation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,

    /// The number uniquely identifying the project. Example: "415104041262"
    /// Read-only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_number: Option<String>,

    /// The optional user-assigned display name of the project.
    /// When present it must be between 4 to 30 characters.
    /// Read-write.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// The project lifecycle state.
    /// Read-only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lifecycle_state: Option<LifecycleState>,

    /// Creation time in RFC 3339 format.
    /// Read-only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create_time: Option<String>,

    /// The labels associated with this project.
    /// Label keys must be between 1 and 63 characters long and must conform to [a-z][a-z0-9_-]{0,62}.
    /// Label values must be between 0 and 63 characters long and must conform to [a-z0-9_-]{0,63}.
    /// No more than 256 labels can be associated with a given resource.
    /// Read-write.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<std::collections::HashMap<String, String>>,

    /// An optional reference to a parent Resource.
    /// Supported parent types include "organization" and "folder".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<ResourceId>,

    /// Optional. Input only. Immutable. Tag keys/values directly bound to this project.
    /// Each item in the map must be expressed as " : ".
    /// Note: Currently this field is in Preview.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<std::collections::HashMap<String, String>>,

    /// Output only. If this project is a Management Project, list of capabilities configured on the parent folder.
    /// Note, presence of any capability implies that this is a Management Project.
    /// Example: folders/123/capabilities/app-management.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configured_capabilities: Option<Vec<String>>,
}

/// Project lifecycle states.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LifecycleState {
    /// Unspecified state. This is only used/useful for distinguishing unset values.
    LifecycleStateUnspecified,
    /// The normal and active state.
    Active,
    /// The project has been marked for deletion by the user (by invoking projects.delete)
    /// or by the system (Google Cloud Platform). This can generally be reversed by invoking projects.undelete.
    DeleteRequested,
    /// This lifecycle state is no longer used and not returned by the API.
    DeleteInProgress,
}

/// Resource identifier for parent resources.
/// A container to reference an id for any resource type.
#[derive(Debug, Serialize, Deserialize, Clone, Builder)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct ResourceId {
    /// Resource type (e.g., "organization", "folder", "project")
    #[serde(rename = "type")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_type: Option<String>,

    /// The type-specific id. This should correspond to the id used in the type-specific API's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}
