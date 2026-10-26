//! Revoke credentials for retained registries.
use super::GcpArtifactRegistryController;
use crate::{ErrorData, PlatformServiceProvider, Result, StackResourceStateExt};
use alien_client_core::ErrorData as ClientError;
use alien_core::{ClientConfig, Platform, ResourceStatus, StackResourceState, StackState};
use alien_error::{AlienError, ContextError};
use serde_json::Value;
/// Delete the pull and push service accounts of each retained GCP registry.
///
/// The repository and its images stay; nothing the deployment ran can use them afterwards. This
/// matches AWS, whose registry roles are deleted while its repositories stay. Azure registries
/// have no registry-scoped identities: access came from the deployment's own identities, which
/// teardown removes with them.
pub async fn revoke_retained_registry_identities(
    state: &StackState,
    retained_ids: &[String],
    client_config: &ClientConfig,
    provider: &dyn PlatformServiceProvider,
) -> Result<()> {
    for id in retained_ids {
        let Some(resource) = state.resources.get(id) else {
            continue;
        };
        if resource.status == ResourceStatus::Deleted
            || !owned_registry(
                state,
                resource,
                Platform::Gcp,
                "GcpArtifactRegistryController",
            )
        {
            continue;
        }
        let controller =
            resource.get_internal_controller_typed::<GcpArtifactRegistryController>()?;
        let config = client_config
            .gcp_config()
            .ok_or_else(|| invalid(id, "GCP setup credentials are required"))?;
        let client = provider.get_gcp_iam_client(config)?;
        for email in [
            &controller.pull_service_account_email,
            &controller.push_service_account_email,
        ]
        .into_iter()
        .flatten()
        {
            match client.delete_service_account(email.clone()).await {
                Ok(()) => {}
                Err(error) if missing(&error) => {}
                Err(error) => return Err(error.context(cloud_error(id))),
            }
        }
    }
    Ok(())
}

/// A registry this deployment created on `platform`, including one whose declaration was
/// replaced by an interrupted update.
fn owned_registry(
    state: &StackState,
    resource: &StackResourceState,
    platform: Platform,
    controller_type: &str,
) -> bool {
    resource.controller_platform.unwrap_or(state.platform) == platform
        && std::iter::once(&resource.config)
            .chain(resource.previous_config.iter())
            .any(|config| config.resource_type().as_ref() == "artifact-registry")
        && resource
            .internal_state
            .as_ref()
            .and_then(|value| value.get("type"))
            .and_then(Value::as_str)
            == Some(controller_type)
}

fn missing(error: &AlienError<ClientError>) -> bool {
    matches!(
        error.error,
        Some(ClientError::RemoteResourceNotFound { .. })
    )
}

fn invalid(id: &str, message: &str) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::ResourceConfigInvalid {
        resource_id: Some(id.to_string()),
        message: message.to_string(),
    })
}

fn cloud_error(id: &str) -> ErrorData {
    ErrorData::CloudPlatformError {
        resource_id: Some(id.to_string()),
        message: "Failed to clean up registry access or repositories; restore the setup grants and retry deletion".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MockPlatformServiceProvider, ResourceController};
    use alien_core::{ArtifactRegistry, GcpClientConfig, Resource};
    use alien_gcp_clients::{iam::MockIamApi, GcpClientConfigExt};
    use std::sync::{Arc, Mutex};
    fn installed(
        platform: Platform,
        entries: Vec<(&str, Box<dyn ResourceController>)>,
    ) -> StackState {
        let mut state = StackState::new(platform);
        state.resource_prefix = "owned".into();
        for (id, controller) in entries {
            let mut resource = StackResourceState::new_pending(
                "artifact-registry".into(),
                Resource::new(ArtifactRegistry::new(id.into()).build()),
                None,
                vec![],
            );
            resource.status = ResourceStatus::Running;
            resource.set_internal_controller(Some(controller)).unwrap();
            state.resources.insert(id.into(), resource);
        }
        serde_json::from_value(serde_json::to_value(state).unwrap()).unwrap()
    }

    #[tokio::test]
    async fn retained_gcp_registry_loses_its_service_accounts_but_not_its_repository() {
        let deleted = Arc::new(Mutex::new(Vec::new()));
        let mut iam = MockIamApi::new();
        let recorded = deleted.clone();
        iam.expect_delete_service_account().returning(move |email| {
            recorded.lock().unwrap().push(email.clone());
            if email.starts_with("push") {
                return Err(AlienError::new(ClientError::RemoteResourceNotFound {
                    resource_type: "ServiceAccount".into(),
                    resource_name: email,
                }));
            }
            Ok(())
        });
        let iam = Arc::new(iam);
        let mut provider = MockPlatformServiceProvider::new();
        provider
            .expect_get_gcp_iam_client()
            .returning(move |_| Ok(iam.clone()));
        // No Artifact Registry client is expected: the repository is never touched.
        let registry = |name: &str| -> Box<dyn ResourceController> {
            Box::new(GcpArtifactRegistryController {
                repository_name: Some(format!("owned-{name}")),
                pull_service_account_email: Some(format!("pull-{name}@p.iam.gserviceaccount.com")),
                push_service_account_email: Some(format!("push-{name}@p.iam.gserviceaccount.com")),
                ..Default::default()
            })
        };
        let state = installed(
            Platform::Gcp,
            vec![
                ("images", registry("images")),
                ("erased", registry("erased")),
            ],
        );
        let client = ClientConfig::Gcp(Box::new(GcpClientConfig::mock()));

        revoke_retained_registry_identities(&state, &["images".into()], &client, &provider)
            .await
            .unwrap();

        assert_eq!(
            *deleted.lock().unwrap(),
            vec![
                "pull-images@p.iam.gserviceaccount.com".to_string(),
                "push-images@p.iam.gserviceaccount.com".to_string(),
            ]
        );
    }
}
