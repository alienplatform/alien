//! Definite setup-owned inline role overflow after identity mutation.
//!
//! Unavailable controller names remain unresolved. No hypothetical physical names
//! or live IAM reads are used to claim complete role headroom.

use crate::{
    error::{ErrorData, Result},
    CheckResult,
};
use alien_core::{
    BuildOutputs, InitialSetupAuthority, KvOutputs, Platform, QueueOutputs, RemoteBindings,
    RemoteStackManagement, ResourceLifecycle, ServiceAccount, Stack, StackState, StorageOutputs,
    WorkerOutputs,
};
use alien_error::{Context, IntoAlienError};
use alien_permissions::{
    aws_resource_policies::{
        aws_management_resource_policy_name, aws_prefixed_resource_name, aws_remote_access_policy,
        aws_remote_access_policy_name, aws_resource_policy_name,
        aws_service_account_injected_statements, render_aws_resource_policy,
        AwsResourcePolicyContext,
    },
    generators::{AwsIamPolicy, AwsRuntimePermissionsGenerator},
    BindingTarget, PermissionContext,
};
use indexmap::IndexMap;

/// Check the resolved inline documents DirectSetup writes to frozen roles.
pub(crate) fn check(
    stack: &Stack,
    platform: Platform,
    authority: Option<InitialSetupAuthority>,
    context: &PermissionContext,
    state: Option<&StackState>,
) -> Result<CheckResult> {
    let mut result = CheckResult::success().with_check_metadata(
        Some("IAM_WORKLOAD_INLINE_AGGREGATE"),
        "Setup inline policies fit the aggregate per-role limit",
    );
    if platform != Platform::Aws || authority != Some(InitialSetupAuthority::DirectSetup) {
        return Ok(result);
    }
    let generator = AwsRuntimePermissionsGenerator::new();
    let mut roles = Vec::new();
    for (_, entry) in stack.resources() {
        let Some(account) = entry.config.downcast_ref::<ServiceAccount>() else {
            continue;
        };
        if entry.lifecycle != ResourceLifecycle::Frozen || entry.enabled_when.is_some() {
            continue;
        }
        // Only a matching profile receives workload resource policies.
        let profile = account.id.strip_suffix("-sa").and_then(|name| {
            stack
                .permission_profiles()
                .get(name)
                .filter(|_| name != "management")
                .map(|profile| (name, profile))
        });
        roles.push((Some(account), profile));
    }
    if stack.resources().any(|(_, entry)| {
        entry.config.resource_type() == RemoteStackManagement::RESOURCE_TYPE
            && entry.lifecycle == ResourceLifecycle::Frozen
            && entry.enabled_when.is_none()
    }) {
        if let Some(profile) = stack.management().profile() {
            roles.push((None, Some(("management", profile))));
        }
    }
    for (account, profile) in roles {
        let management = account.is_none();
        let role = match profile {
            Some((name, _)) => format!("permission profile '{name}'"),
            None => format!("service account '{}'", account.unwrap().id),
        };
        // Management globals are managed policy chunks, never inline documents.
        let stack_sets = account
            .map(|account| account.stack_permission_sets.as_slice())
            .unwrap_or_default();
        // None replaces an earlier resolved same-name document too: counting
        // the old document would overstate a definite lower bound.
        let mut documents: IndexMap<String, Option<String>> = IndexMap::new();
        let mut statements = Vec::new();
        let mut incomplete = profile.is_none()
            && account.is_some_and(|account| !account.resource_permission_sets.is_empty());
        for set in stack_sets {
            match generator.generate_policy(set, BindingTarget::Stack, context) {
                Ok(policy) => statements.extend(policy.statement),
                Err(_) => incomplete = true,
            }
        }
        if let Some(account) = account {
            statements.extend(aws_service_account_injected_statements(
                account, stack, context,
            ));
        }
        if !statements.is_empty() {
            let document = serde_json::to_string(&AwsIamPolicy {
                version: "2012-10-17".into(),
                statement: statements,
            })
            .into_alien_error()
            .context(ErrorData::CompileTimeCheckFailed {
                check_name: "IAM workload aggregate".into(),
                message: "Could not serialize the stack policy lower bound".into(),
                resource_id: account.map(|account| account.id.clone()),
            })?;
            documents.insert("deployment-permissions".into(), Some(document));
        }
        for (id, entry) in stack.resources() {
            let Some((_, profile)) = profile else {
                break;
            };
            if entry.lifecycle != ResourceLifecycle::Frozen || entry.enabled_when.is_some() {
                continue;
            }
            let resource_type = entry.config.resource_type().to_string();
            // These controllers call the shared workload policy helper. Other
            // controllers have distinct layouts and cannot be inferred here.
            if !matches!(
                resource_type.as_str(),
                "storage"
                    | "queue"
                    | "kv"
                    | "build"
                    | "vault"
                    | "worker"
                    | "artifact-registry"
                    | "service-account"
                    | "ai"
                    | "sandbox"
            ) {
                continue;
            }
            let explicit = profile.0.get(id).map(Vec::as_slice).unwrap_or_default();
            let prefix = format!("{resource_type}/");
            let refs = explicit.iter().chain(
                profile
                    .0
                    .get("*")
                    .into_iter()
                    .flatten()
                    .filter(|reference| !management && reference.id().starts_with(&prefix)),
            );
            let mut resource_context = context.clone().with_resource_id(id.to_string());
            // Existing/imported resources keep their recorded physical names. Never
            // infer a new name for a resource whose controller has already run.
            let name = resource_name(&resource_type, id, context, state);
            resource_context.resource_name = name;
            let selection = AwsResourcePolicyContext {
                named_by_resource: explicit,
                stack_sets,
                generator: &generator,
                permission_context: &resource_context,
            };
            for reference in refs {
                let Some(set) =
                    reference.resolve(|name| alien_permissions::get_permission_set(name).cloned())
                else {
                    incomplete = true;
                    continue;
                };
                if management && set.id.ends_with("/provision") {
                    continue;
                }
                let name = if management {
                    aws_management_resource_policy_name(id, &set.id)
                } else {
                    aws_resource_policy_name(id, &set.id)
                };
                let document = match if management {
                    Ok(false)
                } else {
                    selection.is_redundant(reference)
                } {
                    Ok(true) => continue,
                    Ok(false) => {
                        render_aws_resource_policy(id, &set, &generator, &resource_context)
                            .ok()
                            .map(|policy| policy.document)
                    }
                    Err(_) => None,
                };
                incomplete |= document.is_none();
                documents.insert(name, document);
            }
        }
        let size: usize = documents
            .values()
            .flatten()
            .map(|document| {
                document
                    .chars()
                    .filter(|c| !c.is_ascii_whitespace())
                    .count()
            })
            .sum();
        if size > 10_240 {
            result.add_error(format!("IAM role for {role} needs at least {size} inline policy characters across setup resources; the aggregate per-role limit is 10240. Reduce grants or use managed policies. https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_iam-quotas.html"));
        }
        if incomplete || !documents.is_empty() {
            result.add_warning(format!("IAM inline policy coverage for {role} is incomplete: only resolved setup documents were counted. Controller names, injected grants, remote bindings and retained policies may consume additional quota; passing does not establish total role headroom."));
        }
    }
    // A separate setup identity receives only registered published-resource grants.
    // Do not combine its documents with workload or management role documents.
    if stack.resources().any(|(_, entry)| {
        entry.config.resource_type() == RemoteBindings::RESOURCE_TYPE
            && entry.lifecycle == ResourceLifecycle::Frozen
            && entry.enabled_when.is_none()
    }) {
        let mut documents = IndexMap::new();
        let mut incomplete = false;
        for (id, entry) in stack.resources() {
            if entry.lifecycle != ResourceLifecycle::Frozen || entry.enabled_when.is_some() {
                continue;
            }
            let Some(definition) = alien_core::remote_bindings::remote_binding_for_entry(entry)
            else {
                continue;
            };
            // Key has a distinct writer; Live sandbox scaffolding is not a direct
            // resource-helper write. Neither is inferred from the common layout.
            if !matches!(
                definition.resource_type,
                "storage" | "queue" | "kv" | "ai" | "sandbox"
            ) {
                incomplete = true;
                continue;
            }
            let mut resource_context = context.clone().with_resource_id(id.to_string());
            resource_context.resource_name =
                resource_name(definition.resource_type, id, context, state);
            let document = aws_remote_access_policy(&generator, definition, &resource_context)
                .ok()
                .and_then(|policy| serde_json::to_string(&policy).ok());
            incomplete |= document.is_none();
            documents.insert(aws_remote_access_policy_name(id), document);
        }
        let size: usize = documents
            .values()
            .flatten()
            .map(|document| {
                document
                    .chars()
                    .filter(|c| !c.is_ascii_whitespace())
                    .count()
            })
            .sum();
        if size > 10_240 {
            result.add_error(format!("IAM Remote Bindings role needs at least {size} inline policy characters across setup resources; the aggregate per-role limit is 10240. Reduce published resources. https://docs.aws.amazon.com/IAM/latest/UserGuide/reference_iam-quotas.html"));
        }
        if incomplete || !documents.is_empty() {
            result.add_warning("IAM inline policy coverage for Remote Bindings is incomplete: only resolved setup documents were counted; retained policies and other controller layouts may consume additional quota. Passing does not establish total role headroom.".into());
        }
    }
    Ok(result)
}

fn resource_name(
    resource_type: &str,
    id: &str,
    context: &PermissionContext,
    state: Option<&StackState>,
) -> Option<String> {
    if matches!(
        resource_type,
        "artifact-registry" | "service-account" | "ai" | "sandbox"
    ) {
        // These writers deliberately pass the logical ID, including on
        // updates. Their outputs identify other objects, not this context.
        Some(id.to_string())
    } else {
        match state.and_then(|state| state.resources.get(id)) {
            Some(saved) => saved
                .outputs
                .as_ref()
                .and_then(|outputs| match resource_type {
                    "storage" => outputs
                        .downcast_ref::<StorageOutputs>()
                        .map(|o| o.bucket_name.clone()),
                    "queue" => outputs
                        .downcast_ref::<QueueOutputs>()
                        .map(|o| o.queue_name.clone()),
                    "kv" => outputs
                        .downcast_ref::<KvOutputs>()
                        .map(|o| o.store_name.clone()),
                    "worker" => outputs
                        .downcast_ref::<WorkerOutputs>()
                        .map(|o| o.worker_name.clone()),
                    "build" => outputs
                        .downcast_ref::<BuildOutputs>()
                        .and_then(|o| o.identifier.split_once(":project/"))
                        .filter(|(_, name)| !name.is_empty())
                        .map(|(_, name)| name.to_string()),
                    _ => None,
                }),
            None if matches!(
                resource_type,
                "storage" | "queue" | "kv" | "worker" | "build" | "vault"
            ) =>
            {
                context
                    .stack_prefix
                    .as_deref()
                    .map(|prefix| aws_prefixed_resource_name(prefix, id))
            }
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{mutations::ServiceAccountMutation, StackMutation};
    use alien_core::{
        DeploymentConfig, PermissionProfile, PermissionSetReference, StackState, Storage,
    };

    fn check(
        stack: &Stack,
        platform: Platform,
        authority: Option<InitialSetupAuthority>,
    ) -> Result<CheckResult> {
        super::check(stack, platform, authority, &PermissionContext::new(), None)
    }

    fn grant(id: &str, count: usize, dynamic: bool) -> PermissionSetReference {
        let resources: Vec<_> = (0..count)
            .map(|index| {
                if dynamic {
                    format!("arn:aws:s3:::${{resourceName}}/objects/object-{index:04}")
                } else {
                    format!("arn:aws:s3:::q5-policy-control/objects/object-{index:04}")
                }
            })
            .collect();
        PermissionSetReference::from_inline(
            serde_json::from_value(serde_json::json!({
                "id": id, "description": "Read synthetic objects",
                "platforms": {"aws": [{"grant": {"actions": ["s3:GetObject"]},
                "binding": {"resource": {"resources": resources}}}]}
            }))
            .unwrap(),
        )
    }

    async fn prepared(count: usize, second: Vec<PermissionSetReference>) -> Stack {
        let mut stack = Stack::new("q5-aggregate".into())
            .add(
                Storage::new("objects-a".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                Storage::new("objects-b".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let mut profile = PermissionProfile::new();
        profile
            .0
            .insert("objects-a".into(), vec![grant("read", count, false)]);
        profile.0.insert("objects-b".into(), second);
        stack.permissions.profiles.insert("reader".into(), profile);
        let config: DeploymentConfig = DeploymentConfig::builder()
            .stack_settings(alien_core::StackSettings::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .allow_frozen_changes(false)
            .external_bindings(alien_core::ExternalBindings::default())
            .build();
        ServiceAccountMutation
            .mutate(stack, &StackState::new(Platform::Aws), &config)
            .await
            .unwrap()
    }

    #[test]
    fn remote_bindings_aggregate_uses_registered_grants_and_saved_physical_names() {
        let context = PermissionContext::new().with_stack_prefix("q5-remote");
        let generator = AwsRuntimePermissionsGenerator::new();
        let mut stack = Stack::new("q5-remote".into())
            .add(
                RemoteBindings::new("access".into()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let mut expected = 0;
        for index in 0..30 {
            let id = format!("objects-{index:02}");
            let entry = alien_core::ResourceEntry {
                config: alien_core::Resource::new(Storage::new(id.clone()).build()),
                lifecycle: ResourceLifecycle::Frozen,
                enabled_when: None,
                dependencies: vec![],
                display_name: None,
                description: None,
                remote_access: true,
            };
            let definition = alien_core::remote_bindings::remote_binding_for_entry(&entry).unwrap();
            let policy = aws_remote_access_policy(
                &generator,
                definition,
                &context
                    .clone()
                    .with_resource_id(id.clone())
                    .with_resource_name(format!("q5-remote-{id}")),
            )
            .unwrap();
            let size = serde_json::to_string(&policy).unwrap().len();
            assert!(size < 10240);
            expected += size;
            stack.resources.insert(id, entry);
        }
        assert!(expected > 10240);
        let run = |stack: &Stack, context: &PermissionContext, state: Option<&StackState>| {
            super::check(
                stack,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup),
                context,
                state,
            )
            .unwrap()
        };
        let result = run(&stack, &context, None);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("Remote Bindings role"));
        assert!(result.errors[0].contains(&expected.to_string()));
        assert_eq!(result.warnings.len(), 1);
        // An unavailable physical name is Unknown rather than a guessed failure.
        let unresolved = run(&stack, &PermissionContext::new(), None);
        assert!(unresolved.success);
        assert_eq!(unresolved.warnings.len(), 1);
        let mut state = StackState::new(Platform::Aws);
        for (id, entry) in stack.resources() {
            if entry.config.resource_type() == Storage::RESOURCE_TYPE {
                let mut saved = alien_core::StackResourceState::new_pending(
                    "storage".into(),
                    entry.config.clone(),
                    None,
                    vec![],
                );
                saved.outputs = Some(alien_core::ResourceOutputs::new(StorageOutputs {
                    bucket_name: format!("q5-remote-{id}"),
                }));
                state.resources.insert(id.to_string(), saved);
            }
        }
        assert_eq!(
            run(&stack, &PermissionContext::new(), Some(&state)).errors,
            result.errors
        );
        // Explicit profiles are irrelevant: turning publication off revokes the grant.
        for entry in stack.resources.values_mut() {
            entry.remote_access = false;
        }
        let unpublished = run(&stack, &context, None);
        assert!(unpublished.success);
        assert!(unpublished.warnings.is_empty());
        stack.resources.get_mut("objects-00").unwrap().remote_access = true;
        assert!(run(&stack, &context, None).success);
        stack.resources.shift_remove("access");
        let absent = run(&stack, &context, None);
        assert!(absent.success);
        assert!(absent.warnings.is_empty());
    }

    #[tokio::test]
    async fn management_counts_explicit_inline_grants_without_managed_globals() {
        let mut stack = prepared(100, vec![grant("read", 100, false)]).await;
        let mut profile = stack.permissions.profiles.shift_remove("reader").unwrap();
        stack.resources.shift_remove("reader-sa");
        stack.resources.insert(
            "remote-management".into(),
            alien_core::ResourceEntry {
                config: alien_core::Resource::new(
                    RemoteStackManagement::new("remote-management".into()).build(),
                ),
                lifecycle: ResourceLifecycle::Frozen,
                enabled_when: None,
                dependencies: vec![],
                display_name: None,
                description: None,
                remote_access: false,
            },
        );
        // Wildcards are managed chunks; explicit provision grants are not inline.
        profile
            .0
            .insert("*".into(), vec![grant("storage/read", 400, false)]);
        profile
            .0
            .get_mut("objects-a")
            .unwrap()
            .push(grant("storage/provision", 400, false));
        stack.permissions.management = alien_core::ManagementPermissions::Override(profile);
        let result = check(
            &stack,
            Platform::Aws,
            Some(InitialSetupAuthority::DirectSetup),
        )
        .unwrap();
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("permission profile 'management'"));
        assert!(result.errors[0].contains("10818"), "{:?}", result.errors);
        assert_eq!(result.warnings.len(), 1);
        // One fitting resource document plus both excluded grants must pass.
        stack.resources.shift_remove("objects-b");
        assert!(
            check(
                &stack,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup)
            )
            .unwrap()
            .success
        );
        // No management role writer means no inferred inline role.
        stack.resources.shift_remove("remote-management");
        let skipped = check(
            &stack,
            Platform::Aws,
            Some(InitialSetupAuthority::DirectSetup),
        )
        .unwrap();
        assert!(skipped.success);
        assert!(skipped.warnings.is_empty());
    }

    #[tokio::test]
    async fn physical_names_count_fresh_and_imported_documents_without_guessing() {
        let generator = AwsRuntimePermissionsGenerator::new();
        let context = PermissionContext::new().with_stack_prefix("q5-names");
        for resource_type in ["storage", "queue", "kv", "worker", "build"] {
            let mut stack = prepared(100, vec![grant("read", 100, true)]).await;
            // Both documents must use physical names and logical resource IDs.
            stack
                .permissions
                .profiles
                .get_mut("reader")
                .unwrap()
                .0
                .insert("objects-a".into(), vec![grant("read", 100, true)]);
            for id in ["objects-a", "objects-b"] {
                let config = match resource_type {
                    "queue" => alien_core::Resource::new(alien_core::Queue::new(id.into()).build()),
                    "kv" => alien_core::Resource::new(alien_core::Kv::new(id.into()).build()),
                    "worker" => alien_core::Resource::new(
                        alien_core::Worker::new(id.into())
                            .permissions("reader".into())
                            .code(alien_core::WorkerCode::Image {
                                image: "q5-image:latest".into(),
                            })
                            .build(),
                    ),
                    "build" => alien_core::Resource::new(
                        alien_core::Build::new(id.into())
                            .permissions("reader".into())
                            .build(),
                    ),
                    _ => alien_core::Resource::new(Storage::new(id.into()).build()),
                };
                stack.resources.get_mut(id).unwrap().config = config;
            }
            let expected_size = |names: [&str; 2]| {
                ["objects-a", "objects-b"]
                    .into_iter()
                    .zip(names)
                    .map(|(id, name)| {
                        let set = stack.permissions.profiles["reader"].0[id][0]
                            .resolve(|_| None)
                            .unwrap();
                        render_aws_resource_policy(
                            id,
                            &set,
                            &generator,
                            &context
                                .clone()
                                .with_resource_id(id)
                                .with_resource_name(name),
                        )
                        .unwrap()
                        .document
                        .chars()
                        .filter(|c| !c.is_ascii_whitespace())
                        .count()
                    })
                    .sum::<usize>()
            };
            let fresh_size = expected_size(["q5-names-objects-a", "q5-names-objects-b"]);
            assert!(fresh_size > 10240);
            let fresh = super::check(
                &stack,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup),
                &context,
                Some(&StackState::new(Platform::Aws)),
            )
            .unwrap();
            assert_eq!(fresh.errors.len(), 1);
            assert!(fresh.errors[0].contains(&fresh_size.to_string()));

            let mut state = StackState::new(Platform::Aws);
            for id in ["objects-a", "objects-b"] {
                let mut saved = alien_core::StackResourceState::new_pending(
                    resource_type.into(),
                    stack.resources[id].config.clone(),
                    None,
                    vec![],
                );
                let name = format!("q5-imported-long-name-{id}");
                saved.outputs = Some(match resource_type {
                    "queue" => alien_core::ResourceOutputs::new(QueueOutputs {
                        queue_name: name,
                        identifier: None,
                    }),
                    "kv" => alien_core::ResourceOutputs::new(KvOutputs {
                        store_name: name,
                        identifier: None,
                        endpoint: None,
                    }),
                    "worker" => alien_core::ResourceOutputs::new(WorkerOutputs {
                        worker_name: name,
                        public_endpoints: Default::default(),
                        identifier: None,
                        commands_push_target: None,
                    }),
                    "build" => alien_core::ResourceOutputs::new(BuildOutputs {
                        identifier: format!(
                            "arn:aws:codebuild:us-east-1:123456789012:project/{name}"
                        ),
                    }),
                    _ => alien_core::ResourceOutputs::new(StorageOutputs { bucket_name: name }),
                });
                state.resources.insert(id.into(), saved);
            }
            let imported_size = expected_size([
                "q5-imported-long-name-objects-a",
                "q5-imported-long-name-objects-b",
            ]);
            assert_ne!(fresh_size, imported_size);
            let imported = super::check(
                &stack,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup),
                &context,
                Some(&state),
            )
            .unwrap();
            assert_eq!(imported.errors.len(), 1);
            assert!(imported.errors[0].contains(&imported_size.to_string()));
            for saved in state.resources.values_mut() {
                saved.outputs = None;
            }
            let unknown = super::check(
                &stack,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup),
                &context,
                Some(&state),
            )
            .unwrap();
            assert!(unknown.success);
            assert_eq!(unknown.warnings.len(), 1);
        }
    }

    #[tokio::test]
    async fn fresh_vault_prefix_counts_but_existing_output_does_not_invent_a_prefix() {
        let mut stack = prepared(100, vec![grant("read", 200, true)]).await;
        let id = "objects-b";
        stack.resources.get_mut(id).unwrap().config =
            alien_core::Resource::new(alien_core::Vault::new(id.into()).build());
        let context = PermissionContext::new().with_stack_prefix("q5-vault");
        let fresh = super::check(
            &stack,
            Platform::Aws,
            Some(InitialSetupAuthority::DirectSetup),
            &context,
            None,
        )
        .unwrap();
        assert_eq!(fresh.errors.len(), 1);
        let mut state = StackState::new(Platform::Aws);
        let mut saved = alien_core::StackResourceState::new_pending(
            "vault".into(),
            stack.resources[id].config.clone(),
            None,
            vec![],
        );
        saved.outputs = Some(alien_core::ResourceOutputs::new(alien_core::VaultOutputs {
            vault_id: "123456789012:us-east-1".into(),
        }));
        state.resources.insert(id.into(), saved);
        let existing = super::check(
            &stack,
            Platform::Aws,
            Some(InitialSetupAuthority::DirectSetup),
            &context,
            Some(&state),
        )
        .unwrap();
        assert!(existing.success);
        assert_eq!(existing.warnings.len(), 1);
    }

    #[tokio::test]
    async fn logical_writer_contexts_count_even_without_saved_outputs() {
        for resource_type in ["artifact-registry", "service-account", "ai", "sandbox"] {
            let mut stack = prepared(100, vec![grant("read", 200, true)]).await;
            for id in ["objects-a", "objects-b"] {
                stack.resources.get_mut(id).unwrap().config = match resource_type {
                    "artifact-registry" => alien_core::Resource::new(
                        alien_core::ArtifactRegistry::new(id.into()).build(),
                    ),
                    "service-account" => {
                        alien_core::Resource::new(ServiceAccount::new(id.into()).build())
                    }
                    "sandbox" => alien_core::Resource::new(
                        alien_core::Sandbox::new(id.into())
                            .code(alien_core::SandboxCode::Image {
                                image: "q5-image:latest".into(),
                                bundled_from: None,
                            })
                            .egress(alien_core::SandboxEgress::Internet)
                            .lifecycle(alien_core::SandboxLifecyclePolicy {
                                max_lifetime_seconds: None,
                                idle_pause_seconds: None,
                            })
                            .build(),
                    ),
                    _ => alien_core::Resource::new(alien_core::Ai::new(id.into()).build()),
                };
            }
            let generator = AwsRuntimePermissionsGenerator::new();
            let size: usize = ["objects-a", "objects-b"]
                .into_iter()
                .map(|id| {
                    let set = stack.permissions.profiles["reader"].0[id][0]
                        .resolve(|_| None)
                        .unwrap();
                    render_aws_resource_policy(
                        id,
                        &set,
                        &generator,
                        &PermissionContext::new()
                            .with_resource_id(id)
                            .with_resource_name(id),
                    )
                    .unwrap()
                    .document
                    .chars()
                    .filter(|c| !c.is_ascii_whitespace())
                    .count()
                })
                .sum();
            assert!(size > 10240);
            let mut state =
                StackState::with_resource_prefix(Platform::Aws, "q5-unrelated-prefix".into());
            for id in ["objects-a", "objects-b"] {
                state.resources.insert(
                    id.into(),
                    alien_core::StackResourceState::new_pending(
                        resource_type.into(),
                        stack.resources[id].config.clone(),
                        None,
                        vec![],
                    ),
                );
            }
            for saved in [None, Some(&state)] {
                let result = super::check(
                    &stack,
                    Platform::Aws,
                    Some(InitialSetupAuthority::DirectSetup),
                    &PermissionContext::new().with_stack_prefix("q5-different"),
                    saved,
                )
                .unwrap();
                assert_eq!(result.errors.len(), 1);
                assert!(result.errors[0].contains(&size.to_string()));
            }
            stack.resources.shift_remove("objects-b");
            assert!(
                super::check(
                    &stack,
                    Platform::Aws,
                    Some(InitialSetupAuthority::DirectSetup),
                    &PermissionContext::new(),
                    Some(&state)
                )
                .unwrap()
                .success
            );
        }
    }

    #[tokio::test]
    async fn injected_lambda_grants_can_overflow_an_otherwise_fitting_role() {
        let set = serde_json::from_value(serde_json::json!({
            "id": "q5-global", "description": "Synthetic size boundary",
            "platforms": {"aws": [{"grant": {"actions": ["s3:GetObject"]},
                "binding": {"stack": {"resources": [format!("arn:aws:s3:::{}", "x".repeat(9950))]}}}]}
        })).unwrap();
        let mut account = ServiceAccount::new("reader-sa".into()).build();
        account.stack_permission_sets.push(set);
        let mut stack = Stack::new("q5-injected".into())
            .add(account.clone(), ResourceLifecycle::Frozen)
            .add(
                alien_core::Worker::new("handler".into())
                    .permissions("reader".into())
                    .code(alien_core::WorkerCode::Image {
                        image: "q5-image:latest".into(),
                    })
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        stack
            .permissions
            .profiles
            .insert("reader".into(), PermissionProfile::new());
        let context = PermissionContext::new().with_managing_account_id("123456789012");
        let base = AwsRuntimePermissionsGenerator::new()
            .generate_policy(
                &account.stack_permission_sets[0],
                BindingTarget::Stack,
                &context,
            )
            .unwrap();
        let base_size = serde_json::to_string(&base).unwrap().len();
        assert!(base_size <= 10240);
        let injected = aws_service_account_injected_statements(&account, &stack, &context);
        assert_eq!(injected.len(), 2);
        assert_eq!(
            injected[1].resource,
            ["arn:aws:ecr:*:123456789012:repository/*"]
        );
        let mut final_policy = base;
        final_policy.statement.extend(injected);
        let size = serde_json::to_string(&final_policy).unwrap().len();
        assert!(size > 10240);
        let result = super::check(
            &stack,
            Platform::Aws,
            Some(InitialSetupAuthority::DirectSetup),
            &context,
            None,
        )
        .unwrap();
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains(&size.to_string()));
        assert!(
            check(
                &stack,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup)
            )
            .unwrap()
            .success
        );
        stack.resources.shift_remove("handler");
        assert!(
            super::check(
                &stack,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup),
                &context,
                None,
            )
            .unwrap()
            .success
        );
        stack.permissions.profiles.clear();
        stack.resources.insert(
            "handler".into(),
            alien_core::ResourceEntry {
                config: alien_core::Resource::new(
                    alien_core::Worker::new("handler".into())
                        .permissions("reader".into())
                        .code(alien_core::WorkerCode::Image {
                            image: "q5-image:latest".into(),
                        })
                        .build(),
                ),
                lifecycle: ResourceLifecycle::Frozen,
                enabled_when: None,
                dependencies: vec![],
                display_name: None,
                description: None,
                remote_access: false,
            },
        );
        let direct = super::check(
            &stack,
            Platform::Aws,
            Some(InitialSetupAuthority::DirectSetup),
            &context,
            None,
        )
        .unwrap();
        assert_eq!(direct.errors.len(), 1);
        assert!(direct.errors[0].contains("service account 'reader-sa'"));
        assert!(direct.errors[0].contains(&size.to_string()));
        stack.resources.shift_remove("handler");
        assert!(
            super::check(
                &stack,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup),
                &context,
                None
            )
            .unwrap()
            .success
        );
        account.stack_permission_sets.clear();
        assert!(aws_service_account_injected_statements(&account, &stack, &context).is_empty());
    }

    #[tokio::test]
    async fn direct_identity_without_profile_resolves_at_deployment_time() {
        let runner = crate::runner::PreflightRunner::with_registry(crate::PreflightRegistry::new());
        let state = StackState::with_resource_prefix(Platform::Aws, "q5-direct".into());
        let client = alien_core::ClientConfig::Aws(Box::new(alien_core::AwsClientConfig {
            account_id: "123456789012".into(),
            region: "us-east-1".into(),
            credentials: alien_core::AwsCredentials::Imds { endpoint: None },
            service_overrides: None,
        }));
        let context = PermissionContext::new()
            .with_aws_account_id("123456789012")
            .with_aws_region("us-east-1")
            .with_stack_prefix("q5-direct");
        for count in [200, 10] {
            let resources: Vec<_> = (0..count).map(|index| format!(
                "arn:aws:lambda:${{awsRegion}}:${{awsAccountId}}:function:${{stackPrefix}}-object-{index:04}"
            )).collect();
            let set = serde_json::from_value(serde_json::json!({
                "id": "q5-direct-read", "description": "Synthetic deployment-context grants",
                "platforms": {"aws": [{"grant": {"actions": ["lambda:GetFunction"]},
                    "binding": {"stack": {"resources": resources}}}]}
            }))
            .unwrap();
            let mut account = ServiceAccount::new("direct-identity".into()).build();
            account.stack_permission_sets.push(set);
            let document = AwsRuntimePermissionsGenerator::new()
                .generate_policy(
                    &account.stack_permission_sets[0],
                    BindingTarget::Stack,
                    &context,
                )
                .unwrap();
            let size = serde_json::to_string(&document)
                .unwrap()
                .chars()
                .filter(|c| !c.is_ascii_whitespace())
                .count();
            assert_eq!(size > 10240, count == 200);
            let stack = Stack::new("q5-direct".into())
                .add(account, ResourceLifecycle::Frozen)
                .build();
            assert!(stack.permission_profiles().is_empty());
            // Build time lacks deployment variables: warning, never a false failure.
            let unresolved = check(
                &stack,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup),
            )
            .unwrap();
            assert!(unresolved.success);
            assert_eq!(unresolved.warnings.len(), 1);
            let outcome = runner
                .run_deployment_time_preflights(
                    stack.clone(),
                    &state,
                    &config(),
                    &client,
                    None,
                    None,
                    Some(InitialSetupAuthority::DirectSetup),
                )
                .await;
            if count == 200 {
                let error = outcome.unwrap_err();
                assert_eq!(error.code, "VALIDATION_FAILED");
                assert!(!error.retryable);
                let Some(ErrorData::ValidationFailed { results, .. }) = error.error else {
                    panic!("structured results expected");
                };
                let aggregate = results
                    .iter()
                    .find(|result| result.code.as_deref() == Some("IAM_WORKLOAD_INLINE_AGGREGATE"))
                    .unwrap();
                assert_eq!(aggregate.errors.len(), 1);
                assert!(aggregate.errors[0].contains("service account 'direct-identity'"));
                assert!(aggregate.errors[0].contains(&size.to_string()));
                assert_eq!(aggregate.warnings.len(), 1);
            } else {
                assert!(outcome.unwrap().1.success);
            }
            for authority in [None, Some(InitialSetupAuthority::ImportedHandoff)] {
                assert!(
                    super::check(&stack, Platform::Aws, authority, &context, Some(&state))
                        .unwrap()
                        .success
                );
            }
            let mut live = stack;
            live.resources.get_mut("direct-identity").unwrap().lifecycle = ResourceLifecycle::Live;
            let skipped = super::check(
                &live,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup),
                &context,
                Some(&state),
            )
            .unwrap();
            assert!(skipped.success);
            assert!(skipped.warnings.is_empty());
        }
    }

    #[tokio::test]
    async fn post_mutation_role_aggregate_rejects_two_fitting_resource_batches() {
        let stack = prepared(100, vec![grant("read", 100, false)]).await;
        let result = check(
            &stack,
            Platform::Aws,
            Some(InitialSetupAuthority::DirectSetup),
        )
        .unwrap();
        assert!(!result.success);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("10818"), "{:?}", result.errors);
        assert!(result.errors[0].contains("reader"));
        assert_eq!(result.warnings.len(), 1);
        let mut one = stack.clone();
        one.resources.shift_remove("objects-b");
        assert!(
            check(
                &one,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup)
            )
            .unwrap()
            .success
        );
        let small = prepared(10, vec![grant("read", 10, false)]).await;
        assert!(
            check(
                &small,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup)
            )
            .unwrap()
            .success
        );
    }

    #[tokio::test]
    async fn final_same_name_document_replaces_large_or_unresolved_predecessors() {
        for second in [
            vec![grant("read", 200, false), grant("read", 10, false)],
            vec![grant("read", 200, false), grant("read", 200, true)],
            vec![grant("read", 200, true), grant("read", 10, false)],
        ] {
            let stack = prepared(100, second).await;
            let result = check(
                &stack,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup),
            )
            .unwrap();
            assert!(result.success, "{:?}", result.errors);
            assert_eq!(result.warnings.len(), 1);
        }
    }

    #[tokio::test]
    async fn imported_authority_and_live_resources_do_not_count_hypothetical_writes() {
        let mut stack = prepared(100, vec![grant("read", 100, false)]).await;
        assert_eq!(check(&stack, Platform::Aws, None).unwrap().errors.len(), 0);
        for platform in [
            Platform::Local,
            Platform::Kubernetes,
            Platform::Azure,
            Platform::Gcp,
        ] {
            let result = check(&stack, platform, Some(InitialSetupAuthority::DirectSetup)).unwrap();
            assert!(result.success);
            assert!(result.warnings.is_empty());
        }
        stack.resources.get_mut("objects-b").unwrap().lifecycle = ResourceLifecycle::Live;
        assert!(
            check(
                &stack,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup)
            )
            .unwrap()
            .success
        );
        stack.resources.get_mut("objects-a").unwrap().enabled_when = Some("unresolved".into());
        assert!(
            check(
                &stack,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup)
            )
            .unwrap()
            .success
        );
    }
    #[tokio::test]
    async fn implicit_wildcards_are_omitted_but_explicit_refs_still_consume_role_quota() {
        let mut stack = prepared(10, vec![]).await;
        let resources: Vec<_> = (0..100)
            .map(|index| format!("arn:aws:s3:::q5-policy-control/objects/object-{index:04}"))
            .collect();
        let set = serde_json::from_value(serde_json::json!({
            "id": "storage/read", "description": "Read synthetic objects",
            "platforms": {"aws": [{"grant": {"actions": ["s3:GetObject"]},
            "binding": {"resource": {"resources": resources}, "stack": {"resources": resources}}}]}
        }))
        .unwrap();
        let reference = PermissionSetReference::from_inline(set);
        for explicit in [false, true] {
            let mut profile = PermissionProfile::new();
            profile.0.insert("*".into(), vec![reference.clone()]);
            if explicit {
                profile
                    .0
                    .insert("objects-a".into(), vec![reference.clone()]);
            }
            stack.permissions.profiles.insert("reader".into(), profile);
            let config = config();
            let target = ServiceAccountMutation
                .mutate(stack.clone(), &StackState::new(Platform::Aws), &config)
                .await
                .unwrap();
            let result = check(
                &target,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup),
            )
            .unwrap();
            assert_eq!(result.success, !explicit, "{:?}", result.errors);
            assert_eq!(result.errors.len(), usize::from(explicit));
            if explicit {
                assert!(result.errors[0].contains("10832"), "{:?}", result.errors);
            }
        }
    }

    #[tokio::test]
    async fn runner_resolves_deployment_values_without_inventing_resource_names() {
        let mut stack = prepared(10, vec![]).await;
        let resources: Vec<_> = (0..100)
            .map(|index| format!(
                "arn:aws:lambda:${{awsRegion}}:${{awsAccountId}}:function:${{stackPrefix}}-object-{index:04}"
            ))
            .collect();
        let set = serde_json::from_value(serde_json::json!({
            "id": "read", "description": "Synthetic deployment-context grants",
            "platforms": {"aws": [{"grant": {"actions": ["lambda:GetFunction"]},
            "binding": {"resource": {"resources": resources}}}]}
        }))
        .unwrap();
        let reference = PermissionSetReference::from_inline(set);
        let profile = stack.permissions.profiles.get_mut("reader").unwrap();
        profile
            .0
            .insert("objects-a".into(), vec![reference.clone()]);
        profile.0.insert("objects-b".into(), vec![reference]);
        // Without deployment values these documents are unresolved and cannot block.
        assert!(
            check(
                &stack,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup)
            )
            .unwrap()
            .success
        );
        let state = StackState::with_resource_prefix(Platform::Aws, "q5-context".into());
        let client = alien_core::ClientConfig::Aws(Box::new(alien_core::AwsClientConfig {
            account_id: "123456789012".into(),
            region: "us-east-1".into(),
            credentials: alien_core::AwsCredentials::Imds { endpoint: None },
            service_overrides: None,
        }));
        let mut registry = crate::PreflightRegistry::new();
        registry.add_mutation(Box::new(ServiceAccountMutation));
        let runner = crate::runner::PreflightRunner::with_registry(registry);
        let error = runner
            .run_deployment_time_preflights(
                stack.clone(),
                &state,
                &config(),
                &client,
                None,
                None,
                Some(InitialSetupAuthority::DirectSetup),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "VALIDATION_FAILED");
        assert!(!error.retryable);
        let Some(ErrorData::ValidationFailed { results, .. }) = error.error else {
            panic!("structured results expected");
        };
        let aggregate = results
            .iter()
            .find(|result| result.code.as_deref() == Some("IAM_WORKLOAD_INLINE_AGGREGATE"))
            .unwrap();
        let context = PermissionContext::new()
            .with_aws_account_id("123456789012")
            .with_aws_region("us-east-1")
            .with_stack_prefix("q5-context");
        let generator = AwsRuntimePermissionsGenerator::new();
        let profile = stack.permissions.profiles.get("reader").unwrap();
        let set = profile.0["objects-a"][0].resolve(|_| None).unwrap();
        let document = render_aws_resource_policy("objects-a", &set, &generator, &context)
            .unwrap()
            .document;
        assert!(document
            .contains("arn:aws:lambda:us-east-1:123456789012:function:q5-context-object-0000"));
        let size = 2 * document
            .chars()
            .filter(|c| !c.is_ascii_whitespace())
            .count();
        assert!(size > 10_240);
        assert_eq!(aggregate.errors.len(), 1);
        assert!(
            aggregate.errors[0].contains(&size.to_string()),
            "{:?}",
            aggregate.errors
        );
        assert_eq!(aggregate.warnings.len(), 1);
        // Without a deployment prefix, fresh physical names remain unresolved.
        let unresolved = prepared(100, vec![grant("read", 200, true)]).await;
        assert!(
            super::check(
                &unresolved,
                Platform::Aws,
                Some(InitialSetupAuthority::DirectSetup),
                &PermissionContext::new(),
                None,
            )
            .unwrap()
            .success
        );
        stack.resources.shift_remove("objects-b");
        assert!(
            runner
                .run_deployment_time_preflights(
                    stack,
                    &state,
                    &config(),
                    &client,
                    None,
                    None,
                    Some(InitialSetupAuthority::DirectSetup),
                )
                .await
                .unwrap()
                .1
                .success
        );
    }

    fn config() -> DeploymentConfig {
        DeploymentConfig::builder()
            .stack_settings(alien_core::StackSettings::default())
            .environment_variables(alien_core::EnvironmentVariablesSnapshot {
                variables: vec![],
                hash: String::new(),
                created_at: String::new(),
            })
            .allow_frozen_changes(false)
            .external_bindings(alien_core::ExternalBindings::default())
            .build()
    }

    #[tokio::test]
    async fn deployment_runner_reports_aggregate_before_provisioning_with_exact_authority() {
        let stack = prepared(100, vec![grant("read", 100, false)]).await;
        let mut registry = crate::PreflightRegistry::new();
        registry.add_mutation(Box::new(ServiceAccountMutation));
        let runner = crate::runner::PreflightRunner::with_registry(registry);
        let state = StackState::new(Platform::Aws);
        let client = alien_core::ClientConfig::Local {
            state_directory: "/unused".into(),
        };
        for authority in [
            None,
            Some(InitialSetupAuthority::ImportedHandoff),
            Some(InitialSetupAuthority::DirectSetup),
        ] {
            let outcome = runner
                .run_deployment_time_preflights(
                    stack.clone(),
                    &state,
                    &config(),
                    &client,
                    None,
                    None,
                    authority,
                )
                .await;
            if authority == Some(InitialSetupAuthority::DirectSetup) {
                let error = outcome.unwrap_err();
                assert_eq!(error.code, "VALIDATION_FAILED");
                assert!(!error.retryable);
                let Some(ErrorData::ValidationFailed { results, .. }) = error.error else {
                    panic!("structured results expected");
                };
                let aggregate = results
                    .iter()
                    .find(|result| result.code.as_deref() == Some("IAM_WORKLOAD_INLINE_AGGREGATE"))
                    .unwrap();
                assert_eq!(aggregate.errors.len(), 1);
                assert!(aggregate.errors[0].contains("10818"));
            } else {
                assert!(outcome.unwrap().1.success);
            }
        }
    }
}
