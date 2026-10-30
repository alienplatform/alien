//! Give a deployment's Live AWS sandboxes a bucket of its own to build their images from.

use crate::error::Result;
use crate::StackMutation;
use alien_core::permissions::{ManagementPermissions, PermissionProfile, PermissionSetReference};
use alien_core::sandbox_build_role::SANDBOX_BUNDLE_STORAGE_ID;
use alien_core::{
    DeploymentConfig, Platform, RemoteStackManagement, Resource, ResourceEntry, ResourceLifecycle,
    ResourceRef, Sandbox, SandboxCode, Stack, StackState, Storage,
};
use async_trait::async_trait;
use tracing::info;

/// Adds the Storage a Live AWS sandbox's controller renders its image-build bundle into.
///
/// `CreateMicrovmImage` builds from a zip in S3 in the image's own region. A Live sandbox released
/// as a container image has no zip, so its controller writes one, `FROM` the release's image, into
/// this Storage, which every such sandbox depends on. Management may write it and each sandbox's
/// build role may read it; the emitters render both from the permission sets.
///
/// The Storage is Frozen, created by setup, so management never creates or deletes a bucket. An
/// installed deployment whose setup predates it would need setup to add it, so a runtime update of
/// such a deployment leaves it out, and its sandboxes keep building from the bundle a publisher put
/// in the store their setup granted. The next setup adds it: setup prepares its stack from empty
/// state, and later updates find it installed.
pub struct SandboxBundleStorageMutation;

#[async_trait]
impl StackMutation for SandboxBundleStorageMutation {
    fn description(&self) -> &'static str {
        "Add the bucket Live AWS sandboxes build their images from"
    }

    fn should_run(&self, stack: &Stack, state: &StackState, _config: &DeploymentConfig) -> bool {
        state.platform == Platform::Aws
            && (state.resources.is_empty()
                || state.resources.contains_key(SANDBOX_BUNDLE_STORAGE_ID))
            && stack.resources().any(|(_, entry)| renders_bundle(entry))
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        _state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        info!("Adding the sandbox bundle bucket");
        stack
            .resources
            .entry(SANDBOX_BUNDLE_STORAGE_ID.to_string())
            .or_insert_with(|| ResourceEntry {
                config: Resource::new(Storage::new(SANDBOX_BUNDLE_STORAGE_ID.to_string()).build()),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                display_name: Some("Sandbox image bundles".to_string()),
                description: Some(
                    "Holds the bundles this deployment's sandbox images are built from."
                        .to_string(),
                ),
                remote_access: false,
                enabled_when: None,
            });

        let storage = ResourceRef::new(Storage::RESOURCE_TYPE, SANDBOX_BUNDLE_STORAGE_ID);
        for (_, entry) in stack.resources.iter_mut() {
            if renders_bundle(entry) && !entry.dependencies.contains(&storage) {
                entry.dependencies.push(storage.clone());
            }
        }

        let has_management = stack
            .resources
            .values()
            .any(|entry| entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE);
        let management = match &stack.permissions.management {
            // The deployer authored management's grants in full.
            ManagementPermissions::Override(_) => None,
            ManagementPermissions::Extend(profile) => Some(profile.clone()),
            ManagementPermissions::Auto => Some(PermissionProfile::new()),
        };
        if let Some(mut profile) = management.filter(|_| has_management) {
            let grants = profile
                .0
                .entry(SANDBOX_BUNDLE_STORAGE_ID.to_string())
                .or_default();
            if !grants.iter().any(|set| set.id() == "storage/data-write") {
                grants.push(PermissionSetReference::from_name("storage/data-write"));
            }
            stack.permissions.management = ManagementPermissions::Extend(profile);
        }
        Ok(stack)
    }
}

/// A Live sandbox released as a container image rather than a prebuilt `s3://` bundle.
fn renders_bundle(entry: &ResourceEntry) -> bool {
    entry.lifecycle == ResourceLifecycle::Live
        && entry.config.downcast_ref::<Sandbox>().is_some_and(|sandbox| {
            matches!(&sandbox.code, SandboxCode::Image { image } if !image.starts_with("s3://"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compatibility::FrozenResourcesUnchangedCheck;
    use crate::runner::PreflightRunner;
    use crate::StackCompatibilityCheck;
    use alien_core::sandbox_setup_inputs::{
        aws_sandbox_renders_bundle, comparable_aws_sandbox_setup_inputs,
    };
    use alien_core::{
        AwsManagementConfig, EnvironmentVariablesSnapshot, ExternalBindings, ManagementConfig,
        SandboxEgress, SandboxLifecyclePolicy, StackResourceState, StackSettings,
    };

    const RELEASED: &str = "123456789012.dkr.ecr.us-east-1.amazonaws.com/acme/agents@sha256:\
                            aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const RELEASED_NEXT: &str = "123456789012.dkr.ecr.us-east-1.amazonaws.com/acme/agents@sha256:\
                                 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn release(image: &str, published_bundle: Option<&str>) -> Stack {
        let mut sandbox = Sandbox::new("agents".to_string())
            .code(SandboxCode::Image {
                image: image.to_string(),
            })
            .egress(SandboxEgress::Allow)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();
        sandbox.published_bundle = published_bundle.map(str::to_string);
        Stack::new("app".to_string())
            .add(sandbox, ResourceLifecycle::Live)
            .build()
    }

    fn config() -> DeploymentConfig {
        DeploymentConfig {
            gcp_federation_scope: None,
            deployment_name: None,
            stack_settings: StackSettings::default(),
            management_config: Some(ManagementConfig::Aws(AwsManagementConfig {
                managing_role_arn: "arn:aws:iam::210987654321:role/manager".to_string(),
            })),
            environment_variables: EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: "empty".to_string(),
                created_at: "1970-01-01T00:00:00Z".to_string(),
            },
            stored_secret_input_ids: None,
            input_values: Default::default(),
            allow_frozen_changes: false,
            setup_run: false,
            compute_backend: None,
            external_bindings: ExternalBindings::default(),
            base_platform: None,
            label_domain: None,
            observe_label_selector: None,
            observe_all_namespaces: false,
            public_endpoints: None,
            domain_metadata: None,
            monitoring: None,
            manager_url: None,
            deployment_token: None,
            native_image_host: None,
            volume_restores: Vec::new(),
        }
    }

    async fn prepared(stack: Stack, state: &StackState) -> Stack {
        PreflightRunner::new()
            .apply_mutations(stack, state, &config())
            .await
            .expect("the release prepares")
    }

    /// The state a deployment has once `prepared` is installed.
    fn installed(prepared: &Stack) -> StackState {
        let mut state = StackState::new(Platform::Aws);
        for (id, entry) in &prepared.resources {
            state.resources.insert(
                id.clone(),
                StackResourceState::new_pending(
                    entry.config.resource_type().to_string(),
                    entry.config.clone(),
                    Some(entry.lifecycle),
                    entry.dependencies.clone(),
                ),
            );
        }
        state
    }

    async fn frozen_unchanged(installed: &Stack, target: &Stack) -> bool {
        FrozenResourcesUnchangedCheck {
            platform: Platform::Aws,
        }
        .check(installed, target)
        .await
        .expect("the check runs")
        .success
    }

    fn sandbox_setup_inputs(stack: &Stack) -> Vec<(&'static str, serde_json::Value)> {
        let entry = &stack.resources["agents"];
        comparable_aws_sandbox_setup_inputs(
            stack,
            entry.config.downcast_ref::<Sandbox>().expect("a sandbox"),
            entry.lifecycle,
        )
    }

    /// A new setup creates the bucket and grants management its writes. An update of that
    /// deployment keeps both, so it needs no setup.
    #[tokio::test]
    async fn a_new_setup_adds_the_bundle_storage_and_updates_keep_it() {
        let setup = prepared(release(RELEASED, None), &StackState::new(Platform::Aws)).await;
        let storage = &setup.resources[SANDBOX_BUNDLE_STORAGE_ID];
        assert_eq!(storage.lifecycle, ResourceLifecycle::Frozen);
        assert!(aws_sandbox_renders_bundle(&setup, "agents"));
        let ManagementPermissions::Extend(management) = &setup.permissions.management else {
            panic!("management keeps its automatic grants and gains the bucket's");
        };
        assert_eq!(
            management.0[SANDBOX_BUNDLE_STORAGE_ID]
                .iter()
                .map(|set| set.id())
                .collect::<Vec<_>>(),
            vec!["storage/data-write"]
        );

        let update = prepared(release(RELEASED_NEXT, None), &installed(&setup)).await;
        assert!(aws_sandbox_renders_bundle(&update, "agents"));
        assert!(frozen_unchanged(&setup, &update).await);
        assert_eq!(sandbox_setup_inputs(&setup), sandbox_setup_inputs(&update));
    }

    /// A deployment set up when its releases named a published bundle has no bundle Storage.
    /// A release that moves to a container image, with the published bundle beside it, deploys
    /// without setup and keeps building from the store that setup granted. Running setup again adds
    /// the Storage and moves the sandbox onto it.
    #[tokio::test]
    async fn an_installed_setup_without_it_keeps_its_published_bundles_until_setup_runs() {
        let old_setup = prepared(
            release("s3://publisher-{region}/sandbox-bundle/v1/bundle.zip", None),
            &StackState::new(Platform::Aws),
        )
        .await;
        assert!(!old_setup.resources.contains_key(SANDBOX_BUNDLE_STORAGE_ID));

        let new_release = release(
            RELEASED,
            Some("s3://publisher-{region}/sandbox-bundle/v2/bundle.zip"),
        );
        let update = prepared(new_release.clone(), &installed(&old_setup)).await;
        assert!(!update.resources.contains_key(SANDBOX_BUNDLE_STORAGE_ID));
        assert!(!aws_sandbox_renders_bundle(&update, "agents"));
        assert!(frozen_unchanged(&old_setup, &update).await);
        assert_eq!(
            sandbox_setup_inputs(&old_setup),
            sandbox_setup_inputs(&update),
            "the build role this setup rendered still fits"
        );

        let rerun = prepared(new_release, &StackState::new(Platform::Aws)).await;
        assert!(aws_sandbox_renders_bundle(&rerun, "agents"));
        assert!(!frozen_unchanged(&old_setup, &rerun).await);
    }

    /// A Frozen sandbox is built by setup from its published bundle and needs no bucket.
    #[tokio::test]
    async fn a_frozen_or_published_sandbox_gets_no_bundle_storage() {
        let mut frozen = release(RELEASED, None);
        frozen.resources["agents"].lifecycle = ResourceLifecycle::Frozen;
        for stack in [
            frozen,
            release("s3://publisher-{region}/sandbox-bundle/v1/bundle.zip", None),
        ] {
            let prepared = prepared(stack, &StackState::new(Platform::Aws)).await;
            assert!(!prepared.resources.contains_key(SANDBOX_BUNDLE_STORAGE_ID));
        }
    }
}
