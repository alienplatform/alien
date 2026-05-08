//! `alien_deployment` resource lifecycle.
//!
//! Drives the manager's `/v1/stack/import` endpoint via [`alien_manager_api`]
//! during Create / Read / Delete. Update is not implemented — every change to
//! the resource triggers a delete + create cycle.
//!
//! The manager treats repeated imports for the same imported deployment as an
//! upsert, replacing the imported stack state on refresh/apply.

use alien_manager_api::types::{
    ImportedResource, ManagementConfig, Platform, StackImportRequest, StackImportResponse,
    StackSettings,
};
use alien_manager_api::{Client, SdkResultExt};

use serde::{Deserialize, Serialize};

/// HCL-side input. Mirrors the [`crate::schema::resource_schema`] attributes
/// and round-trips through `serde` for testability without dragging the
/// tfplugin6 protobuf shape into this layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlienDeploymentInput {
    /// Manager URL. Falls back to the white-label default when empty.
    pub manager_url: Option<String>,
    pub deployment_group_token: String,
    /// Deployment name. Required, unique within the deployment group.
    pub name: String,
    /// Physical stack prefix used by the generated module.
    pub stack_prefix: String,
    pub platform: Platform,
    pub region: String,
    pub management_config: ManagementConfig,
    pub stack_settings: StackSettings,
    pub resources: Vec<ImportedResource>,
}

/// State persisted in the Terraform state file after Create.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlienDeploymentState {
    pub deployment_id: String,
    pub manager_url: String,
    pub name: String,
    pub platform: Platform,
    pub region: String,
}

/// Errors surfaced from CRUD operations. The variants match the categories
/// Terraform itself classifies in its diagnostics — "transient" → retryable
/// step, "validation" → planner abort, "auth" → user error.
#[derive(Debug, thiserror::Error)]
pub enum CrudError {
    #[error("validation: {0}")]
    Validation(String),
    #[error("authentication failed (token rejected by manager)")]
    Unauthorized,
    #[error("not found: deployment is missing on the manager — drift")]
    NotFound,
    /// Deployment name already exists in the deployment group. Surfaced as
    /// no-op on Read (the deployment exists, no drift) and as a hard
    /// validation error on Create (the user picked a duplicate name).
    #[error("deployment name already exists in deployment group")]
    Conflict,
    #[error("manager error: {0}")]
    Manager(String),
    #[error("transport error: {0}")]
    Transport(String),
}

/// Build a manager client. Resolution order: explicit `manager_url` > white-
/// label default > error. Tests inject a fake `Client` directly and skip
/// this helper.
pub fn build_client(input: &AlienDeploymentInput) -> Result<Client, CrudError> {
    let url = match &input.manager_url {
        Some(u) if !u.is_empty() => u.clone(),
        _ => {
            return Err(CrudError::Validation(
                "manager_url is required when no white-label default is baked in".into(),
            ));
        }
    };
    Ok(Client::new(&url))
}

/// Resource Create: ship the resolved import payload to the manager.
///
/// Returns the response unchanged so the caller can populate computed
/// attributes (`deployment_id`) before persisting state.
pub async fn create(
    client: &Client,
    input: &AlienDeploymentInput,
) -> Result<StackImportResponse, CrudError> {
    let body = build_request(input)?;
    let response = client
        .stack_import()
        .body(body)
        .send()
        .await
        .into_sdk_error()
        .map_err(map_sdk_err)?;
    Ok(response.into_inner())
}

/// Resource Read: re-issue the import request as a drift probe. The manager
/// treats repeated imports for imported deployments as an upsert; a conflict
/// now means the name belongs to a native deployment and must surface.
pub async fn read(client: &Client, input: &AlienDeploymentInput) -> Result<(), CrudError> {
    create(client, input).await.map(|_| ())
}

/// Resource Delete: tear down the manager-side deployment. The manager's
/// `DELETE /v1/deployments/{id}` performs the actual lifecycle — this CRUD
/// helper just calls it. Failures map to retryable errors so a flaky network
/// doesn't leave Terraform thinking the deployment is gone when it isn't.
pub async fn delete(client: &Client, deployment_id: &str) -> Result<(), CrudError> {
    client
        .delete_deployment()
        .id(deployment_id)
        .send()
        .await
        .into_sdk_error()
        .map_err(map_sdk_err)?;
    Ok(())
}

fn build_request(input: &AlienDeploymentInput) -> Result<StackImportRequest, CrudError> {
    if input.deployment_group_token.is_empty() {
        return Err(CrudError::Validation(
            "deployment_group_token is required".into(),
        ));
    }
    if input.name.trim().is_empty() {
        return Err(CrudError::Validation("name is required".into()));
    }
    if input.stack_prefix.trim().is_empty() {
        return Err(CrudError::Validation("stack_prefix is required".into()));
    }
    if input.region.is_empty() {
        return Err(CrudError::Validation("region is required".into()));
    }
    if input.resources.is_empty() {
        return Err(CrudError::Validation(
            "resources must contain at least one entry".into(),
        ));
    }
    Ok(StackImportRequest {
        deployment_group_token: input.deployment_group_token.clone(),
        deployment_name: input.name.clone(),
        stack_prefix: input.stack_prefix.clone(),
        management_config: input.management_config.clone(),
        platform: input.platform,
        region: input.region.clone(),
        resources: input.resources.clone(),
        source_kind: Some(alien_manager_api::types::ImportSourceKind::Terraform),
        release_id: None,
        stack_settings: input.stack_settings.clone(),
    })
}

fn map_sdk_err(err: alien_error::AlienError<alien_error::GenericError>) -> CrudError {
    let msg = err.message.clone();
    // The SDK's `convert_sdk_error` stamps a code prefix derived from the HTTP
    // status. Match on the prefix so we keep the typed Terraform diagnostics
    // (auth vs not-found vs generic) without touching the SDK.
    if msg.starts_with("Unexpected response: 401") || msg.starts_with("Unexpected response: 403") {
        CrudError::Unauthorized
    } else if msg.starts_with("Unexpected response: 404") {
        CrudError::NotFound
    } else if msg.starts_with("Unexpected response: 409") {
        CrudError::Conflict
    } else if msg.starts_with("Unexpected response: 5") {
        CrudError::Transport(msg)
    } else {
        CrudError::Manager(msg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_input() -> AlienDeploymentInput {
        // Construct from JSON so this test isn't brittle to schema-codegen
        // additions in the SDK — every required field of StackSettings /
        // ManagementConfig has a JSON form here, and any new required
        // additions will fail to deserialize loudly.
        let mc: ManagementConfig = serde_json::from_value(serde_json::json!({
            "platform": "aws",
            "managingRoleArn": "arn:aws:iam::000000000000:role/test"
        }))
        .expect("management config json");
        let stack_settings: StackSettings =
            serde_json::from_value(serde_json::json!({})).expect("stack settings json");
        let resource: ImportedResource = serde_json::from_value(serde_json::json!({
            "id": "data",
            "type": "storage",
            "importData": {
                "bucketName": "acme-data",
                "bucketArn": "arn:aws:s3:::acme-data"
            }
        }))
        .expect("imported resource json");
        AlienDeploymentInput {
            manager_url: Some("http://localhost:0".into()),
            deployment_group_token: "tok".into(),
            name: "acme-prod".into(),
            stack_prefix: "acme-stack".into(),
            platform: Platform::Aws,
            region: "us-east-1".into(),
            management_config: mc,
            stack_settings,
            resources: vec![resource],
        }
    }

    #[test]
    fn build_request_rejects_empty_token() {
        let mut input = dummy_input();
        input.deployment_group_token.clear();
        let err = build_request(&input).unwrap_err();
        assert!(matches!(err, CrudError::Validation(_)));
    }

    #[test]
    fn build_request_rejects_empty_name() {
        let mut input = dummy_input();
        input.name.clear();
        let err = build_request(&input).unwrap_err();
        assert!(matches!(err, CrudError::Validation(_)));
    }

    #[test]
    fn build_request_propagates_name_to_deployment_name() {
        let input = dummy_input();
        let req = build_request(&input).unwrap();
        assert_eq!(req.deployment_name, "acme-prod");
    }

    #[test]
    fn build_request_propagates_stack_prefix() {
        let input = dummy_input();
        let req = build_request(&input).unwrap();
        assert_eq!(req.stack_prefix, "acme-stack");
    }

    #[test]
    fn build_request_rejects_empty_resources() {
        let mut input = dummy_input();
        input.resources.clear();
        let err = build_request(&input).unwrap_err();
        assert!(matches!(err, CrudError::Validation(_)));
    }

    #[test]
    fn build_client_requires_manager_url() {
        let mut input = dummy_input();
        input.manager_url = None;
        let err = build_client(&input).unwrap_err();
        assert!(matches!(err, CrudError::Validation(_)));
    }

    fn make_alien_err(msg: &str) -> alien_error::AlienError<alien_error::GenericError> {
        let mut err = alien_error::AlienError::new(alien_error::GenericError {
            message: msg.to_string(),
        });
        err.message = msg.to_string();
        err
    }

    #[test]
    fn map_sdk_err_categorizes_status_codes() {
        assert!(matches!(
            map_sdk_err(make_alien_err("Unexpected response: 401 Unauthorized")),
            CrudError::Unauthorized
        ));
        assert!(matches!(
            map_sdk_err(make_alien_err("Unexpected response: 404 Not Found")),
            CrudError::NotFound
        ));
        assert!(matches!(
            map_sdk_err(make_alien_err("Unexpected response: 409 Conflict")),
            CrudError::Conflict
        ));
        assert!(matches!(
            map_sdk_err(make_alien_err(
                "Unexpected response: 503 Service Unavailable"
            )),
            CrudError::Transport(_)
        ));
        assert!(matches!(
            map_sdk_err(make_alien_err("Unexpected response: 400 Bad Request")),
            CrudError::Manager(_)
        ));
    }
}
