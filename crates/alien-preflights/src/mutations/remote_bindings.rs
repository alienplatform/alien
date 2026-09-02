//! Adds the one setup-owned Remote Bindings identity required by opted-in resources.

use crate::{error::ErrorData, error::Result, StackMutation};
use alien_core::{
    links_of, permissions::PermissionSetReference, Ai, DeploymentConfig, Key, Platform,
    RemoteBindingGrant, RemoteBindings, ResourceEntry, ResourceLifecycle, Sandbox, ServiceAccount,
    Stack, StackState,
};
use alien_error::AlienError;
use alien_permissions::{get_permission_set, permission_set_reaches_a_microvm_session};
use async_trait::async_trait;

pub const REMOTE_BINDINGS_ID: &str = "access";

pub struct RemoteBindingsMutation;

#[async_trait]
impl StackMutation for RemoteBindingsMutation {
    fn description(&self) -> &'static str {
        "Add the setup-owned Remote Bindings identity"
    }

    fn should_run(
        &self,
        stack: &Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> bool {
        let platform = config.base_platform.unwrap_or(stack_state.platform);
        matches!(
            platform,
            Platform::Aws | Platform::Gcp | Platform::Azure | Platform::Test
        ) && stack
            .resources
            .values()
            .any(ResourceEntry::has_remote_bindings)
            && !stack
                .resources
                .values()
                .any(|entry| entry.config.resource_type() == RemoteBindings::RESOURCE_TYPE)
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<Stack> {
        let platform = config.base_platform.unwrap_or(stack_state.platform);
        validate_remote_bindings_cover_the_platform(&stack, platform, self.description())?;
        validate_isolated_remote_resource(&stack, self.description())?;
        validate_remote_sandboxes_are_deliverable(&stack, self.description())?;
        validate_remote_sandboxes_are_single_tenant(&stack, self.description())?;

        if let Some(existing) = stack.resources.get(REMOTE_BINDINGS_ID) {
            return Err(AlienError::new(ErrorData::StackMutationFailed {
                mutation_name: self.description().to_string(),
                message: format!(
                    "resource ID '{REMOTE_BINDINGS_ID}' is reserved for application access, but is already used by resource type '{}'",
                    existing.config.resource_type()
                ),
                resource_id: Some(REMOTE_BINDINGS_ID.to_string()),
            }));
        }

        let mut grants = stack
            .resources
            .iter()
            .filter_map(|(resource_id, entry)| {
                alien_core::remote_bindings::remote_binding_for_entry(entry).map(|definition| {
                    RemoteBindingGrant {
                        resource_id: resource_id.clone(),
                        permission_set: definition.permission_set.to_string(),
                        revision: definition.revision,
                    }
                })
            })
            .collect::<Vec<_>>();
        grants.sort_by(|left, right| left.resource_id.cmp(&right.resource_id));
        stack.resources.insert(
            REMOTE_BINDINGS_ID.to_string(),
            ResourceEntry {
                enabled_when: None,
                config: alien_core::Resource::new(
                    RemoteBindings::new(REMOTE_BINDINGS_ID.to_string())
                        .grants(grants)
                        .build(),
                ),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
            },
        );
        Ok(stack)
    }
}

/// Refuses a remote binding whose permission set grants nothing on the deployment's platform.
///
/// The permission set is the whole grant, so an uncovered kind installs no role binding and its
/// generated permission docs carry a heading with no permissions under it — while the manager
/// still refuses to resolve it, days later and at the vendor rather than at deploy.
fn validate_remote_bindings_cover_the_platform(
    stack: &Stack,
    platform: Platform,
    mutation_name: &str,
) -> Result<()> {
    // `Platform::Test` is not deployable and no permission set describes it.
    if platform == Platform::Test {
        return Ok(());
    }
    for (resource_id, entry) in &stack.resources {
        let Some(definition) = alien_core::remote_bindings::remote_binding_for_entry(entry) else {
            continue;
        };
        if alien_permissions::permission_set_covers_platform(definition.permission_set, platform) {
            continue;
        }
        return Err(AlienError::new(ErrorData::StackMutationFailed {
            mutation_name: mutation_name.to_string(),
            message: format!(
                "a remotely published {} is not supported on {platform}: permission set '{}' \
                 grants nothing there",
                entry.config.resource_type(),
                definition.permission_set
            ),
            resource_id: Some(resource_id.clone()),
        }));
    }
    Ok(())
}

/// Refuses a deployment that pairs an isolated remote kind with any other published resource.
///
/// Every kind's grants attach to the one shared `-access` role, and the credential lease is a
/// plain AssumeRole with no session policy, so resolving any binding hands the caller the whole
/// role's authority. Isolation is the only thing bounding what one resolve is worth.
fn validate_isolated_remote_resource(stack: &Stack, mutation_name: &str) -> Result<()> {
    let remote_resources = stack
        .resources
        .iter()
        .filter(|(_, entry)| entry.has_remote_bindings())
        .collect::<Vec<_>>();
    let isolated_resources = remote_resources
        .iter()
        .filter(|(_, entry)| {
            matches!(
                entry.config.resource_type(),
                resource_type
                    if resource_type == Key::RESOURCE_TYPE
                        || resource_type == Ai::RESOURCE_TYPE
                        || resource_type == Sandbox::RESOURCE_TYPE
            )
        })
        .collect::<Vec<_>>();

    if isolated_resources.is_empty()
        || (isolated_resources.len() == 1 && remote_resources.len() == 1)
    {
        return Ok(());
    }

    let (resource_id, entry) = isolated_resources[0];

    Err(AlienError::new(ErrorData::StackMutationFailed {
        mutation_name: mutation_name.to_string(),
        message: format!(
            "a remotely published {} must be the deployment's only remoteAccess resource",
            entry.config.resource_type()
        ),
        resource_id: Some((*resource_id).clone()),
    }))
}

/// Refuses a remotely published sandbox declaring a policy the remote grant cannot carry.
///
/// The declaration has to fail here rather than install a role carrying arbitrary code execution
/// for a binding the manager will then refuse to resolve.
/// `remote_binding_undeliverable_reason` holds the reasons and is the same source both setup
/// emitters read, so each case reaches the user in its own terms rather than under the first one's.
fn validate_remote_sandboxes_are_deliverable(stack: &Stack, mutation_name: &str) -> Result<()> {
    for (resource_id, entry) in &stack.resources {
        if !entry.has_remote_bindings() {
            continue;
        }
        let Some(reason) = alien_core::remote_bindings::remote_binding_undeliverable_reason(entry)
        else {
            continue;
        };
        return Err(AlienError::new(ErrorData::StackMutationFailed {
            mutation_name: mutation_name.to_string(),
            message: reason.to_string(),
            resource_id: Some(resource_id.clone()),
        }));
    }
    Ok(())
}

/// Refuses a remotely published sandbox that the deployment's own workloads can also reach.
///
/// One MicroVM image serves every session of a sandbox and AWS scopes a token mint no finer than
/// the image, so a remote caller holding raw credentials can read, suspend, terminate or mint into
/// sessions the customer's own compute started. Single tenancy is the only containment available.
fn validate_remote_sandboxes_are_single_tenant(stack: &Stack, mutation_name: &str) -> Result<()> {
    for (resource_id, entry) in &stack.resources {
        if !entry.has_remote_bindings() || entry.config.resource_type() != Sandbox::RESOURCE_TYPE {
            continue;
        }
        let Some(reach) = in_cloud_reach_to(stack, resource_id) else {
            continue;
        };
        return Err(AlienError::new(ErrorData::StackMutationFailed {
            mutation_name: mutation_name.to_string(),
            message: format!(
                "a remotely published sandbox must be reachable by nobody else in the deployment, \
                 but {reach}"
            ),
            resource_id: Some(resource_id.clone()),
        }));
    }
    Ok(())
}

/// How the deployment's own workloads can reach `sandbox_id`, if any can.
///
/// Three routes, none of which the vendor's own `-access` grant travels: a compute link, a
/// permission profile, and a stack permission set on a user-declared ServiceAccount. Reach is
/// decided by the resolved set's session-reaching verbs, never by a set id — an inline set an
/// author names anything defeats a prefix test. This scan is shaped for the AWS routes; what
/// keeps it honest for GCP is the platform gate refusing a GCP remote sandbox before this answer
/// is used, not anything this function checks.
fn in_cloud_reach_to(stack: &Stack, sandbox_id: &str) -> Option<String> {
    let linked_by = stack.resources().find(|(_, entry)| {
        links_of(&entry.config)
            .iter()
            .any(|link| link.id() == sandbox_id)
    });
    if let Some((consumer_id, _)) = linked_by {
        return Some(format!("resource '{consumer_id}' links it"));
    }

    let by_profile = stack
        .permissions
        .profiles
        .iter()
        .find_map(|(profile_name, profile)| {
            profile
                .0
                .iter()
                .any(|(target, permission_sets)| {
                    (target == sandbox_id || target == "*")
                        && permission_sets.iter().any(reference_reaches_a_session)
                })
                .then(|| format!("permission profile '{profile_name}' grants access to it"))
        });
    if by_profile.is_some() {
        return by_profile;
    }

    stack.resources().find_map(|(id, entry)| {
        entry
            .config
            .downcast_ref::<ServiceAccount>()
            .filter(|account| {
                account
                    .stack_permission_sets
                    .iter()
                    .any(permission_set_reaches_a_microvm_session)
            })
            .map(|_| format!("service account '{id}' can start sessions in it"))
    })
}

/// Whether a profile's permission set, once resolved, reaches a session.
///
/// A named reference is looked up in the built-in registry rather than trusted by name; an
/// inline set is inspected directly. An unresolvable name is treated as no reach — the manager
/// rejects an unknown set before it grants anything.
fn reference_reaches_a_session(reference: &PermissionSetReference) -> bool {
    match reference {
        PermissionSetReference::Inline(set) => permission_set_reaches_a_microvm_session(set),
        PermissionSetReference::Name(name) => {
            get_permission_set(name).is_some_and(permission_set_reaches_a_microvm_session)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{
        permissions::PermissionProfile, Ai, EnvironmentVariablesSnapshot, ExternalBindings, Key,
        ManagementConfig, Sandbox, SandboxCode, SandboxEgress, SandboxSessionPolicy, StackSettings,
        Storage, Worker, WorkerCode,
    };

    fn config() -> DeploymentConfig {
        DeploymentConfig {
            deployment_name: None,
            stack_settings: StackSettings::default(),
            management_config: Some(ManagementConfig::Kubernetes),
            environment_variables: EnvironmentVariablesSnapshot {
                variables: Vec::new(),
                hash: "empty".to_string(),
                created_at: "1970-01-01T00:00:00Z".to_string(),
            },
            input_values: Default::default(),
            allow_frozen_changes: false,
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
        }
    }

    #[tokio::test]
    async fn one_opted_in_resource_adds_one_shared_identity() {
        let stack = Stack::new("byo-bucket".to_string())
            .add_with_remote_access(
                Storage::new("exports".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let state = StackState::new(Platform::Test);
        let mutation = RemoteBindingsMutation;

        assert!(mutation.should_run(&stack, &state, &config()));
        let mutated = mutation.mutate(stack, &state, &config()).await.unwrap();
        assert!(mutated.resources.contains_key(REMOTE_BINDINGS_ID));
        assert_eq!(
            mutated
                .resources
                .values()
                .filter(|entry| entry.config.resource_type() == RemoteBindings::RESOURCE_TYPE)
                .count(),
            1
        );
        let bindings = mutated
            .resources
            .get(REMOTE_BINDINGS_ID)
            .and_then(|entry| entry.config.downcast_ref::<RemoteBindings>())
            .expect("Remote Bindings config");
        assert_eq!(bindings.grants.len(), 1);
        assert_eq!(bindings.grants[0].resource_id, "exports");
        assert_eq!(
            bindings.grants[0].permission_set,
            "storage/remote-data-write"
        );
    }

    fn published(resource: impl alien_core::ResourceDefinition) -> Stack {
        Stack::new("byo".to_string())
            .add_with_remote_access(resource, ResourceLifecycle::Frozen)
            .build()
    }

    fn sandbox(egress: SandboxEgress) -> Sandbox {
        Sandbox::new("agents".to_string())
            .code(SandboxCode::Image {
                image: "ubuntu:24.04".to_string(),
            })
            .egress(egress)
            .session(SandboxSessionPolicy {
                max_lifetime_seconds: None,
                idle_suspend_seconds: None,
            })
            .build()
    }

    /// A Sandbox declared the way Remote Bindings requires — `Frozen` with `remoteAccess` — is
    /// what the remaining remote-binding paths assume exists. Lifecycle is per declaration, so
    /// nothing about the type blocks it; this pins that the grant reaches the shared identity.
    #[tokio::test]
    async fn a_frozen_remote_sandbox_is_granted_the_remote_execute_set() {
        let stack = Stack::new("byo-sandbox".to_string())
            .add_with_remote_access(sandbox(SandboxEgress::Allow), ResourceLifecycle::Frozen)
            .build();
        let state = StackState::new(Platform::Aws);

        let entry = stack.resources.get("agents").expect("declared sandbox");
        assert!(entry.remote_access, "the declaration opted in");
        assert_eq!(entry.lifecycle, ResourceLifecycle::Frozen);
        let definition = alien_core::remote_bindings::remote_binding_for_entry(entry)
            .expect("a Frozen remoteAccess Sandbox is a Remote Bindings resource");
        assert_eq!(
            definition.kind,
            alien_core::remote_bindings::RemoteBindingKind::Sandbox
        );

        assert!(RemoteBindingsMutation.should_run(&stack, &state, &config()));
        let mutated = RemoteBindingsMutation
            .mutate(stack, &state, &config())
            .await
            .expect("an open-egress sandbox alone in its deployment is a valid remote binding");
        let bindings = mutated
            .resources
            .get(REMOTE_BINDINGS_ID)
            .and_then(|entry| entry.config.downcast_ref::<RemoteBindings>())
            .expect("Remote Bindings config");
        assert_eq!(bindings.grants.len(), 1);
        assert_eq!(bindings.grants[0].resource_id, "agents");
        assert_eq!(bindings.grants[0].permission_set, "sandbox/remote-execute");
        assert_eq!(bindings.grants[0].revision, definition.revision);
    }

    /// A Live sandbox is scaffolded by setup — its build role and connector — and its binding is
    /// published by the runtime controller once the image is active, so the remote grant attaches
    /// exactly as it does for a Frozen one.
    #[tokio::test]
    async fn a_live_remote_sandbox_is_granted_the_remote_execute_set() {
        let stack = Stack::new("byo-sandbox".to_string())
            .add_with_remote_access(sandbox(SandboxEgress::Allow), ResourceLifecycle::Live)
            .build();
        let state = StackState::new(Platform::Aws);

        assert!(RemoteBindingsMutation.should_run(&stack, &state, &config()));
        let mutated = RemoteBindingsMutation
            .mutate(stack, &state, &config())
            .await
            .expect("a Live open-egress sandbox is a valid remote binding");
        let bindings = mutated
            .resources
            .get(REMOTE_BINDINGS_ID)
            .and_then(|entry| entry.config.downcast_ref::<RemoteBindings>())
            .expect("Remote Bindings config");
        assert_eq!(bindings.grants.len(), 1);
        assert_eq!(bindings.grants[0].resource_id, "agents");
        assert_eq!(bindings.grants[0].permission_set, "sandbox/remote-execute");
    }

    /// `sandbox/remote-execute` has an AWS block alone. Without this gate the deployment installs
    /// a GCP or Azure identity with no sandbox role binding, and the customer's security team
    /// approves a PERMISSIONS.md heading promising arbitrary code execution and listing nothing.
    #[tokio::test]
    async fn a_remote_sandbox_is_refused_where_its_permission_set_grants_nothing() {
        for platform in [Platform::Gcp, Platform::Azure] {
            let stack = Stack::new("byo-sandbox".to_string())
                .add_with_remote_access(sandbox(SandboxEgress::Allow), ResourceLifecycle::Frozen)
                .build();

            let error = RemoteBindingsMutation
                .mutate(stack, &StackState::new(platform), &config())
                .await
                .expect_err("a sandbox the permission set does not cover must not deploy");

            assert_eq!(error.code, "STACK_MUTATION_FAILED");
            let message = error.to_string();
            assert!(
                message.contains(&format!("is not supported on {platform}")),
                "the refusal must name the platform, got: {message}"
            );
            assert!(
                message.contains("sandbox/remote-execute"),
                "the refusal must name the permission set that decides it, got: {message}"
            );
        }
    }

    /// The gate reads `base_platform` where a deployment sets one, exactly as `should_run` does.
    #[tokio::test]
    async fn a_remote_sandbox_is_refused_on_the_base_platform_the_config_selects() {
        let stack = Stack::new("byo-sandbox".to_string())
            .add_with_remote_access(sandbox(SandboxEgress::Allow), ResourceLifecycle::Frozen)
            .build();
        let mut config = config();
        config.base_platform = Some(Platform::Gcp);

        let error = RemoteBindingsMutation
            .mutate(stack, &StackState::new(Platform::Kubernetes), &config)
            .await
            .expect_err("the base platform is the one the grants are emitted for");

        assert_eq!(error.code, "STACK_MUTATION_FAILED");
        assert!(error.to_string().contains("is not supported on gcp"));
    }

    /// A GCP remote sandbox's in-cloud reach is not something `in_cloud_reach_to` can see, so
    /// single tenancy would certify this stack. The platform gate runs first and is what keeps
    /// that unreachable.
    #[tokio::test]
    async fn a_gcp_remote_sandbox_is_refused_before_single_tenancy_can_certify_it() {
        let worker = Worker::new("processor".to_string())
            .permissions("execution".to_string())
            .code(WorkerCode::Image {
                image: "example.com/processor:latest".to_string(),
            })
            .build();
        let stack = Stack::new("application".to_string())
            .add_with_remote_access(sandbox(SandboxEgress::Allow), ResourceLifecycle::Frozen)
            .add(worker, ResourceLifecycle::Live)
            .build();

        assert!(
            in_cloud_reach_to(&stack, "agents").is_none(),
            "no link and no profile grant, so the single-tenancy scan finds nobody"
        );

        let error = RemoteBindingsMutation
            .mutate(stack, &StackState::new(Platform::Gcp), &config())
            .await
            .expect_err("a GCP remote sandbox must be refused whatever the reach scan says");

        assert_eq!(error.code, "STACK_MUTATION_FAILED");
        assert!(error.to_string().contains("is not supported on gcp"));
    }

    /// The gate is the permission set's own coverage, so the kinds that carry all three blocks
    /// must be untouched by it on every cloud.
    #[tokio::test]
    async fn the_other_remote_kinds_deploy_on_every_cloud_platform() {
        let cases: [(fn() -> Stack, &str, &str); 3] = [
            (
                || published(Storage::new("exports".to_string()).build()),
                "exports",
                "storage/remote-data-write",
            ),
            (
                || published(Key::new("customer-key".to_string()).build()),
                "customer-key",
                "key/remote-cryptography",
            ),
            (
                || published(Ai::new("models".to_string()).build()),
                "models",
                "ai/invoke",
            ),
        ];

        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            for (build_stack, resource_id, permission_set) in cases {
                let mutated = RemoteBindingsMutation
                    .mutate(build_stack(), &StackState::new(platform), &config())
                    .await
                    .unwrap_or_else(|error| {
                        panic!("{permission_set} covers {platform} and must deploy: {error}")
                    });
                let bindings = mutated
                    .resources
                    .get(REMOTE_BINDINGS_ID)
                    .and_then(|entry| entry.config.downcast_ref::<RemoteBindings>())
                    .expect("Remote Bindings config");

                assert_eq!(bindings.grants.len(), 1);
                assert_eq!(bindings.grants[0].resource_id, resource_id);
                assert_eq!(bindings.grants[0].permission_set, permission_set);
            }
        }
    }

    /// `sandbox/remote-execute` withholds `lambda:PassNetworkConnector`, so a session on an egress
    /// connector cannot be started remotely. The deployment has to fail here rather than install a
    /// role carrying arbitrary code execution for a binding the manager will then refuse.
    #[tokio::test]
    async fn a_remote_sandbox_that_restricts_egress_is_refused() {
        for egress in [
            SandboxEgress::Deny,
            SandboxEgress::AllowDomains {
                domains: vec!["registry.npmjs.org".to_string()],
            },
        ] {
            let stack = Stack::new("byo-sandbox".to_string())
                .add_with_remote_access(sandbox(egress), ResourceLifecycle::Frozen)
                .build();

            let error = RemoteBindingsMutation
                .mutate(stack, &StackState::new(Platform::Test), &config())
                .await
                .expect_err("a restricted-egress sandbox must not reach a deployed remote grant");

            assert_eq!(error.code, "STACK_MUTATION_FAILED");
            assert!(
                error.to_string().contains("must declare egress 'allow'"),
                "the refusal must name the declaration to change, got: {error}"
            );
        }
    }

    /// Resolving any binding yields credentials for the whole shared `-access` role, so a vendor
    /// granted bucket read on a stack like this would also receive arbitrary code execution.
    #[tokio::test]
    async fn a_remote_sandbox_rejects_another_published_resource() {
        let stack = Stack::new("application".to_string())
            .add_with_remote_access(sandbox(SandboxEgress::Allow), ResourceLifecycle::Frozen)
            .add_with_remote_access(
                Storage::new("exports".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();

        let error = RemoteBindingsMutation
            .mutate(stack, &StackState::new(Platform::Test), &config())
            .await
            .expect_err("a remote sandbox must reject another remotely published resource");

        assert_eq!(error.code, "STACK_MUTATION_FAILED");
        assert!(
            error.to_string().contains(
                "a remotely published sandbox must be the deployment's only remoteAccess resource"
            ),
            "the sandbox must be named as the resource forcing isolation, got: {error}"
        );
    }

    #[tokio::test]
    async fn a_remote_sandbox_allows_non_remote_application_resources() {
        let stack = Stack::new("application".to_string())
            .add_with_remote_access(sandbox(SandboxEgress::Allow), ResourceLifecycle::Frozen)
            .add(
                Storage::new("internal".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();

        let mutated = RemoteBindingsMutation
            .mutate(stack, &StackState::new(Platform::Test), &config())
            .await
            .expect("non-remote application resources are allowed beside a remote sandbox");
        let bindings = mutated
            .resources
            .get(REMOTE_BINDINGS_ID)
            .and_then(|entry| entry.config.downcast_ref::<RemoteBindings>())
            .expect("Remote Bindings config");

        assert_eq!(bindings.grants.len(), 1);
        assert_eq!(bindings.grants[0].resource_id, "agents");
        assert_eq!(bindings.grants[0].permission_set, "sandbox/remote-execute");
    }

    /// A link authors `sandbox/execute` on the customer's own workload, which starts sessions in
    /// the same MicroVM image the remote caller's credentials address.
    #[tokio::test]
    async fn a_remote_sandbox_linked_by_compute_is_refused() {
        let sandbox = sandbox(SandboxEgress::Allow);
        let worker = Worker::new("processor".to_string())
            .permissions("execution".to_string())
            .code(WorkerCode::Image {
                image: "example.com/processor:latest".to_string(),
            })
            .link(&sandbox)
            .build();
        let stack = Stack::new("application".to_string())
            .add_with_remote_access(sandbox, ResourceLifecycle::Frozen)
            .add(worker, ResourceLifecycle::Live)
            .build();

        let error = RemoteBindingsMutation
            .mutate(stack, &StackState::new(Platform::Test), &config())
            .await
            .expect_err("a linked sandbox is not single-tenant to the remote caller");

        assert_eq!(error.code, "STACK_MUTATION_FAILED");
        assert!(
            error.to_string().contains("resource 'processor' links it"),
            "the refusal must name the workload that shares the sandbox, got: {error}"
        );
    }

    /// `sandbox/management` never arrives through a link, so a profile-only grant is the case a
    /// link scan misses — and it is session lifecycle over the remote caller's own sessions.
    #[tokio::test]
    async fn a_remote_sandbox_granted_to_a_profile_is_refused() {
        let stack = Stack::new("application".to_string())
            .add_with_remote_access(sandbox(SandboxEgress::Allow), ResourceLifecycle::Frozen)
            .permission(
                "execution",
                PermissionProfile::new().resource("agents", ["sandbox/management"]),
            )
            .build();

        let error = RemoteBindingsMutation
            .mutate(stack, &StackState::new(Platform::Test), &config())
            .await
            .expect_err("an in-cloud management grant is not single-tenant to the remote caller");

        assert_eq!(error.code, "STACK_MUTATION_FAILED");
        assert!(
            error
                .to_string()
                .contains("permission profile 'execution' grants access to it"),
            "the refusal must name the profile that shares the sandbox, got: {error}"
        );
    }

    #[tokio::test]
    async fn a_remote_sandbox_allows_compute_that_does_not_reach_it() {
        let worker = Worker::new("processor".to_string())
            .permissions("execution".to_string())
            .code(WorkerCode::Image {
                image: "example.com/processor:latest".to_string(),
            })
            .build();
        let stack = Stack::new("application".to_string())
            .add_with_remote_access(sandbox(SandboxEgress::Allow), ResourceLifecycle::Frozen)
            .add(worker, ResourceLifecycle::Live)
            .permission(
                "execution",
                PermissionProfile::new().resource("exports", ["storage/data-write"]),
            )
            .build();

        let mutated = RemoteBindingsMutation
            .mutate(stack, &StackState::new(Platform::Test), &config())
            .await
            .expect("compute that never reaches the sandbox leaves it single-tenant");
        let bindings = mutated
            .resources
            .get(REMOTE_BINDINGS_ID)
            .and_then(|entry| entry.config.downcast_ref::<RemoteBindings>())
            .expect("Remote Bindings config");

        assert_eq!(bindings.grants.len(), 1);
        assert_eq!(bindings.grants[0].resource_id, "agents");
        assert_eq!(bindings.grants[0].permission_set, "sandbox/remote-execute");
    }

    /// An inline set that starts a session, named so it would clear an id-prefix test.
    fn inline_run_microvm_set(id: &str) -> alien_core::permissions::PermissionSet {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "description": "custom",
            "platforms": { "aws": [{
                "grant": { "actions": ["lambda:RunMicrovm"] },
                "binding": { "stack": { "resources": ["*"] } }
            }]}
        }))
        .expect("valid inline permission set")
    }

    #[tokio::test]
    async fn a_remote_sandbox_reached_by_an_inline_non_sandbox_set_is_refused() {
        // Reach is the resolved verb, not the id: the vendor already holds the mint on the image,
        // so a workload that can merely start a session hands it every session of the sandbox.
        let stack = Stack::new("application".to_string())
            .add_with_remote_access(sandbox(SandboxEgress::Allow), ResourceLifecycle::Frozen)
            .permission(
                "execution",
                PermissionProfile::new().global([inline_run_microvm_set("custom/telemetry")]),
            )
            .build();

        let error = RemoteBindingsMutation
            .mutate(stack, &StackState::new(Platform::Test), &config())
            .await
            .expect_err("an inline set granting RunMicrovm is not single-tenant, whatever its id");

        assert_eq!(error.code, "STACK_MUTATION_FAILED");
        assert!(
            error
                .to_string()
                .contains("permission profile 'execution' grants access to it"),
            "the refusal must name the profile, got: {error}"
        );
    }

    #[tokio::test]
    async fn a_lowercase_run_microvm_grant_does_not_slip_the_reach_scan() {
        // AWS matches action names case-insensitively, so a set the classifier read literally
        // would be a bypass by casing alone.
        let mut set = inline_run_microvm_set("custom/telemetry");
        set.platforms.aws.as_mut().unwrap()[0].grant.actions =
            Some(vec!["lambda:runmicrovm".to_string()]);
        let stack = Stack::new("application".to_string())
            .add_with_remote_access(sandbox(SandboxEgress::Allow), ResourceLifecycle::Frozen)
            .permission("execution", PermissionProfile::new().global([set]))
            .build();

        let error = RemoteBindingsMutation
            .mutate(stack, &StackState::new(Platform::Test), &config())
            .await
            .expect_err("a lowercase RunMicrovm still starts a session");
        assert_eq!(error.code, "STACK_MUTATION_FAILED");
    }

    #[tokio::test]
    async fn a_remote_sandbox_reached_by_a_service_account_set_is_refused() {
        let account = ServiceAccount::new("runner".to_string())
            .stack_permission_set(inline_run_microvm_set("custom/telemetry"))
            .build();
        let stack = Stack::new("application".to_string())
            .add_with_remote_access(sandbox(SandboxEgress::Allow), ResourceLifecycle::Frozen)
            .add(account, ResourceLifecycle::Live)
            .build();

        let error = RemoteBindingsMutation
            .mutate(stack, &StackState::new(Platform::Test), &config())
            .await
            .expect_err("a service account that can start sessions is a second tenant");

        assert_eq!(error.code, "STACK_MUTATION_FAILED");
        assert!(
            error
                .to_string()
                .contains("service account 'runner' can start sessions in it"),
            "the refusal must name the service account, got: {error}"
        );
    }

    #[tokio::test]
    async fn reserved_access_id_is_never_overwritten() {
        let stack = Stack::new("byo-bucket".to_string())
            .add(
                Storage::new(REMOTE_BINDINGS_ID.to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add_with_remote_access(
                Storage::new("exports".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let state = StackState::new(Platform::Test);

        let error = RemoteBindingsMutation
            .mutate(stack, &state, &config())
            .await
            .expect_err("reserved resource ID must fail instead of being overwritten");

        assert_eq!(error.code, "STACK_MUTATION_FAILED");
        assert!(error
            .to_string()
            .contains("reserved for application access"));
    }

    #[test]
    fn ordinary_storage_does_not_add_remote_bindings() {
        let stack = Stack::new("app".to_string())
            .add(
                Storage::new("internal".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        assert!(!RemoteBindingsMutation.should_run(
            &stack,
            &StackState::new(Platform::Test),
            &config()
        ));
    }

    #[tokio::test]
    async fn remote_key_must_be_the_only_published_resource() {
        let stack = Stack::new("application".to_string())
            .add_with_remote_access(
                Key::new("customer-key".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add_with_remote_access(
                Storage::new("exports".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let state = StackState::new(Platform::Test);

        let error = RemoteBindingsMutation
            .mutate(stack, &state, &config())
            .await
            .expect_err("a remote Key must reject another remotely published resource");

        assert_eq!(error.code, "STACK_MUTATION_FAILED");
        assert!(error.to_string().contains("only remoteAccess resource"));
    }

    #[tokio::test]
    async fn remote_key_allows_non_remote_application_resources() {
        let stack = Stack::new("application".to_string())
            .add_with_remote_access(
                Key::new("customer-key".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Storage::new("internal".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let state = StackState::new(Platform::Test);

        let mutated = RemoteBindingsMutation
            .mutate(stack, &state, &config())
            .await
            .expect("non-remote application resources are allowed beside a remote Key");
        let bindings = mutated
            .resources
            .get(REMOTE_BINDINGS_ID)
            .and_then(|entry| entry.config.downcast_ref::<RemoteBindings>())
            .expect("Remote Bindings config");

        assert_eq!(bindings.grants.len(), 1);
        assert_eq!(bindings.grants[0].resource_id, "customer-key");
        assert_eq!(bindings.grants[0].permission_set, "key/remote-cryptography");
    }

    #[tokio::test]
    async fn remote_ai_adds_only_the_inference_grant_and_allows_non_remote_siblings() {
        let stack = Stack::new("application".to_string())
            .add_with_remote_access(
                Ai::new("models".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Storage::new("internal".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();

        let mutated = RemoteBindingsMutation
            .mutate(stack, &StackState::new(Platform::Test), &config())
            .await
            .expect("non-remote siblings are allowed beside remote AI");
        let bindings = mutated
            .resources
            .get(REMOTE_BINDINGS_ID)
            .and_then(|entry| entry.config.downcast_ref::<RemoteBindings>())
            .expect("Remote Bindings config");

        assert_eq!(bindings.grants.len(), 1);
        assert_eq!(bindings.grants[0].resource_id, "models");
        assert_eq!(bindings.grants[0].permission_set, "ai/invoke");
    }

    #[tokio::test]
    async fn remote_ai_rejects_another_published_resource() {
        let stack = Stack::new("application".to_string())
            .add_with_remote_access(
                Ai::new("models".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add_with_remote_access(
                Storage::new("exports".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();

        let error = RemoteBindingsMutation
            .mutate(stack, &StackState::new(Platform::Test), &config())
            .await
            .expect_err("remote AI must reject another remotely published resource");

        assert_eq!(error.code, "STACK_MUTATION_FAILED");
        assert!(error.to_string().contains("only remoteAccess resource"));
    }
}
