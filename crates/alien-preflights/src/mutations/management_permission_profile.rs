use crate::error::{ErrorData, Result};
use crate::StackMutation;
use alien_core::permissions::{
    ManagementPermissions, PermissionProfile, PermissionSet, PermissionSetReference,
};
use alien_core::{
    ownership_policy_for_resource_type, Container, DeploymentConfig, KubernetesCertificateMode,
    KubernetesCluster, KubernetesExposureSettings, KubernetesHeartbeatMode,
    KubernetesIngressRouteProfile, KubernetesRouteProfile, KubernetesRouteProviderOptions,
    Platform, Postgres, ResourceLifecycle, Sandbox, Stack, StackState, Storage, Worker,
    WorkerTrigger,
};
use alien_error::{AlienError, Context, IntoAlienError};
use alien_permissions::{
    get_permission_set, list_permission_set_ids, MANAGEMENT_ROLE_GUARD, SANDBOX_SETUP_ROLES_GUARD,
};
use indexmap::IndexMap;
use std::collections::BTreeSet;

const OBSERVE_PERMISSION_SET_ID: &str = "observe/observe";

/// Automatically adds management permission profile with necessary permissions for all resources in the stack.
///
/// This mutation generates management permissions based on resource lifecycles and feature policies:
/// - Live resources get `<resourceType>/provision`.
/// - Frozen resources get management only when their ownership policy allows
///   Alien to operate part of an existing setup-owned resource.
/// - Resources get `<resourceType>/heartbeat` permission sets (when heartbeat is not Disabled)
/// - Resources get `<resourceType>/telemetry` permission sets (when telemetry is not Disabled)
pub struct ManagementPermissionProfileMutation;

#[async_trait::async_trait]
impl StackMutation for ManagementPermissionProfileMutation {
    fn description(&self) -> &'static str {
        "Automatically add management permission profile with necessary permissions for all resources in the stack"
    }

    fn should_run(
        &self,
        stack: &Stack,
        _stack_state: &StackState,
        _config: &DeploymentConfig,
    ) -> bool {
        // Only run if management is set to Auto or if we need to extend existing permissions
        match stack.management() {
            ManagementPermissions::Auto => true,
            ManagementPermissions::Extend(_) => true,
            ManagementPermissions::Override(_) => true,
        }
    }

    async fn mutate(
        &self,
        mut stack: Stack,
        stack_state: &StackState,
        config: &DeploymentConfig,
    ) -> Result<Stack> {
        alien_core::validate_automatic_setup_resources(
            &stack,
            stack_state.platform,
            config.stack_settings.setup_management,
        )
        .context(ErrorData::StackMutationFailed {
            mutation_name: self.description().to_string(),
            message: "The stack contains resources that require manual setup".to_string(),
            resource_id: None,
        })?;
        validate_deployer_secret_management(&stack)?;
        let current_management = stack.management().clone();
        if management_profile_contains(&current_management, OBSERVE_PERMISSION_SET_ID) {
            return Err(AlienError::new(ErrorData::StackMutationFailed {
                mutation_name: self.description().to_string(),
                message: "Account-wide observation is available only through a dedicated observe-only deployment; remove 'observe/observe' from the application stack"
                    .to_string(),
                resource_id: None,
            }));
        }

        if config.stack_settings.setup_management == alien_core::SetupManagement::Automatic
            && matches!(current_management, ManagementPermissions::Override(_))
        {
            return Err(AlienError::new(ErrorData::StackMutationFailed {
                mutation_name: self.description().to_string(),
                message: "Automatic infrastructure management requires auto or extended management permissions; an override cannot guarantee future resource provisioning".to_string(),
                resource_id: None,
            }));
        }

        match current_management {
            ManagementPermissions::Auto => {
                // Auto-generate management permissions based on resource lifecycles
                let management_profile =
                    generate_auto_management_profile(&stack, stack_state, config)?;
                if let Some(profile) = management_profile {
                    stack.permissions.management = ManagementPermissions::Extend(profile);
                }
            }
            ManagementPermissions::Extend(extend_profile) => {
                // Generate auto permissions first, then merge with extend profile
                let auto_profile = generate_auto_management_profile(&stack, stack_state, config)?;
                if let Some(mut auto_profile) = auto_profile {
                    // Merge the extend profile into the auto-generated profile
                    for (scope, permission_sets) in &extend_profile.0 {
                        if let Some(existing_permissions) = auto_profile.0.get_mut(scope) {
                            // Add new permission sets, avoiding duplicates
                            for permission_set in permission_sets {
                                if !existing_permissions.contains(permission_set) {
                                    existing_permissions.push(permission_set.clone());
                                }
                            }
                        } else {
                            // Add new scope if it doesn't exist
                            auto_profile
                                .0
                                .insert(scope.clone(), permission_sets.clone());
                        }
                    }
                    stack.permissions.management = ManagementPermissions::Extend(auto_profile);
                } else {
                    // No auto permissions generated, keep the extend profile as is
                    stack.permissions.management = ManagementPermissions::Extend(extend_profile);
                }
            }
            ManagementPermissions::Override(override_profile) => {
                stack.permissions.management = ManagementPermissions::Override(override_profile);
            }
        }
        add_batch_queue_metadata_permissions(&mut stack, stack_state.platform);
        protect_deployer_secrets(&mut stack, stack_state.platform, config)?;
        add_role_guards(
            &mut stack,
            stack_state.platform,
            config.stack_settings.setup_management,
        );

        Ok(stack)
    }
}

/// Batch reconciliation reads setup-owned SQS visibility even when periodic
/// heartbeats are disabled. Author that prerequisite in the setup contract so
/// existing installations require setup before runtime attempts the new read.
fn add_batch_queue_metadata_permissions(stack: &mut Stack, platform: Platform) {
    if platform != Platform::Aws {
        return;
    }
    let queues: BTreeSet<String> = stack
        .resources()
        .filter_map(|(_, entry)| entry.config.downcast_ref::<Worker>())
        .flat_map(|worker| &worker.triggers)
        .filter_map(|trigger| match trigger {
            WorkerTrigger::Queue {
                queue,
                batch: Some(_),
            } if stack
                .resources
                .get(&queue.id)
                .is_some_and(|entry| entry.lifecycle == ResourceLifecycle::Frozen) =>
            {
                Some(queue.id.clone())
            }
            _ => None,
        })
        .collect();
    if queues.is_empty() {
        return;
    }
    if matches!(stack.permissions.management, ManagementPermissions::Auto) {
        stack.permissions.management = ManagementPermissions::Extend(PermissionProfile::new());
    }
    let profile = match &mut stack.permissions.management {
        ManagementPermissions::Auto => unreachable!(),
        ManagementPermissions::Extend(profile) | ManagementPermissions::Override(profile) => {
            profile
        }
    };
    for queue in queues {
        let already_readable = ["*", queue.as_str()].iter().any(|scope| {
            profile.0.get(*scope).is_some_and(|grants| {
                grants.iter().any(|grant| matches!(grant,
                    PermissionSetReference::Name(name)
                    if matches!(name.as_str(), "queue/heartbeat" | "queue/management" | "queue/provision")
                ))
            })
        });
        if !already_readable {
            profile
                .0
                .entry(queue)
                .or_default()
                .push(PermissionSetReference::from_name("queue/heartbeat"));
        }
    }
}

/// Reject explicit grants that could bypass the reserved deployer-vault boundary.
fn validate_deployer_secret_management(stack: &Stack) -> Result<()> {
    if !stack
        .resources
        .contains_key(alien_core::DEPLOYER_SECRETS_VAULT_ID)
    {
        return Ok(());
    }
    let profile = match stack.management() {
        ManagementPermissions::Auto => return Ok(()),
        ManagementPermissions::Extend(profile) | ManagementPermissions::Override(profile) => {
            profile
        }
    };
    for (scope, permissions) in &profile.0 {
        for permission in permissions {
            // Inline cloud roles can embed arbitrary scopes or predefined payload roles.
            // Keep them out of management when native slots are active: named, scoped
            // registry permissions are the reviewable path on clouds without an IAM deny.
            let unreviewed_inline = matches!(permission, PermissionSetReference::Inline(set)
                if set.platforms.gcp.is_some() || set.platforms.azure.is_some());
            let canonical = match permission {
                PermissionSetReference::Name(name) => get_permission_set(name)
                    .map(|set| set.id.as_str())
                    .unwrap_or(name.as_str()),
                PermissionSetReference::Inline(set) => set.id.as_str(),
            };
            let forbidden = unreviewed_inline
                || (scope == alien_core::DEPLOYER_SECRETS_VAULT_ID
                    && !matches!(permission, PermissionSetReference::Name(name) if matches!(name.as_str(), "vault/heartbeat" | "vault/management")))
                || (scope == "*" && matches!(canonical, "vault/data-read" | "vault/data-write"));
            if forbidden {
                return Err(AlienError::new(ErrorData::StackMutationFailed {
                    mutation_name: "ManagementPermissionProfileMutation".to_string(),
                    message: format!("Management permission '{}' on '{}' can reach deployer secrets. Grant data access only to a concrete shared vault; deployer-secrets permits metadata access only.", permission.id(), scope),
                    resource_id: Some(alien_core::DEPLOYER_SECRETS_VAULT_ID.to_string()),
                }));
            }
        }
    }
    Ok(())
}

fn inline_vault_permission(value: serde_json::Value) -> Result<PermissionSetReference> {
    let permission: PermissionSet = serde_json::from_value(value).into_alien_error().context(
        ErrorData::StackMutationFailed {
            mutation_name: "ManagementPermissionProfileMutation".to_string(),
            message: "Failed to construct reserved-vault permissions".to_string(),
            resource_id: None,
        },
    )?;
    Ok(PermissionSetReference::from_inline(permission))
}

fn protect_deployer_secrets(
    stack: &mut Stack,
    platform: Platform,
    config: &DeploymentConfig,
) -> Result<()> {
    let mut grants = Vec::new();
    // Install this before the first workload exists: automatic management cannot
    // attach policies to its own role when a later release adds the shared vault.
    if platform == Platform::Aws
        && config.stack_settings.setup_management == alien_core::SetupManagement::Automatic
    {
        grants.push(inline_vault_permission(serde_json::json!({
            "id": "vault/stack-secrets-data",
            "description": "Read and sync stack vaults except protected deployer secrets",
            "platforms": { "aws": [{
                "grant": { "actions": ["ssm:GetParameter", "ssm:PutParameter", "ssm:DeleteParameter"] },
                "binding": { "stack": { "resources": ["arn:aws:ssm:${awsRegion}:${awsAccountId}:parameter/${stackPrefix}-*"] }, "resource": { "resources": ["arn:aws:ssm:${awsRegion}:${awsAccountId}:parameter/${stackPrefix}-*"] } }
            }] }
        }))?);
    }
    // The deny also covers stack-wide provision writes and user-supplied AWS grants.
    if platform == Platform::Aws
        && (config.stack_settings.setup_management == alien_core::SetupManagement::Automatic
            || stack
                .resources
                .contains_key(alien_core::DEPLOYER_SECRETS_VAULT_ID))
    {
        grants.push(inline_vault_permission(serde_json::json!({
            "id": "vault/protect-deployer-secrets",
            "description": "Management cannot read or modify customer-written deployer secrets",
            "platforms": { "aws": [{
                "effect": "Deny", "grant": { "actions": ["ssm:GetParameter*", "ssm:PutParameter", "ssm:DeleteParameter*", "ssm:LabelParameterVersion", "ssm:UnlabelParameterVersion", "ssm:AddTagsToResource", "ssm:RemoveTagsFromResource", "ssm:PutResourcePolicy", "ssm:DeleteResourcePolicy"] },
                "binding": { "stack": { "resources": ["arn:aws:ssm:${awsRegion}:${awsAccountId}:parameter/${stackPrefix}-deployer-secrets-*"] }, "resource": { "resources": ["arn:aws:ssm:${awsRegion}:${awsAccountId}:parameter/${stackPrefix}-deployer-secrets-*"] } }
            }, {
                "effect": "Deny",
                "grant": { "actions": ["ssm:GetParametersByPath"] },
                "binding": { "stack": { "resources": ["*"] }, "resource": { "resources": ["*"] } }
            }, {
                "effect": "Deny",
                "grant": { "actions": ["sts:AssumeRole"] },
                "binding": { "stack": { "resources": ["arn:aws:iam::${awsAccountId}:role/${stackPrefix}-*"] }, "resource": { "resources": ["arn:aws:iam::${awsAccountId}:role/${stackPrefix}-*"] } }
            }] }
        }))?);
    }
    if stack
        .resources
        .contains_key(alien_core::DEPLOYER_SECRETS_VAULT_ID)
    {
        // Presence checks must work even with heartbeat disabled and Override profiles.
        grants.push(PermissionSetReference::from_name("vault/management"));
    }
    if grants.is_empty() {
        return Ok(());
    }
    let profile = match &mut stack.permissions.management {
        ManagementPermissions::Auto => {
            stack.permissions.management = ManagementPermissions::Extend(PermissionProfile::new());
            match &mut stack.permissions.management {
                ManagementPermissions::Extend(profile) => profile,
                _ => unreachable!(),
            }
        }
        ManagementPermissions::Extend(profile) | ManagementPermissions::Override(profile) => {
            profile
        }
    };
    let global = profile.0.entry("*".to_string()).or_default();
    for grant in grants {
        // A user-authored inline permission cannot shadow a reserved guard by id.
        global.retain(|existing| existing.id() != grant.id());
        global.push(grant);
    }
    Ok(())
}

/// Whatever else management is granted on `role/<prefix>-*`, it may not change its own role, nor
/// the roles setup creates for AWS sandboxes. A Deny can only narrow the identity, so an Override
/// profile gets them too.
fn add_role_guards(
    stack: &mut Stack,
    platform: Platform,
    setup_management: alien_core::SetupManagement,
) {
    if platform != Platform::Aws {
        return;
    }
    let declares_a_sandbox = stack
        .resources()
        .any(|(_, entry)| entry.config.downcast_ref::<Sandbox>().is_some());
    let guards: Vec<PermissionSetReference> = [
        (declares_a_sandbox && setup_management != alien_core::SetupManagement::Automatic)
            .then_some(SANDBOX_SETUP_ROLES_GUARD),
        Some(MANAGEMENT_ROLE_GUARD),
    ]
    .into_iter()
    .flatten()
    .map(PermissionSetReference::from_name)
    .collect();
    let add_guards = |profile: &mut PermissionProfile| {
        let global = profile.0.entry("*".to_string()).or_default();
        for guard in &guards {
            if !global.contains(guard) {
                global.push(guard.clone());
            }
        }
    };
    match &mut stack.permissions.management {
        ManagementPermissions::Extend(profile) | ManagementPermissions::Override(profile) => {
            add_guards(profile)
        }
        ManagementPermissions::Auto => {
            let mut profile = PermissionProfile::new();
            add_guards(&mut profile);
            stack.permissions.management = ManagementPermissions::Extend(profile);
        }
    }
}

fn management_profile_contains(
    management: &ManagementPermissions,
    permission_set_id: &str,
) -> bool {
    let profile = match management {
        ManagementPermissions::Auto => return false,
        ManagementPermissions::Extend(profile) | ManagementPermissions::Override(profile) => {
            profile
        }
    };

    profile
        .0
        .values()
        .flatten()
        .any(|permission| permission.id() == permission_set_id)
}

/// Generates the default management permission profile from resource ownership
/// and feature settings.
fn generate_auto_management_profile(
    stack: &Stack,
    stack_state: &StackState,
    config: &DeploymentConfig,
) -> Result<Option<PermissionProfile>> {
    let mut permission_set_ids = BTreeSet::new();
    let mut resource_permission_set_ids: IndexMap<String, BTreeSet<String>> = IndexMap::new();
    let platform = stack_state.platform;
    let automatic_aws = platform == Platform::Aws
        && config.stack_settings.setup_management == alien_core::SetupManagement::Automatic;

    if config.stack_settings.setup_management == alien_core::SetupManagement::Automatic {
        // A direct setup explicitly grants ongoing infrastructure control for all supported
        // resource types, including ones a later release adds. Reuse the permission registry
        // rather than keeping a second cloud-specific administrator-policy implementation.
        // Data payload permissions stay explicit; on AWS they are stack-scoped (see below)
        // because the management role cannot receive a resource-scoped grant after setup.
        for id in list_permission_set_ids() {
            let Some((resource_type, operation)) = id.split_once('/') else {
                continue;
            };
            let requested_operation = matches!(operation, "provision" | "management" | "heartbeat")
                || (operation == "telemetry" && config.stack_settings.telemetry.is_enabled());
            if requested_operation
                && resource_type != "remote-stack-management"
                && supports_stack_grant(id, platform)
            {
                permission_set_ids.insert(id.to_string());
            }
        }
        if platform == Platform::Aws {
            permission_set_ids.insert("worker/dispatch-command".to_string());
        }
    }

    // Iterate through all resources in the stack to determine required management permissions
    for (resource_id, resource_entry) in stack.resources() {
        let resource_type_value = resource_entry.config.resource_type();
        let resource_type = resource_type_value.0.as_ref();
        let permission_resource_type = permission_resource_type(resource_type);
        let policy = ownership_policy_for_resource_type(resource_type);

        match resource_entry.lifecycle {
            ResourceLifecycle::Live => {
                // Live resources are Alien-owned. Provision is required so
                // Alien can create, replace, and delete them after setup.
                permission_set_ids.insert(format!("{}/provision", permission_resource_type));
            }
            ResourceLifecycle::Frozen if policy.requires_management_permissions() => {
                let permission = format!("{}/management", permission_resource_type);
                if resource_type == "key" {
                    // Key metadata access must name the exact customer key.
                    // The Key emitter binds this permission after setup knows
                    // the provider resource identifier.
                    resource_permission_set_ids
                        .entry(resource_id.clone())
                        .or_default()
                        .insert(permission);
                } else {
                    permission_set_ids.insert(permission);
                }
            }
            ResourceLifecycle::Frozen => {
                // Frozen resources are setup-owned by default. Heartbeat,
                // telemetry, and explicit policy-granted management are added
                // independently.
            }
        }

        // After a Terraform setup the manager builds and replaces a Frozen GCP sandbox's template;
        // a direct setup does it with the deployer's credentials. Resource-scoped: the GCP emitter
        // binds it on this sandbox's engine once setup has created it.
        if platform == Platform::Gcp
            && resource_type == "sandbox"
            && resource_entry.lifecycle == ResourceLifecycle::Frozen
        {
            resource_permission_set_ids
                .entry(resource_id.clone())
                .or_default()
                .insert("sandbox/templates".to_string());
        }

        // Disk images live on the data plane, which `provision` and `management` do not reach.
        // Every lifecycle and image, so a later switch to a registry image rolls without rerunning
        // setup; the Azure emitter binds it on this sandbox's group.
        if platform == Platform::Azure && resource_type == "sandbox" {
            resource_permission_set_ids
                .entry(resource_id.clone())
                .or_default()
                .insert("sandbox/images".to_string());
        }

        if resource_type == "key" && platform == Platform::Aws {
            // Setups from before this grant keep updating without it; see
            // PermissionProfilesUnchangedCheck.
            resource_permission_set_ids
                .entry(resource_id.clone())
                .or_default()
                .insert(KEY_AVAILABILITY_PERMISSION_SET_ID.to_string());
        }

        if platform == Platform::Aws {
            if resource_entry
                .config
                .downcast_ref::<Postgres>()
                .is_some_and(|postgres| postgres.encryption_key.is_some())
            {
                // Setup still owns the cluster. Recovery may only start that
                // existing cluster after its customer key becomes available.
                resource_permission_set_ids
                    .entry(resource_id.clone())
                    .or_default()
                    .insert("postgres/key-recovery".to_string());
            }
            if let Some(key) = resource_entry
                .config
                .downcast_ref::<Container>()
                .and_then(|container| container.persistent_storage.as_ref())
                .and_then(|storage| storage.encryption_key.as_ref())
            {
                resource_permission_set_ids
                    .entry(key.id().to_string())
                    .or_default()
                    .insert("key/native-volume".to_string());
            }
        }

        // Add heartbeat permissions if heartbeat is enabled (Auto or RequiresApproval)
        // Disabled means no infrastructure/IAM permissions at all
        if config.stack_settings.heartbeats.is_enabled() {
            add_cloud_heartbeat_permission(
                resource_id,
                resource_type,
                permission_resource_type,
                resource_entry,
                &mut permission_set_ids,
                &mut resource_permission_set_ids,
            );
        }

        // Add telemetry permissions if telemetry is enabled (Auto or RequiresApproval)
        // Disabled means no infrastructure/IAM permissions at all
        if config.stack_settings.telemetry.is_enabled() {
            permission_set_ids.insert(format!("{}/telemetry", permission_resource_type));
        }

        // Add command dispatch permissions for workers with commands_enabled = true.
        if resource_type == "worker" {
            if let Some(worker) = resource_entry.config.downcast_ref::<Worker>() {
                if worker.commands_enabled {
                    match platform {
                        Platform::Aws
                            if config.stack_settings.setup_management
                                == alien_core::SetupManagement::Automatic =>
                        {
                            // Already installed by approved setup. Runtime credentials
                            // cannot attach another policy to their own management role.
                        }
                        Platform::Aws | Platform::Gcp | Platform::Azure => {
                            // Preflights author the explicit command dispatch grant for
                            // this concrete worker. Each cloud maps it to that worker's
                            // platform command transport.
                            resource_permission_set_ids
                                .entry(resource_id.clone())
                                .or_default()
                                .insert("worker/dispatch-command".to_string());
                        }
                        _ => {
                            // Other platforms like Kubernetes and Local use HTTP polling.
                        }
                    }
                }
            }
        }

        if platform == Platform::Kubernetes
            && kubernetes_exposure_needs_acm_import(config)
            && resource_needs_kubernetes_public_endpoint(resource_entry)
        {
            resource_permission_set_ids
                .entry(resource_id.clone())
                .or_default()
                .insert("kubernetes-public-endpoint/management".to_string());
        }

        // Automatic AWS installs configure trigger notifications through storage/provision,
        // which approved setup installs stack-wide (see above).
        if !automatic_aws {
            add_storage_trigger_source_management_permissions(
                stack,
                platform,
                resource_entry,
                &mut resource_permission_set_ids,
            );
        }
    }

    if automatic_aws {
        if let Some((resource_id, permission_set_ids)) = resource_permission_set_ids.first() {
            return Err(AlienError::new(ErrorData::StackMutationFailed {
                mutation_name: "management permission profile".to_string(),
                message: format!(
                    "Automatic infrastructure management cannot grant {permission_set_ids:?} on '{resource_id}': the management role cannot change its own policies after setup; use manual setup"
                ),
                resource_id: Some(resource_id.clone()),
            }));
        }
    }

    if permission_set_ids.is_empty() && resource_permission_set_ids.is_empty() {
        return Ok(None);
    }

    fn resolve_permission_refs(
        permission_set_ids: BTreeSet<String>,
    ) -> Vec<PermissionSetReference> {
        let mut valid_permission_refs = Vec::new();
        for permission_set_id in permission_set_ids {
            if get_permission_set(&permission_set_id).is_some() {
                valid_permission_refs.push(PermissionSetReference::from_name(permission_set_id));
            } else {
                // Log warning but continue - allows system to work even if some permission sets are missing
                tracing::debug!(
                    permission_set_id = %permission_set_id,
                    "Management permission set not found in registry, skipping"
                );
            }
        }
        valid_permission_refs
    }

    // Validate permission sets exist in registry and filter out missing ones
    let valid_permission_refs = resolve_permission_refs(permission_set_ids);

    // Create the management permission profile. Auto lifecycle permissions are
    // wildcard-scoped. Permissions that address imported/existing physical
    // resources, such as KubernetesCluster cloud metadata heartbeats, are scoped
    // to the concrete resource so setup can use its actual provider identity.
    let mut management_permissions = IndexMap::new();
    if !valid_permission_refs.is_empty() {
        management_permissions.insert("*".to_string(), valid_permission_refs);
    }

    for (resource_id, permission_set_ids) in resource_permission_set_ids {
        let valid_resource_refs = resolve_permission_refs(permission_set_ids);
        if !valid_resource_refs.is_empty() {
            management_permissions.insert(resource_id, valid_resource_refs);
        }
    }

    if management_permissions.is_empty() {
        return Ok(None);
    }

    Ok(Some(PermissionProfile(management_permissions)))
}

/// Future resources can only be covered by grants whose binding exists before the resource.
/// Resource-only permissions keep their explicit resource scope below.
fn supports_stack_grant(id: &str, platform: Platform) -> bool {
    let Some(permission_set) = get_permission_set(id) else {
        return false;
    };
    match platform {
        Platform::Aws => permission_set
            .platforms
            .aws
            .as_ref()
            .is_some_and(|entries| {
                !entries.is_empty() && entries.iter().all(|entry| entry.binding.stack.is_some())
            }),
        Platform::Gcp => permission_set
            .platforms
            .gcp
            .as_ref()
            .is_some_and(|entries| {
                !entries.is_empty() && entries.iter().all(|entry| entry.binding.stack.is_some())
            }),
        Platform::Azure => permission_set
            .platforms
            .azure
            .as_ref()
            .is_some_and(|entries| {
                !entries.is_empty() && entries.iter().all(|entry| entry.binding.stack.is_some())
            }),
        Platform::Kubernetes | Platform::Machines | Platform::Local | Platform::Test => false,
    }
}

fn add_storage_trigger_source_management_permissions(
    stack: &Stack,
    platform: Platform,
    resource_entry: &alien_core::ResourceEntry,
    resource_permission_set_ids: &mut IndexMap<String, BTreeSet<String>>,
) {
    if !matches!(platform, Platform::Aws | Platform::Gcp | Platform::Azure) {
        return;
    }

    if resource_entry.lifecycle != ResourceLifecycle::Live {
        return;
    }

    let Some(worker) = resource_entry.config.downcast_ref::<Worker>() else {
        return;
    };

    for trigger in &worker.triggers {
        let WorkerTrigger::Storage { storage, .. } = trigger else {
            continue;
        };
        if storage.resource_type != Storage::RESOURCE_TYPE {
            continue;
        }

        let Some(source_entry) = stack.resources.get(storage.id()) else {
            continue;
        };
        if source_entry.lifecycle != ResourceLifecycle::Frozen {
            continue;
        }

        resource_permission_set_ids
            .entry(storage.id().to_string())
            .or_default()
            .insert("storage/trigger-management".to_string());
    }
}

fn kubernetes_exposure_needs_acm_import(config: &DeploymentConfig) -> bool {
    let Some(KubernetesExposureSettings::Generated { route, certificate }) = config
        .stack_settings
        .kubernetes
        .as_ref()
        .and_then(|settings| settings.exposure.as_ref())
    else {
        return false;
    };

    matches!(
        certificate,
        KubernetesCertificateMode::ManagedAcmImport { .. }
    ) && matches!(
        route,
        KubernetesRouteProfile::Ingress(KubernetesIngressRouteProfile {
            provider: Some(KubernetesRouteProviderOptions::AwsAlb { .. }),
            ..
        })
    )
}

fn resource_needs_kubernetes_public_endpoint(resource_entry: &alien_core::ResourceEntry) -> bool {
    resource_entry
        .config
        .downcast_ref::<Worker>()
        .is_some_and(|worker| !worker.public_endpoints.is_empty())
        || resource_entry
            .config
            .downcast_ref::<Container>()
            .is_some_and(|container| !container.public_endpoints.is_empty())
}

fn resource_needs_cloud_heartbeat_permission(
    resource_type: &str,
    resource_entry: &alien_core::ResourceEntry,
) -> bool {
    if resource_type != KubernetesCluster::RESOURCE_TYPE.as_ref() {
        return true;
    }

    resource_entry
        .config
        .downcast_ref::<KubernetesCluster>()
        .is_some_and(|cluster| {
            cluster.heartbeat_mode == KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata
        })
}

fn add_cloud_heartbeat_permission(
    resource_id: &str,
    resource_type: &str,
    permission_resource_type: &str,
    resource_entry: &alien_core::ResourceEntry,
    permission_set_ids: &mut BTreeSet<String>,
    resource_permission_set_ids: &mut IndexMap<String, BTreeSet<String>>,
) {
    if !resource_needs_cloud_heartbeat_permission(resource_type, resource_entry) {
        return;
    }

    let permission_set_id = format!("{}/heartbeat", permission_resource_type);
    if resource_type == KubernetesCluster::RESOURCE_TYPE.as_ref() {
        resource_permission_set_ids
            .entry(resource_id.to_string())
            .or_default()
            .insert(permission_set_id);
    } else {
        permission_set_ids.insert(permission_set_id);
    }
}

/// Lets the AWS Key controller probe a customer key's availability.
pub(crate) const KEY_AVAILABILITY_PERMISSION_SET_ID: &str = "key/availability";

/// The `'*'`-scope grant suffixes this derivation emits per resource type
/// (the `format!("{}/…")` inserts above). A check exempting gate-derived
/// grants must recognize exactly this set — a suffix added there without
/// updating this list turns legitimate gate toggles into refused updates.
/// Data grants are never derived, so they are never in this list.
pub(crate) const GATE_DERIVED_GLOBAL_SUFFIXES: [&str; 4] =
    ["provision", "management", "heartbeat", "telemetry"];

/// Maps a raw resource type to the namespace its permission-set ids use.
/// Every check that matches grant ids by `<type>/` prefix must build the
/// prefix through this map, or the check and the grants silently diverge.
pub(crate) fn permission_resource_type(resource_type: &str) -> &str {
    match resource_type {
        "azure_resource_group" => "azure-resource-group",
        "azure_storage_account" => "azure-storage-account",
        "azure_container_apps_environment" => "azure-container-apps-environment",
        "azure_service_bus_namespace" => "azure-service-bus-namespace",
        "service_activation" => "service-activation",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_permissions::{
        generators::AwsRuntimePermissionsGenerator, BindingTarget, PermissionContext,
    };

    #[tokio::test]
    async fn deployer_vault_management_is_metadata_only_in_each_setup_mode() {
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            for automatic in [false, true] {
                let mut config = DeploymentConfig::builder()
                    .stack_settings(alien_core::StackSettings::default())
                    .environment_variables(empty_env_snapshot())
                    .allow_frozen_changes(false)
                    .external_bindings(alien_core::ExternalBindings::default())
                    .build();
                if automatic {
                    config.stack_settings.setup_management = alien_core::SetupManagement::Automatic;
                }
                let stack = Stack::new("test".to_string())
                    .add(
                        alien_core::Vault::new(alien_core::DEPLOYER_SECRETS_VAULT_ID.to_string())
                            .build(),
                        ResourceLifecycle::Frozen,
                    )
                    .build();
                let prepared = ManagementPermissionProfileMutation
                    .mutate(stack, &StackState::new(platform), &config)
                    .await
                    .unwrap();
                let profile = prepared.management().profile().unwrap();
                assert!(profile.0["*"]
                    .iter()
                    .any(|permission| permission.id() == "vault/management"));
                assert!(!profile.0["*"].iter().any(|permission| matches!(
                    permission.id(),
                    "vault/data-read" | "vault/data-write"
                )));
                assert!(profile
                    .0
                    .get(alien_core::DEPLOYER_SECRETS_VAULT_ID)
                    .is_none_or(|permissions| permissions.iter().all(|permission| matches!(
                        permission.id(),
                        "vault/heartbeat" | "vault/management"
                    ))));
            }
        }
    }

    #[tokio::test]
    async fn aws_management_guards_apply_in_every_setup_and_profile_mode() {
        let context = PermissionContext::new()
            .with_aws_account_id("123456789012")
            .with_aws_region("us-east-1")
            .with_stack_prefix("acme");
        for automatic in [false, true] {
            for slots in [false, true] {
                for mode in ["auto", "extend", "override"] {
                    let mut config = deployment_config_for_management_permission_test();
                    if automatic {
                        config.stack_settings.setup_management =
                            alien_core::SetupManagement::Automatic;
                    }
                    let builder = Stack::new("test".into());
                    let mut stack = if slots {
                        builder
                            .add(
                                alien_core::Vault::new(
                                    alien_core::DEPLOYER_SECRETS_VAULT_ID.into(),
                                )
                                .build(),
                                ResourceLifecycle::Frozen,
                            )
                            .build()
                    } else {
                        builder.build()
                    };
                    stack.permissions.management = management_permissions_for_test(mode);
                    let result = ManagementPermissionProfileMutation
                        .mutate(stack, &StackState::new(Platform::Aws), &config)
                        .await;
                    if automatic && mode == "override" {
                        assert!(
                            result.is_err(),
                            "automatic setup does not support overrides"
                        );
                        continue;
                    }
                    let prepared = result.unwrap();
                    let profile = prepared.management().profile().unwrap();
                    let grants = &profile.0["*"];
                    let guard = grants
                        .iter()
                        .find(|grant| grant.id() == "vault/protect-deployer-secrets");
                    assert_eq!(guard.is_some(), automatic || slots);
                    if let Some(guard) = guard {
                        let policy = AwsRuntimePermissionsGenerator::new()
                            .generate_policy(
                                &guard.resolve(|_| None).unwrap(),
                                BindingTarget::Stack,
                                &context,
                            )
                            .unwrap();
                        assert!(policy
                            .statement
                            .iter()
                            .all(|statement| statement.effect == "Deny"));
                        let assume = policy
                            .statement
                            .iter()
                            .find(|statement| statement.action.contains(&"sts:AssumeRole".into()))
                            .unwrap();
                        assert_eq!(assume.resource, ["arn:aws:iam::123456789012:role/acme-*"]);
                        let values = policy
                            .statement
                            .iter()
                            .find(|statement| {
                                statement.action.contains(&"ssm:GetParameter*".into())
                            })
                            .unwrap();
                        assert_eq!(values.resource, ["arn:aws:ssm:us-east-1:123456789012:parameter/acme-deployer-secrets-*"]);
                    }
                    if automatic {
                        let grant = grants
                            .iter()
                            .find(|grant| grant.id() == "vault/stack-secrets-data")
                            .unwrap();
                        let policy = AwsRuntimePermissionsGenerator::new()
                            .generate_policy(
                                &grant.resolve(|_| None).unwrap(),
                                BindingTarget::Stack,
                                &context,
                            )
                            .unwrap();
                        assert_eq!(policy.statement[0].effect, "Allow");
                        assert_eq!(
                            policy.statement[0].resource,
                            ["arn:aws:ssm:us-east-1:123456789012:parameter/acme-*"]
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn explicit_management_data_grants_cannot_reopen_deployer_vault() {
        for scope in ["*", alien_core::DEPLOYER_SECRETS_VAULT_ID] {
            for id in [
                "vault/data-read",
                "vault/data-write",
                "vault/data_read",
                "vault/data_write",
            ] {
                for override_profile in [false, true] {
                    let profile = PermissionProfile::new().resource(scope, [id]);
                    let stack = Stack::new("test".to_string())
                        .add(
                            alien_core::Vault::new(
                                alien_core::DEPLOYER_SECRETS_VAULT_ID.to_string(),
                            )
                            .build(),
                            ResourceLifecycle::Frozen,
                        )
                        .management(if override_profile {
                            ManagementPermissions::Override(profile)
                        } else {
                            ManagementPermissions::Extend(profile)
                        })
                        .build();
                    assert!(
                        validate_deployer_secret_management(&stack).is_err(),
                        "{scope} {id}"
                    );
                }
            }
        }
        let shared = Stack::new("test".to_string())
            .management(ManagementPermissions::Extend(
                PermissionProfile::new().resource(
                    alien_core::SECRETS_VAULT_ID,
                    ["vault/data-read", "vault/data-write"],
                ),
            ))
            .build();
        assert!(validate_deployer_secret_management(&shared).is_ok());
        let no_slots = Stack::new("test".to_string())
            .management(ManagementPermissions::Extend(
                PermissionProfile::new().resource("*", ["vault/data-read", "vault/data-write"]),
            ))
            .build();
        assert!(validate_deployer_secret_management(&no_slots).is_ok());
    }

    #[test]
    fn automatic_stack_grants_render_on_each_cloud() {
        use alien_permissions::generators::{
            AwsRuntimePermissionsGenerator, AzureRuntimePermissionsGenerator,
            GcpRuntimePermissionsGenerator,
        };
        use alien_permissions::{BindingTarget, PermissionContext};

        let context = PermissionContext::new()
            .with_aws_account_id("123456789012")
            .with_aws_region("us-east-1")
            .with_managing_account_id("123456789012")
            .with_managing_role_arn("arn:aws:iam::123456789012:role/automatic-management-test")
            .with_project_name("automatic-management-test")
            .with_project_number("123456789012")
            .with_managing_project_id("automatic-management-test")
            .with_region("us-central1")
            .with_subscription_id("00000000-0000-0000-0000-000000000001")
            .with_managing_subscription_id("00000000-0000-0000-0000-000000000001")
            .with_managing_resource_group("automatic-management-test")
            .with_resource_group("automatic-management-test")
            .with_stack_prefix("automatic-management-test")
            .with_principal_id("00000000-0000-0000-0000-000000000002");

        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            for id in list_permission_set_ids() {
                let Some((resource_type, operation)) = id.split_once('/') else {
                    continue;
                };
                let requested = matches!(operation, "provision" | "management" | "heartbeat")
                    || (platform == Platform::Aws
                        && matches!(
                            id,
                            "worker/dispatch-command" | "vault/data-read" | "vault/data-write"
                        ));
                if !requested
                    || resource_type == "remote-stack-management"
                    || !supports_stack_grant(id, platform)
                {
                    continue;
                }
                let permission_set = get_permission_set(id).unwrap();
                let result = match platform {
                    Platform::Aws => AwsRuntimePermissionsGenerator::new()
                        .generate_policy(&permission_set, BindingTarget::Stack, &context)
                        .map(|_| ()),
                    Platform::Gcp => GcpRuntimePermissionsGenerator::new()
                        .generate_grant_plan(&permission_set, BindingTarget::Stack, &context)
                        .map(|_| ()),
                    Platform::Azure => AzureRuntimePermissionsGenerator::new()
                        .generate_grant_plan(&permission_set, BindingTarget::Stack, &context)
                        .map(|plan| {
                            for role in plan.custom_roles {
                                for action in role.role_definition.actions.iter().chain(&role.role_definition.data_actions) {
                                    assert!(!action.starts_with("Microsoft.KeyVault/vaults/secrets/") && !action.starts_with("Microsoft.KeyVault/vaults/certificates/"), "Automatic management set {id} reaches secret values: {action}");
                                }
                            }
                        }),
                    _ => unreachable!(),
                };
                result.unwrap_or_else(|error| panic!("{platform} {id}: {error}"));
            }
        }
    }
    use alien_core::permissions::{ManagementPermissions, PermissionsConfig};
    use alien_core::{
        ArtifactRegistry, AzureContainerAppsEnvironment, AzureResourceGroup,
        AzureServiceBusNamespace, AzureStorageAccount, CapacityGroup, ComputeCluster, Container,
        ContainerCode, DeploymentModel, Email, EnvironmentVariablesSnapshot, ExternalBindings,
        HeartbeatsMode, KubernetesCertificateMode, KubernetesCluster, KubernetesClusterOwnership,
        KubernetesClusterProvider, KubernetesExposureSettings, KubernetesHeartbeatMode,
        KubernetesIngressRouteProfile, KubernetesRouteProfile, KubernetesRouteProviderOptions,
        KubernetesSettings, ResourceEntry, ResourceLifecycle, ResourceSpec, Sandbox, SandboxCode,
        SandboxEgress, SandboxLifecyclePolicy, ServiceActivation, StackSettings, StackState,
        Storage, TelemetryMode, Worker, WorkerCode, WorkerPublicEndpoint,
    };

    fn empty_env_snapshot() -> EnvironmentVariablesSnapshot {
        EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: String::new(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    fn management_permissions_for_test(mode: &str) -> ManagementPermissions {
        match mode {
            "auto" => ManagementPermissions::Auto,
            "extend" => ManagementPermissions::Extend(
                PermissionProfile::new().global(["worker/management"]),
            ),
            "override" => ManagementPermissions::Override(
                PermissionProfile::new().global(["worker/management"]),
            ),
            _ => panic!("unknown management permission mode: {mode}"),
        }
    }

    fn deployment_config_for_management_permission_test() -> DeploymentConfig {
        DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build()
    }

    #[tokio::test]
    async fn automatic_aws_command_workers_reuse_the_installed_stack_grant() {
        let mut config = deployment_config_for_management_permission_test();
        config.stack_settings.setup_management = alien_core::SetupManagement::Automatic;
        let state = StackState::new(Platform::Aws);
        let initial = Stack::new("example".to_string()).build();
        let updated = Stack::new("example".to_string())
            .add(
                Worker::new("later-worker".to_string())
                    .code(WorkerCode::Image {
                        image: "test:latest".to_string(),
                    })
                    .permissions("test".to_string())
                    .commands_enabled(true)
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        for stack in [initial, updated] {
            let prepared = ManagementPermissionProfileMutation
                .mutate(stack, &state, &config)
                .await
                .expect("automatic command workers are supported");
            let profile = prepared.management().profile().expect("management grant");
            assert!(profile.0["*"]
                .iter()
                .any(|grant| grant.id() == "worker/dispatch-command"));
            assert!(!profile
                .0
                .get("later-worker")
                .into_iter()
                .flatten()
                .any(|grant| grant.id() == "worker/dispatch-command"));
        }
    }

    /// The AWS management role may not change its own policies after setup, so an automatic
    /// grant must hold no resource-scoped entry for a runtime controller to attach. Storage
    /// triggers on Frozen storage are configured through the installed storage/provision.
    #[tokio::test]
    async fn automatic_aws_grant_has_nothing_for_runtime_controllers_to_attach() {
        let mut config = deployment_config_for_management_permission_test();
        config.stack_settings.setup_management = alien_core::SetupManagement::Automatic;
        let state = StackState::new(Platform::Aws);
        let storage = Storage::new("uploads".to_string()).build();
        let worker = Worker::new("processor".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .commands_enabled(true)
            .trigger(WorkerTrigger::storage(
                &storage,
                vec!["created".to_string()],
            ))
            .build();
        let stack = Stack::new("example".to_string())
            .add(storage, ResourceLifecycle::Frozen)
            .add(worker, ResourceLifecycle::Live)
            .build();

        let prepared = ManagementPermissionProfileMutation
            .mutate(stack, &state, &config)
            .await
            .expect("storage-triggered workers are supported by automatic setup");
        let profile = prepared.management().profile().expect("management grant");
        assert_eq!(
            profile.0.keys().collect::<Vec<_>>(),
            vec!["*"],
            "only stack-wide grants: {profile:?}"
        );
        for id in [
            "storage/provision",
            "worker/dispatch-command",
            "vault/stack-secrets-data",
            "vault/protect-deployer-secrets",
        ] {
            assert!(
                profile.0["*"].iter().any(|grant| grant.id() == id),
                "approved setup installs {id}"
            );
        }
    }

    #[tokio::test]
    async fn automatic_keys_are_refused_before_provisioning() {
        let stack = Stack::new("example".to_string())
            .add(
                alien_core::Key::new("customer-key".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let state = StackState::new(platform);
            let mut config = deployment_config_for_management_permission_test();
            ManagementPermissionProfileMutation
                .mutate(stack.clone(), &state, &config)
                .await
                .expect("manual key setup remains supported");
            config.stack_settings.setup_management = alien_core::SetupManagement::Automatic;
            let error = ManagementPermissionProfileMutation
                .mutate(stack.clone(), &state, &config)
                .await
                .expect_err("initial setup and later key additions must stop before cloud changes");
            assert!(error.to_string().contains("manual setup"), "{error}");
        }
    }

    #[tokio::test]
    async fn automatic_azure_sandbox_is_refused_before_provisioning() {
        let stack = Stack::new("example".to_string())
            .add(
                Sandbox::new("agents".to_string())
                    .code(SandboxCode::Image {
                        image: "ubuntu".to_string(),
                    })
                    .egress(SandboxEgress::Internet)
                    .lifecycle(SandboxLifecyclePolicy {
                        max_lifetime_seconds: None,
                        idle_pause_seconds: None,
                    })
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let state = StackState::new(Platform::Azure);
        let mut config = deployment_config_for_management_permission_test();

        ManagementPermissionProfileMutation
            .mutate(stack.clone(), &state, &config)
            .await
            .expect("manual Azure setup retains its sandbox support");

        config.stack_settings.setup_management = alien_core::SetupManagement::Automatic;
        ManagementPermissionProfileMutation
            .mutate(stack, &state, &config)
            .await
            .expect_err("initial setup and later sandbox additions must fail before cloud changes");

        ManagementPermissionProfileMutation
            .mutate(Stack::new("example".to_string()).build(), &state, &config)
            .await
            .expect("automatic Azure management without a sandbox stays supported");
    }

    #[tokio::test]
    async fn email_heartbeat_permission_is_added_to_management() {
        let stack = Stack::new("test-stack".to_string())
            .add(
                Email::new("mail".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .build();
        let stack_state = StackState::new(Platform::Aws);

        let result_stack = ManagementPermissionProfileMutation
            .mutate(
                stack,
                &stack_state,
                &deployment_config_for_management_permission_test(),
            )
            .await
            .expect("management permission mutation should succeed");

        let permissions = result_stack
            .management()
            .profile()
            .expect("auto management profile should be generated");
        assert!(permissions
            .0
            .get("*")
            .expect("global management permissions")
            .iter()
            .any(|permission| permission.id() == "email/heartbeat"));
    }

    #[tokio::test]
    async fn every_aws_stack_with_a_sandbox_keeps_its_setup_roles_out_of_managements_reach() {
        let sandbox = |id: &str| {
            Sandbox::new(id.to_string())
                .code(SandboxCode::Image {
                    image: "s3://acme-artifacts/sandbox-bundle/f00dcafe/bundle.zip".to_string(),
                })
                .egress(SandboxEgress::Internet)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build()
        };
        let guarded_globally = |management: &ManagementPermissions| {
            let profile = match management {
                ManagementPermissions::Auto => return false,
                ManagementPermissions::Extend(profile)
                | ManagementPermissions::Override(profile) => profile,
            };
            let at = |scope: &str| {
                profile.0.get(scope).is_some_and(|refs| {
                    refs.iter()
                        .any(|permission| permission.id() == SANDBOX_SETUP_ROLES_GUARD)
                })
            };
            assert!(
                !at("live-box") && !at("frozen-box"),
                "one tag-matched Deny covers every sandbox: {profile:?}"
            );
            at("*")
        };
        for platform in [Platform::Aws, Platform::Gcp] {
            for mode in ["auto", "extend", "override"] {
                for with_sandboxes in [true, false] {
                    let mut builder = Stack::new("test-stack".to_string())
                        .add(
                            alien_core::Kv::new("cache".to_string()).build(),
                            ResourceLifecycle::Live,
                        )
                        .management(management_permissions_for_test(mode));
                    if with_sandboxes {
                        builder = builder
                            .add(sandbox("live-box"), ResourceLifecycle::Live)
                            .add(sandbox("frozen-box"), ResourceLifecycle::Frozen);
                    }
                    let result_stack = ManagementPermissionProfileMutation
                        .mutate(
                            builder.build(),
                            &StackState::new(platform),
                            &deployment_config_for_management_permission_test(),
                        )
                        .await
                        .expect("management permission mutation should succeed");

                    assert_eq!(
                        guarded_globally(&result_stack.permissions.management),
                        platform == Platform::Aws && with_sandboxes,
                        "{platform:?}, {mode} mode, sandboxes: {with_sandboxes}: {:?}",
                        result_stack.permissions.management
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn every_aws_stack_keeps_management_off_its_own_role() {
        let guard = PermissionSetReference::from_name(MANAGEMENT_ROLE_GUARD);
        for platform in [Platform::Aws, Platform::Gcp] {
            for mode in ["auto", "extend", "override"] {
                for with_a_resource in [true, false] {
                    let mut builder = Stack::new("test-stack".to_string())
                        .management(management_permissions_for_test(mode));
                    if with_a_resource {
                        builder = builder.add(
                            alien_core::Kv::new("cache".to_string()).build(),
                            ResourceLifecycle::Live,
                        );
                    }
                    let result_stack = ManagementPermissionProfileMutation
                        .mutate(
                            builder.build(),
                            &StackState::new(platform),
                            &deployment_config_for_management_permission_test(),
                        )
                        .await
                        .expect("management permission mutation should succeed");

                    let guarded = result_stack
                        .management()
                        .profile()
                        .and_then(|profile| profile.0.get("*"))
                        .is_some_and(|grants| grants.contains(&guard));
                    assert_eq!(
                        guarded,
                        platform == Platform::Aws,
                        "{platform:?}, {mode} mode, resource: {with_a_resource}: {:?}",
                        result_stack.management()
                    );
                }
            }
        }
    }

    #[tokio::test]
    async fn frozen_remote_sandbox_heartbeat_permission_is_added_to_management() {
        let sandbox = Sandbox::new("agents".to_string())
            .code(SandboxCode::Image {
                image: "public.ecr.aws/docker/library/alpine:3.20".to_string(),
            })
            .egress(SandboxEgress::Internet)
            .lifecycle(SandboxLifecyclePolicy {
                max_lifetime_seconds: None,
                idle_pause_seconds: None,
            })
            .build();
        let stack = Stack::new("test-stack".to_string())
            .add_with_remote_access(sandbox, ResourceLifecycle::Frozen)
            .build();
        let stack_state = StackState::new(Platform::Aws);

        let result_stack = ManagementPermissionProfileMutation
            .mutate(
                stack,
                &stack_state,
                &deployment_config_for_management_permission_test(),
            )
            .await
            .expect("management permission mutation should succeed");

        let permissions = result_stack
            .management()
            .profile()
            .expect("auto management profile should be generated");
        let global = permissions
            .0
            .get("*")
            .expect("global management permissions");
        assert!(
            global
                .iter()
                .any(|permission| permission.id() == "sandbox/heartbeat"),
            "a frozen remote-access sandbox must keep its management identity able to heartbeat"
        );
        let granted: Vec<&str> = permissions.0.values().flatten().map(|p| p.id()).collect();
        assert!(
            !granted.contains(&"sandbox/execute") && !granted.contains(&"sandbox/remote-execute"),
            "heartbeat must not drag session execution onto the management identity: {granted:?}"
        );
    }

    /// Only a Frozen GCP sandbox gets template verbs, and only scoped to itself: at `*` they would
    /// bind at project scope and reach every sibling sandbox's template.
    #[tokio::test]
    async fn frozen_gcp_sandbox_gets_template_verbs_scoped_to_itself() {
        let sandbox = |id: &str| {
            Sandbox::new(id.to_string())
                .code(SandboxCode::Image {
                    image: "us-central1-docker.pkg.dev/p/r/agent:1".to_string(),
                })
                .egress(SandboxEgress::Internet)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build()
        };
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let stack = Stack::new("test-stack".to_string())
                .add(sandbox("frozen-box"), ResourceLifecycle::Frozen)
                .add(sandbox("live-box"), ResourceLifecycle::Live)
                .build();
            let result_stack = ManagementPermissionProfileMutation
                .mutate(
                    stack,
                    &StackState::new(platform),
                    &deployment_config_for_management_permission_test(),
                )
                .await
                .expect("management permission mutation should succeed");
            let profile = result_stack
                .management()
                .profile()
                .expect("auto management profile should be generated");
            let has_templates = |scope: &str| {
                profile.0.get(scope).is_some_and(|refs| {
                    refs.iter()
                        .any(|permission| permission.id() == "sandbox/templates")
                })
            };

            assert_eq!(
                has_templates("frozen-box"),
                platform == Platform::Gcp,
                "{platform:?}: {profile:?}"
            );
            assert!(!has_templates("*"), "{platform:?}: {profile:?}");
            assert!(!has_templates("live-box"), "{platform:?}: {profile:?}");
        }
    }

    /// Every Azure sandbox gets disk-image verbs scoped to itself, whatever its lifecycle or
    /// image: at `*` they would bind at resource-group scope and reach every sibling's images.
    #[tokio::test]
    async fn azure_sandbox_gets_disk_image_verbs_scoped_to_itself() {
        let sandbox = |id: &str, image: &str| {
            Sandbox::new(id.to_string())
                .code(SandboxCode::Image {
                    image: image.to_string(),
                })
                .egress(SandboxEgress::Internet)
                .lifecycle(SandboxLifecyclePolicy {
                    max_lifetime_seconds: None,
                    idle_pause_seconds: None,
                })
                .build()
        };
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            let stack = Stack::new("test-stack".to_string())
                .add(sandbox("catalog-box", "ubuntu"), ResourceLifecycle::Frozen)
                .add(
                    sandbox("registry-box", "docker.io/library/python:3.14-slim"),
                    ResourceLifecycle::Live,
                )
                .build();
            let result_stack = ManagementPermissionProfileMutation
                .mutate(
                    stack,
                    &StackState::new(platform),
                    &deployment_config_for_management_permission_test(),
                )
                .await
                .expect("management permission mutation should succeed");
            let profile = result_stack
                .management()
                .profile()
                .expect("auto management profile should be generated");
            let has_images = |scope: &str| {
                profile.0.get(scope).is_some_and(|refs| {
                    refs.iter().any(|permission| {
                        matches!(permission, PermissionSetReference::Name(name) if name == "sandbox/images")
                    })
                })
            };

            for scope in ["catalog-box", "registry-box"] {
                assert_eq!(
                    has_images(scope),
                    platform == Platform::Azure,
                    "{platform:?} {scope}: {profile:?}"
                );
            }
            assert!(!has_images("*"), "{platform:?}: {profile:?}");
        }
    }

    #[tokio::test]
    async fn remote_storage_never_adds_data_access_to_management() {
        for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
            for mode in ["auto", "extend"] {
                let storage = Storage::new("uploads".to_string()).build();
                let stack = Stack::new("test-stack".to_string())
                    .add_with_remote_access(storage, ResourceLifecycle::Frozen)
                    .management(management_permissions_for_test(mode))
                    .build();
                let stack_state = StackState::new(platform);

                let result_stack = ManagementPermissionProfileMutation
                    .mutate(
                        stack,
                        &stack_state,
                        &deployment_config_for_management_permission_test(),
                    )
                    .await
                    .expect("management permission mutation should succeed");

                assert!(
                    !result_stack.management().profile().is_some_and(|profile| profile
                        .0
                        .values()
                        .flatten()
                        .any(|permission| permission.id() == "storage/remote-data-write")),
                    "{platform:?} {mode} must keep remote binding data access off the management identity"
                );
            }
        }
    }

    fn kubernetes_generated_aws_alb_acm_settings() -> StackSettings {
        StackSettings {
            kubernetes: Some(KubernetesSettings {
                cluster: None,
                exposure: Some(KubernetesExposureSettings::Generated {
                    route: aws_alb_route_profile(),
                    certificate: KubernetesCertificateMode::ManagedAcmImport {
                        region: None,
                        tags: Default::default(),
                    },
                }),
            }),
            ..Default::default()
        }
    }

    fn kubernetes_custom_aws_alb_byo_acm_settings() -> StackSettings {
        StackSettings {
            kubernetes: Some(KubernetesSettings {
                cluster: None,
                exposure: Some(KubernetesExposureSettings::Custom {
                    domain: "api.example.com".to_string(),
                    route: aws_alb_route_profile(),
                    certificate: KubernetesCertificateMode::AwsAcmArn {
                        certificate_arn: "arn:aws:acm:us-east-1:123456789012:certificate/customer"
                            .to_string(),
                    },
                }),
            }),
            ..Default::default()
        }
    }

    fn aws_alb_route_profile() -> KubernetesRouteProfile {
        KubernetesRouteProfile::Ingress(KubernetesIngressRouteProfile {
            ingress_class_name: "alb".to_string(),
            provider: Some(KubernetesRouteProviderOptions::AwsAlb {
                scheme: "internet-facing".to_string(),
                target_type: "ip".to_string(),
                ip_address_type: None,
                subnet_ids: Vec::new(),
            }),
            ..Default::default()
        })
    }

    #[tokio::test]
    async fn test_auto_management_profile_generation() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build();
        let storage = Storage::new("test-storage".to_string()).build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );
        resources.insert(
            "test-storage".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(storage),
                lifecycle: ResourceLifecycle::Frozen, // Should get nothing (heartbeat only)
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that management permissions were generated
        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                assert!(profile.0.contains_key("*"));
                let global_permissions = profile.0.get("*").unwrap();

                // Live worker gets provision; storage is frozen so no management.
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&OBSERVE_PERMISSION_SET_ID.to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
                assert!(!permission_names.contains(&"storage/management".to_string()));
                assert!(!permission_names.contains(&"aws/tag-tamper-protection".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn storage_trigger_from_frozen_storage_gets_source_management_permission() {
        let storage = Storage::new("uploads".to_string()).build();
        let worker = Worker::new("processor".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .trigger(WorkerTrigger::storage(
                &storage,
                vec!["created".to_string()],
            ))
            .build();

        let stack = Stack::new("test-stack".to_string())
            .add(storage, ResourceLifecycle::Frozen)
            .add(worker, ResourceLifecycle::Live)
            .management(ManagementPermissions::Auto)
            .build();
        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let result_stack = ManagementPermissionProfileMutation
            .mutate(stack, &stack_state, &config)
            .await
            .unwrap();

        let ManagementPermissions::Extend(profile) = result_stack.management() else {
            panic!("Expected Extend management permissions");
        };
        let global_permissions = profile.0.get("*").expect("global management grants");
        let global_permission_names: Vec<String> = global_permissions
            .iter()
            .map(|perm_ref| perm_ref.id().to_string())
            .collect();
        assert!(global_permission_names.contains(&"worker/provision".to_string()));

        let storage_permissions = profile
            .0
            .get("uploads")
            .expect("storage trigger source management grants");
        let storage_permission_names: Vec<String> = storage_permissions
            .iter()
            .map(|perm_ref| perm_ref.id().to_string())
            .collect();
        assert!(storage_permission_names.contains(&"storage/trigger-management".to_string()));
    }

    #[tokio::test]
    async fn azure_storage_trigger_from_frozen_storage_gets_source_management_permission() {
        let storage = Storage::new("uploads".to_string()).build();
        let worker = Worker::new("processor".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .trigger(WorkerTrigger::storage(
                &storage,
                vec!["created".to_string()],
            ))
            .build();

        let stack = Stack::new("test-stack".to_string())
            .add(storage, ResourceLifecycle::Frozen)
            .add(worker, ResourceLifecycle::Live)
            .management(ManagementPermissions::Auto)
            .build();
        let stack_state = StackState::new(Platform::Azure);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let result_stack = ManagementPermissionProfileMutation
            .mutate(stack, &stack_state, &config)
            .await
            .unwrap();

        let ManagementPermissions::Extend(profile) = result_stack.management() else {
            panic!("Expected Extend management permissions");
        };
        let storage_permissions = profile
            .0
            .get("uploads")
            .expect("Azure Event Grid source management grants");
        assert!(storage_permissions
            .iter()
            .any(|permission| permission.id() == "storage/trigger-management"));
    }

    #[tokio::test]
    async fn kubernetes_cluster_cloud_metadata_heartbeat_is_explicit() {
        let cluster = KubernetesCluster::new("kubernetes".to_string())
            .provider(KubernetesClusterProvider::Eks)
            .ownership(KubernetesClusterOwnership::Managed)
            .namespace("default".to_string())
            .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApiAndCloudMetadata)
            .build();

        let stack = Stack::new("test-stack".to_string())
            .add(cluster, ResourceLifecycle::Frozen)
            .management(ManagementPermissions::Auto)
            .build();
        let stack_state = StackState::new(Platform::Kubernetes);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let result_stack = ManagementPermissionProfileMutation
            .mutate(stack, &stack_state, &config)
            .await
            .unwrap();

        let ManagementPermissions::Extend(profile) = result_stack.management() else {
            panic!("Expected Extend management permissions");
        };
        let global_permission_names: Vec<String> = profile
            .0
            .get("*")
            .into_iter()
            .flatten()
            .map(|perm_ref| perm_ref.id().to_string())
            .collect();
        assert!(
            global_permission_names.is_empty(),
            "KubernetesCluster heartbeat should be resource-scoped because existing clusters do not necessarily use the deployment prefix"
        );
        let permission_names: Vec<String> = profile
            .0
            .get("kubernetes")
            .unwrap()
            .iter()
            .map(|perm_ref| perm_ref.id().to_string())
            .collect();
        assert!(permission_names.contains(&"kubernetes-cluster/heartbeat".to_string()));
        assert!(!permission_names.contains(&"compute-cluster/heartbeat".to_string()));
    }

    #[tokio::test]
    async fn kubernetes_api_only_heartbeat_gets_no_cloud_metadata_permission() {
        let cluster = KubernetesCluster::new("kubernetes".to_string())
            .provider(KubernetesClusterProvider::Generic)
            .ownership(KubernetesClusterOwnership::External)
            .namespace("default".to_string())
            .heartbeat_mode(KubernetesHeartbeatMode::KubernetesApi)
            .build();

        let stack = Stack::new("test-stack".to_string())
            .add(cluster, ResourceLifecycle::Frozen)
            .management(ManagementPermissions::Auto)
            .build();
        let stack_state = StackState::new(Platform::Kubernetes);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let result_stack = ManagementPermissionProfileMutation
            .mutate(stack, &stack_state, &config)
            .await
            .unwrap();

        assert!(
            matches!(result_stack.management(), ManagementPermissions::Auto),
            "an application with no management grants should remain Auto"
        );
    }

    #[tokio::test]
    async fn azure_setup_resource_heartbeats_use_permission_set_names() {
        let stack = Stack::new("test-stack".to_string())
            .add(
                AzureResourceGroup::new("default-resource-group".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                AzureStorageAccount::new("storage-account".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                AzureContainerAppsEnvironment::new("container-apps-env".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                AzureServiceBusNamespace::new("service-bus".to_string()).build(),
                ResourceLifecycle::Frozen,
            )
            .add(
                ServiceActivation::new("enable-storage".to_string())
                    .service_name("Microsoft.Storage".to_string())
                    .build(),
                ResourceLifecycle::Frozen,
            )
            .management(ManagementPermissions::Auto)
            .build();

        let stack_state = StackState::new(Platform::Azure);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let result_stack = ManagementPermissionProfileMutation
            .mutate(stack, &stack_state, &config)
            .await
            .unwrap();

        let ManagementPermissions::Extend(profile) = result_stack.management() else {
            panic!("Expected Extend management permissions");
        };
        let permission_names: Vec<String> = profile
            .0
            .get("*")
            .unwrap()
            .iter()
            .map(|perm_ref| perm_ref.id().to_string())
            .collect();

        for permission_set in [
            "azure-resource-group/heartbeat",
            "azure-storage-account/heartbeat",
            "azure-container-apps-environment/heartbeat",
            "azure-service-bus-namespace/heartbeat",
            "service-activation/heartbeat",
        ] {
            assert!(
                permission_names.contains(&permission_set.to_string()),
                "expected generated management profile to include {permission_set}"
            );
        }
        assert!(!permission_names.iter().any(|permission| {
            permission.contains("azure_") || permission.contains("service_activation")
        }));
    }

    #[tokio::test]
    async fn kubernetes_generated_aws_alb_public_worker_gets_acm_permission() {
        let worker = Worker::new("api".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .public_endpoint(WorkerPublicEndpoint {
                name: "api".to_string(),
                host_label: None,
                wildcard_subdomains: false,
            })
            .build();

        let stack = Stack::new("test-stack".to_string())
            .add(worker, ResourceLifecycle::Live)
            .management(ManagementPermissions::Auto)
            .build();
        let stack_state = StackState::new(Platform::Kubernetes);
        let config = DeploymentConfig::builder()
            .stack_settings(kubernetes_generated_aws_alb_acm_settings())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let result_stack = ManagementPermissionProfileMutation
            .mutate(stack, &stack_state, &config)
            .await
            .unwrap();

        let ManagementPermissions::Extend(profile) = result_stack.management() else {
            panic!("Expected Extend management permissions");
        };
        let permission_names: Vec<String> = profile
            .0
            .get("api")
            .unwrap()
            .iter()
            .map(|perm_ref| perm_ref.id().to_string())
            .collect();

        assert!(permission_names.contains(&"kubernetes-public-endpoint/management".to_string()));
    }

    #[tokio::test]
    async fn kubernetes_byo_acm_public_worker_gets_no_managed_acm_permission() {
        let worker = Worker::new("api".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .public_endpoint(WorkerPublicEndpoint {
                name: "api".to_string(),
                host_label: None,
                wildcard_subdomains: false,
            })
            .build();

        let stack = Stack::new("test-stack".to_string())
            .add(worker, ResourceLifecycle::Live)
            .management(ManagementPermissions::Auto)
            .build();
        let stack_state = StackState::new(Platform::Kubernetes);
        let config = DeploymentConfig::builder()
            .stack_settings(kubernetes_custom_aws_alb_byo_acm_settings())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let result_stack = ManagementPermissionProfileMutation
            .mutate(stack, &stack_state, &config)
            .await
            .unwrap();

        let ManagementPermissions::Extend(profile) = result_stack.management() else {
            panic!("Expected Extend management permissions");
        };
        assert!(
            !profile.0.contains_key("api"),
            "BYO ACM should not add Kubernetes managed ACM permissions"
        );
    }

    #[tokio::test]
    async fn test_extend_management_profile_merge() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        // Create an extend profile with additional permissions
        let extend_profile = PermissionProfile::new().global(["storage/data-write"]);

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Extend(extend_profile),
            },
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that both auto-generated and extended permissions are present
        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should have auto-generated live provision and extended storage/data-write.
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
                assert!(permission_names.contains(&"storage/data-write".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_override_management_profile_is_unchanged() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        // Create an override profile
        let override_profile =
            PermissionProfile::new().global(["storage/management", "worker/management"]);

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Override(override_profile.clone()),
            },
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Override profiles are authored explicitly; only the role guards, which can only narrow the
        // identity, are added.
        match result_stack.management() {
            ManagementPermissions::Override(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                assert_eq!(global_permissions.len(), 3);

                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                assert!(permission_names.contains(&"storage/management".to_string()));
                assert!(permission_names.contains(&"worker/management".to_string()));
                assert!(permission_names.contains(&MANAGEMENT_ROLE_GUARD.to_string()));
                // Should NOT have auto-generated worker/provision
                assert!(!permission_names.contains(&"worker/provision".to_string()));
            }
            _ => panic!("Expected Override management permissions"),
        }
    }

    #[tokio::test]
    async fn application_stack_cannot_request_account_wide_observation() {
        let stack = Stack::new("test-stack".to_string())
            .management(ManagementPermissions::Extend(
                PermissionProfile::new().global([OBSERVE_PERMISSION_SET_ID]),
            ))
            .build();
        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let error = ManagementPermissionProfileMutation
            .mutate(stack, &stack_state, &config)
            .await
            .expect_err("application observation grant should be rejected");

        assert!(error
            .to_string()
            .contains("dedicated observe-only deployment"));
    }

    #[tokio::test]
    async fn test_pull_model_permissions() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build();
        let storage = Storage::new("test-storage".to_string()).build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );
        resources.insert(
            "test-storage".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(storage),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        // Create stack state with Pull model. Permission derivation is model
        // independent: the credentials are attached differently, but the
        // resource operations required by Alien are the same.
        let stack_settings = StackSettings {
            deployment_model: DeploymentModel::Pull,
            ..Default::default()
        };
        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(stack_settings)
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                assert!(profile.0.contains_key("*"));
                let global_permissions = profile.0.get("*").unwrap();

                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should contain heartbeat permissions for both resources
                assert!(permission_names.contains(&"worker/heartbeat".to_string()));
                assert!(permission_names.contains(&"storage/heartbeat".to_string()));

                // Should contain live resource mutation permissions in both
                // Pull and Push models.
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
                assert!(!permission_names.contains(&"storage/management".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_live_container_and_frozen_cluster_permissions() {
        let container = Container::new("web".to_string())
            .code(ContainerCode::Image {
                image: "test:latest".to_string(),
            })
            .cpu(ResourceSpec {
                min: "0.5".to_string(),
                desired: "1".to_string(),
            })
            .memory(ResourceSpec {
                min: "512Mi".to_string(),
                desired: "1Gi".to_string(),
            })
            .port(8080)
            .permissions("test".to_string())
            .build();
        let cluster = ComputeCluster::new("compute".to_string())
            .capacity_group(CapacityGroup {
                group_id: "general".to_string(),
                instance_type: Some("m7g.large".to_string()),
                profile: None,
                min_size: 1,
                max_size: 3,
                scale_policy: None,
                nested_virtualization: None,
            })
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "web".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(container),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );
        resources.insert(
            "compute".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(cluster),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let permission_names: Vec<String> = profile
                    .0
                    .get("*")
                    .unwrap()
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                assert!(permission_names.contains(&"container/provision".to_string()));
                assert!(!permission_names.contains(&"container/management".to_string()));
                assert!(permission_names.contains(&"compute-cluster/management".to_string()));
                assert!(!permission_names.contains(&"compute-cluster/provision".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_frozen_artifact_registry_gets_management_permissions() {
        let registry = ArtifactRegistry::new("registry".to_string()).build();

        let mut resources = IndexMap::new();
        resources.insert(
            "registry".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(registry),
                lifecycle: ResourceLifecycle::Frozen,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let permission_names: Vec<String> = profile
                    .0
                    .get("*")
                    .unwrap()
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                assert!(permission_names.contains(&"artifact-registry/management".to_string()));
                assert!(!permission_names.contains(&"artifact-registry/provision".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_push_model_permissions() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        // Create stack state with Push model (Manager deploys remotely)
        let stack_settings = StackSettings {
            deployment_model: DeploymentModel::Push,
            ..Default::default()
        };
        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(stack_settings)
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that both heartbeat and provision permissions were generated
        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should contain heartbeat plus live mutation permissions.
                assert!(permission_names.contains(&"worker/heartbeat".to_string()));
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_commands_enabled_function_permissions() {
        // Test AWS platform - should add resource-scoped worker/dispatch-command.
        let arc_function = Worker::new("arc-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .commands_enabled(true)
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "arc-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(arc_function),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        // Check that Commands permissions were added
        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should have live resource permissions globally; command dispatch is resource-scoped.
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
                assert!(!permission_names.contains(&"worker/dispatch-command".to_string()));

                let worker_permissions = profile.0.get("arc-worker").unwrap();
                let worker_permission_names: Vec<String> = worker_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();
                assert!(worker_permission_names.contains(&"worker/dispatch-command".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }

        // Test GCP platform - should author an explicit command dispatch grant.
        let stack_state_gcp = StackState::new(Platform::Gcp);
        let config_gcp = DeploymentConfig::builder()
            .stack_settings(StackSettings::default())
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();
        let stack_gcp = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack-gcp".to_string(),
            resources: {
                let mut resources = IndexMap::new();
                let arc_function = Worker::new("arc-worker".to_string())
                    .code(WorkerCode::Image {
                        image: "test:latest".to_string(),
                    })
                    .permissions("test".to_string())
                    .commands_enabled(true)
                    .build();
                resources.insert(
                    "arc-worker".to_string(),
                    ResourceEntry {
                        config: alien_core::Resource::new(arc_function),
                        lifecycle: ResourceLifecycle::Live,
                        dependencies: Vec::new(),
                        remote_access: false,
                        enabled_when: None,
                    },
                );
                resources
            },
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        let result_stack_gcp = mutation
            .mutate(stack_gcp, &stack_state_gcp, &config_gcp)
            .await
            .unwrap();

        match result_stack_gcp.management() {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should have live resource permissions globally; command dispatch is resource-scoped.
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
                assert!(!permission_names.contains(&"worker/dispatch-command".to_string()));

                let worker_permissions = profile.0.get("arc-worker").unwrap();
                let worker_permission_names: Vec<String> = worker_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();
                assert!(worker_permission_names.contains(&"worker/dispatch-command".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_disabled_heartbeat_no_permissions() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        // Disable heartbeat
        let stack_settings = StackSettings {
            heartbeats: HeartbeatsMode::Off,
            ..Default::default()
        };
        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(stack_settings)
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should NOT contain heartbeat permissions since heartbeat is disabled
                assert!(!permission_names.contains(&"worker/heartbeat".to_string()));
                // Should still have live mutation permissions.
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_disabled_telemetry_no_permissions() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        // Disable telemetry
        let stack_settings = StackSettings {
            telemetry: TelemetryMode::Off,
            ..Default::default()
        };
        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(stack_settings)
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should NOT contain telemetry permissions since telemetry is disabled
                assert!(!permission_names.contains(&"worker/telemetry".to_string()));
                // Should still have heartbeat and live mutation permissions.
                assert!(permission_names.contains(&"worker/heartbeat".to_string()));
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }

    #[tokio::test]
    async fn test_requires_approval_still_creates_permissions() {
        let worker = Worker::new("test-worker".to_string())
            .code(WorkerCode::Image {
                image: "test:latest".to_string(),
            })
            .permissions("test".to_string())
            .build();

        let mut resources = IndexMap::new();
        resources.insert(
            "test-worker".to_string(),
            ResourceEntry {
                config: alien_core::Resource::new(worker),
                lifecycle: ResourceLifecycle::Live,
                dependencies: Vec::new(),
                remote_access: false,
                enabled_when: None,
            },
        );

        let stack = Stack {
            dynamic_container_repositories: Vec::new(),
            dynamic_container_image_resources: Vec::new(),
            operations: None,
            id: "test-stack".to_string(),
            resources,
            permissions: PermissionsConfig {
                profiles: IndexMap::new(),
                management: ManagementPermissions::Auto,
            },
            supported_platforms: None,
            inputs: vec![],
        };

        // Require approval for telemetry - permissions should still be created
        // Note: ApprovalRequired is a runtime check, permissions are infrastructure.
        // When telemetry is ApprovalRequired (not Off), permissions should be created
        // for telemetry collection. The approval gate happens at runtime.
        let stack_settings = StackSettings {
            telemetry: TelemetryMode::ApprovalRequired,
            ..Default::default()
        };
        let stack_state = StackState::new(Platform::Aws);
        let config = DeploymentConfig::builder()
            .stack_settings(stack_settings)
            .environment_variables(empty_env_snapshot())
            .allow_frozen_changes(false)
            .external_bindings(ExternalBindings::default())
            .build();

        let mutation = ManagementPermissionProfileMutation;
        let result_stack = mutation.mutate(stack, &stack_state, &config).await.unwrap();

        match result_stack.management() {
            ManagementPermissions::Extend(profile) => {
                let global_permissions = profile.0.get("*").unwrap();
                let permission_names: Vec<String> = global_permissions
                    .iter()
                    .map(|perm_ref| perm_ref.id().to_string())
                    .collect();

                // Should contain heartbeat permissions (heartbeat is On by default)
                assert!(permission_names.contains(&"worker/heartbeat".to_string()));
                // Should contain live mutation permissions.
                assert!(permission_names.contains(&"worker/provision".to_string()));
                assert!(!permission_names.contains(&"worker/management".to_string()));
                // Note: worker/telemetry permission set may not exist in registry,
                // but the code attempts to add it. If it exists, it would be added.
            }
            _ => panic!("Expected Extend management permissions"),
        }
    }
}
