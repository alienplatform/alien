//! Give a deployment's Live AWS sandboxes a bucket of its own to build their images from.

use crate::error::{ErrorData, Result};
use crate::StackMutation;
use alien_core::permissions::{ManagementPermissions, PermissionProfile, PermissionSetReference};
use alien_core::sandbox_build_role::SANDBOX_BUNDLE_STORAGE_ID;
use alien_core::{
    DeploymentConfig, Platform, RemoteStackManagement, Resource, ResourceEntry, ResourceLifecycle,
    ResourceRef, Sandbox, SandboxCode, Stack, StackState, Storage,
};
use alien_error::AlienError;
use async_trait::async_trait;
use tracing::info;

/// The permission sets that can change other buckets' policies: `storage/provision` granted
/// stack-wide, and `email/provision` at any scope, since its bucket-policy grant always covers
/// every bucket of the stack.
const STACK_WIDE_BUCKET_POLICY_SET: &str = "storage/provision";
const ANY_SCOPE_BUCKET_POLICY_SET: &str = "email/provision";

/// Denies changing the bundle bucket's policy, for a workload holding one of those.
const PROTECT_BUNDLE_POLICY: &str = "sandbox/protect-bundle-policy";

/// Lets management write and delete bundles in the bundle Storage, and in no other bucket.
const SANDBOX_BUNDLE_WRITE: &str = "sandbox/bundle-write";

/// Adds the Storage a Live AWS sandbox's controller renders its image-build bundle into.
///
/// `CreateMicrovmImage` builds from a zip in S3 in the image's own region. A Live sandbox released
/// as a container image has no zip, so its controller writes one, `FROM` the release's image, into
/// this Storage, which every such sandbox depends on. Management may write it (`sandbox/bundle-write`)
/// and each sandbox's build role may read it (`storage/data-read`).
///
/// The Storage is Frozen, created by setup, so management never creates or deletes a bucket. An
/// installed deployment whose setup predates it would need setup to add it, so a runtime update of
/// such a deployment leaves it out, and its sandboxes keep building from the bundle a publisher put
/// in the store their setup granted. The next setup adds it: setup prepares its stack from empty
/// state, and later updates find it installed.
///
/// The controller writes bundles as the management role, the one principal the bucket lets write,
/// so a new stack with no remote stack management cannot render bundles and is refused here.
pub struct SandboxBundleStorageMutation;

#[async_trait]
impl StackMutation for SandboxBundleStorageMutation {
    fn description(&self) -> &'static str {
        "Add the bucket Live AWS sandboxes build their images from"
    }

    fn should_run(&self, stack: &Stack, state: &StackState, _config: &DeploymentConfig) -> bool {
        // An installed one stays even once no sandbox renders into it: dropping a Frozen resource
        // would need setup, and an empty bucket costs nothing.
        state.platform == Platform::Aws
            && (state.resources.contains_key(SANDBOX_BUNDLE_STORAGE_ID)
                || (state.resources.is_empty()
                    && stack.resources().any(|(_, entry)| renders_bundle(entry))))
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        _state: &StackState,
        _config: &DeploymentConfig,
    ) -> Result<Stack> {
        let has_management = stack
            .resources
            .values()
            .any(|entry| entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE);
        if !has_management {
            // A publisher's bundle beside the image still builds, from the store setup grants.
            let Some(sandbox) = stack
                .resources()
                .find(|(_, entry)| renders_bundle(entry) && !published(entry))
                .map(|(id, _)| id.clone())
            else {
                return Ok(stack);
            };
            return Err(AlienError::new(ErrorData::StackMutationFailed {
                mutation_name: self.description().to_string(),
                message: "a Live AWS sandbox released as a container image has its image-build \
                          bundle written by the deployment's management role, and this deployment \
                          has no remote stack management; deploy it with a management \
                          configuration, or make the sandbox Frozen with an s3:// bundle"
                    .to_string(),
                resource_id: Some(sandbox),
            }));
        }

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

        let management = match &stack.permissions.management {
            // The deployer authored management's grants in full.
            ManagementPermissions::Override(_) => None,
            ManagementPermissions::Extend(profile) => Some(profile.clone()),
            ManagementPermissions::Auto => Some(PermissionProfile::new()),
        };
        // The bucket's policy lets only management write bundles. A workload that can manage the
        // stack's bucket policies gets a Deny on this bucket's, so it cannot lift that guard.
        for profile in stack.permissions.profiles.values_mut() {
            let reaches_bucket_policies = profile.0.iter().any(|(scope, sets)| {
                sets.iter().any(|set| {
                    set.id() == ANY_SCOPE_BUCKET_POLICY_SET
                        || (scope == "*" && set.id() == STACK_WIDE_BUCKET_POLICY_SET)
                })
            });
            if !reaches_bucket_policies {
                continue;
            }
            let sets = profile.0.entry("*".to_string()).or_default();
            if !sets.iter().any(|set| set.id() == PROTECT_BUNDLE_POLICY) {
                sets.push(PermissionSetReference::from_name(PROTECT_BUNDLE_POLICY));
            }
        }

        // Stack-wide, because an automatically managed AWS setup cannot take a resource-scoped
        // management grant; the set itself is bound to this bucket's name.
        if let Some(mut profile) = management {
            let grants = profile.0.entry("*".to_string()).or_default();
            if !grants.iter().any(|set| set.id() == SANDBOX_BUNDLE_WRITE) {
                grants.push(PermissionSetReference::from_name(SANDBOX_BUNDLE_WRITE));
            }
            stack.permissions.management = ManagementPermissions::Extend(profile);
        }
        Ok(stack)
    }
}

/// A sandbox whose release also carries a publisher's bundle in `code.image`.
fn published(entry: &ResourceEntry) -> bool {
    entry.config.downcast_ref::<Sandbox>().is_some_and(|sandbox| {
        matches!(&sandbox.code, SandboxCode::Image { image, .. } if image.starts_with("s3://"))
    })
}

/// A Live sandbox released as a container image, alone or beside the bundle a publisher built
/// from it, rather than as a prebuilt `s3://` bundle only.
fn renders_bundle(entry: &ResourceEntry) -> bool {
    entry.lifecycle == ResourceLifecycle::Live
        && entry
            .config
            .downcast_ref::<Sandbox>()
            .is_some_and(|sandbox| {
                matches!(&sandbox.code, SandboxCode::Image { image, bundled_from }
                if bundled_from.is_some() || !image.starts_with("s3://"))
            })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compatibility::FrozenResourcesUnchangedCheck;
    use crate::runner::PreflightRunner;
    use crate::{PreflightRegistry, StackCompatibilityCheck};
    use alien_core::permissions::PermissionProfile;
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

    /// A release of `image`, with the bundle a publisher built from it in `code.image` when
    /// `published_bundle` is given.
    fn release(image: &str, published_bundle: Option<&str>) -> Stack {
        let code = match published_bundle {
            Some(bundle) => SandboxCode::Image {
                image: bundle.to_string(),
                bundled_from: Some(image.to_string()),
            },
            None => SandboxCode::Image {
                image: image.to_string(),
                bundled_from: None,
            },
        };
        let sandbox = Sandbox::new("agents".to_string())
            .code(code)
            .egress(SandboxEgress::Allow)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();
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
            deletion_policy: Default::default(),
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
        assert!(management.0["*"]
            .iter()
            .any(|set| set.id() == SANDBOX_BUNDLE_WRITE));
        assert!(
            !management.0.contains_key(SANDBOX_BUNDLE_STORAGE_ID),
            "an automatically managed AWS setup refuses resource-scoped management grants"
        );

        let update = prepared(release(RELEASED_NEXT, None), &installed(&setup)).await;
        assert!(aws_sandbox_renders_bundle(&update, "agents"));
        assert!(frozen_unchanged(&setup, &update).await);
        assert_eq!(sandbox_setup_inputs(&setup), sandbox_setup_inputs(&update));
    }

    /// A deployment set up when its releases named a published bundle has no bundle Storage.
    /// A release that still names one, with the image it was built from in `bundledFrom`, deploys
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

    /// An automatically managed AWS setup refuses any resource-scoped management grant, so the
    /// bundle Storage must prepare under it too.
    #[tokio::test]
    async fn an_automatically_managed_setup_prepares_the_bundle_storage() {
        let mut automatic = config();
        automatic.stack_settings.setup_management = alien_core::SetupManagement::Automatic;
        let setup = PreflightRunner::new()
            .apply_mutations(
                release(RELEASED, None),
                &StackState::new(Platform::Aws),
                &automatic,
            )
            .await
            .expect("an automatic setup prepares the release");
        assert!(aws_sandbox_renders_bundle(&setup, "agents"));
    }

    /// A workload that can manage the stack's bucket policies is kept off the bundle bucket's;
    /// one that cannot gets nothing extra.
    #[tokio::test]
    async fn a_workload_that_can_change_bucket_policies_is_kept_off_the_bundle_bucket() {
        let mut stack = release(RELEASED, None);
        stack.permissions.profiles.insert(
            "execution".to_string(),
            PermissionProfile::new().resource("*", ["storage/data-write", "storage/provision"]),
        );
        stack.permissions.profiles.insert(
            "reader".to_string(),
            PermissionProfile::new().resource("*", ["storage/data-read"]),
        );
        // Email's bucket-policy grant covers every bucket of the stack even when granted on one
        // Email resource.
        stack.permissions.profiles.insert(
            "mailer".to_string(),
            PermissionProfile::new().resource("mail", ["email/provision"]),
        );
        // Storage's is bound to the one bucket it is granted on.
        stack.permissions.profiles.insert(
            "uploads-admin".to_string(),
            PermissionProfile::new().resource("uploads", ["storage/provision"]),
        );
        let setup = prepared(stack, &StackState::new(Platform::Aws)).await;
        let ids = |profile: &str| {
            setup.permissions.profiles[profile]
                .0
                .get("*")
                .map(|sets| sets.iter().map(|set| set.id().to_string()).collect())
                .unwrap_or_else(Vec::new)
        };
        assert!(ids("execution").contains(&PROTECT_BUNDLE_POLICY.to_string()));
        assert!(ids("mailer").contains(&PROTECT_BUNDLE_POLICY.to_string()));
        assert!(!ids("reader").contains(&PROTECT_BUNDLE_POLICY.to_string()));
        assert!(!ids("uploads-admin").contains(&PROTECT_BUNDLE_POLICY.to_string()));
    }

    /// An install set up before the bundle bucket existed, by a release whose workload can manage
    /// bucket policies, and the release that moves its sandbox to a container image.
    async fn pre_change_install_and_update() -> (Stack, Stack) {
        let with_policy_workload = |mut stack: Stack| {
            stack.permissions.profiles.insert(
                "execution".to_string(),
                PermissionProfile::new().resource("*", ["storage/data-write", "storage/provision"]),
            );
            stack
        };
        let mut before_this_change = PreflightRegistry::with_built_ins();
        before_this_change.mutations.retain(|mutation| {
            mutation.description() != SandboxBundleStorageMutation.description()
        });
        let pre_change = PreflightRunner::with_registry(before_this_change)
            .apply_mutations(
                with_policy_workload(release(
                    "s3://publisher-{region}/sandbox-bundle/v1/bundle.zip",
                    None,
                )),
                &StackState::new(Platform::Aws),
                &config(),
            )
            .await
            .expect("the earlier release prepares");
        let update = prepared(
            with_policy_workload(release(
                RELEASED,
                Some("s3://publisher-{region}/sandbox-bundle/v2/bundle.zip"),
            )),
            &installed(&pre_change),
        )
        .await;
        (pre_change, update)
    }

    /// Without the bundle bucket, the workload's ServiceAccount compiles exactly as it did before
    /// this change: no guard is added for a bucket the stack does not hold.
    #[tokio::test]
    async fn without_the_bundle_bucket_the_workload_service_account_compiles_unchanged() {
        let (pre_change, update) = pre_change_install_and_update().await;
        assert!(!update.resources.contains_key(SANDBOX_BUNDLE_STORAGE_ID));
        let service_account = |stack: &Stack| {
            serde_json::to_vec(&stack.resources["execution-sa"]).expect("it serializes")
        };
        assert_eq!(service_account(&pre_change), service_account(&update));
    }

    /// The same update passes the Frozen-resources check against the pre-change install, so it
    /// deploys without setup.
    #[tokio::test]
    async fn a_pre_change_install_with_a_bucket_policy_workload_passes_the_frozen_check() {
        let (pre_change, update) = pre_change_install_and_update().await;
        assert!(frozen_unchanged(&pre_change, &update).await);
    }

    /// Only the management role may write the bundle bucket, so a new stack without one cannot
    /// render bundles: a sandbox released as an image alone is refused before anything is
    /// created, and one released with a publisher's bundle builds from that bundle.
    #[tokio::test]
    async fn without_remote_stack_management_only_a_published_bundle_builds() {
        let mut unmanaged = config();
        unmanaged.management_config = None;
        let prepare = |stack: Stack| {
            let unmanaged = unmanaged.clone();
            async move {
                PreflightRunner::new()
                    .apply_mutations(stack, &StackState::new(Platform::Aws), &unmanaged)
                    .await
            }
        };

        let error = prepare(release(RELEASED, None))
            .await
            .expect_err("nothing could write the bundle");
        assert!(
            error.to_string().contains("no remote stack management"),
            "unexpected error: {error}"
        );

        let published = prepare(release(
            RELEASED,
            Some("s3://publisher-{region}/sandbox-bundle/v1/bundle.zip"),
        ))
        .await
        .expect("a published bundle builds without the bucket");
        assert!(!published.resources.contains_key(SANDBOX_BUNDLE_STORAGE_ID));
        assert!(!aws_sandbox_renders_bundle(&published, "agents"));
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
