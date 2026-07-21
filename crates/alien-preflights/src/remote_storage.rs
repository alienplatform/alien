use crate::error::Result;
use crate::{CheckResult, DeploymentPrerequisiteCheck};
use alien_core::{DeploymentConfig, Platform, ResourceLifecycle, Stack, StackState, Storage};

pub(crate) const REMOTE_STORAGE_DATA_WRITE_PERMISSION_SET_ID: &str = "storage/remote-data-write";

/// Remote Storage is supported only for setup-owned cloud resources that opt
/// into publication. This is shared by permission derivation and validation so
/// the two preflight phases cannot disagree about which resources are exposed.
pub(crate) fn resource_ids(stack: &Stack, platform: Platform) -> Vec<String> {
    if !matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure) {
        return Vec::new();
    }

    stack
        .resources()
        .filter(|(_, entry)| {
            entry.remote_access
                && entry.lifecycle == ResourceLifecycle::Frozen
                && entry.config.downcast_ref::<Storage>().is_some()
        })
        .map(|(resource_id, _)| resource_id.clone())
        .collect()
}

/// Remote Bindings v0 supports only cloud Storage created by the generated
/// setup. A supplied external binding may refer to an arbitrary pre-existing
/// resource, so it cannot participate in this credential-grant flow.
pub(crate) struct ExternalBindingCheck;

#[async_trait::async_trait]
impl DeploymentPrerequisiteCheck for ExternalBindingCheck {
    fn code(&self) -> Option<&'static str> {
        Some("REMOTE_STORAGE_EXTERNAL_BINDING_UNSUPPORTED")
    }

    fn description(&self) -> &'static str {
        "Remote Storage must be created by setup rather than supplied as an external binding"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> bool {
        resource_ids(stack, stack_state.platform)
            .iter()
            .any(|resource_id| config.external_bindings.has(resource_id))
    }

    async fn check(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<CheckResult> {
        let errors = resource_ids(stack, stack_state.platform)
            .into_iter()
            .filter(|resource_id| config.external_bindings.has(resource_id))
            .map(|resource_id| {
                format!(
                    "Remote Storage resource '{resource_id}' cannot use an external binding. Remove the external binding so customer setup creates and owns a dedicated bucket or container."
                )
            })
            .collect::<Vec<_>>();

        if errors.is_empty() {
            Ok(CheckResult::success())
        } else {
            Ok(CheckResult::failed(errors))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        bindings::StorageBinding, EnvironmentVariablesSnapshot, ExternalBinding, ExternalBindings,
    };

    fn deployment_config() -> DeploymentConfig {
        DeploymentConfig::builder()
            .stack_settings(Default::default())
            .environment_variables(EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: "empty".to_string(),
                created_at: "2026-07-21T00:00:00Z".to_string(),
            })
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build()
    }

    #[tokio::test]
    async fn external_bindings_are_rejected_on_every_supported_cloud() {
        let stack = Stack::new("remote-storage".to_string())
            .add_with_remote_access(
                Storage::new("uploads".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let mut config = deployment_config();
        config.external_bindings.insert(
            "uploads",
            ExternalBinding::Storage(StorageBinding::s3("existing-bucket")),
        );
        let check = ExternalBindingCheck;

        assert_eq!(
            check.code(),
            Some("REMOTE_STORAGE_EXTERNAL_BINDING_UNSUPPORTED")
        );
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let state = StackState::new(platform);
            assert!(check.should_run(&stack, &state, &config));
            let result = check.check(&stack, &state, &config).await.unwrap();
            assert_eq!(result.errors.len(), 1);
            assert!(result.errors[0].contains("cannot use an external binding"));
            assert!(result.errors[0].contains("setup creates and owns"));
        }

        assert!(!check.should_run(&stack, &StackState::new(Platform::Local), &config));
    }
}
