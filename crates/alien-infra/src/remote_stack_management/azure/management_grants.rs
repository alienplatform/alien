use super::*;

pub(crate) fn get_management_identity_name(prefix: &str) -> String {
    format!("{}-management-identity", prefix)
}

pub(crate) fn get_fic_name(prefix: &str) -> String {
    format!("{}-management-fic", prefix)
}

pub(crate) fn management_role_definition_scope(
    assignable_scopes: &[String],
    subscription_id: &str,
    resource_group_name: &str,
) -> Scope {
    let subscription_scope = format!("/subscriptions/{subscription_id}");
    if assignable_scopes
        .iter()
        .any(|scope| scope == &subscription_scope)
    {
        Scope::Subscription
    } else {
        Scope::ResourceGroup {
            resource_group_name: resource_group_name.to_string(),
        }
    }
}

pub(crate) fn role_definition_scope_from_id(
    role_definition_id: &str,
    resource_group_name: &str,
) -> Scope {
    if role_definition_id.contains("/resourceGroups/") {
        Scope::ResourceGroup {
            resource_group_name: resource_group_name.to_string(),
        }
    } else {
        Scope::Subscription
    }
}

pub(crate) fn current_unix_timestamp_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(crate) fn ensure_wait_deadline(wait_until_epoch_secs: &mut Option<u64>, wait_secs: u64) -> u64 {
    let now = current_unix_timestamp_secs();
    *wait_until_epoch_secs.get_or_insert_with(|| now.saturating_add(wait_secs))
}

pub(crate) fn wait_delay(deadline_epoch_secs: u64) -> Option<Duration> {
    let now = current_unix_timestamp_secs();
    let remaining = deadline_epoch_secs.saturating_sub(now);

    if remaining == 0 {
        None
    } else {
        Some(Duration::from_secs(
            remaining.min(AZURE_RBAC_WAIT_POLL_SECS),
        ))
    }
}

pub(crate) fn management_role_assignment_key(
    resource_prefix: &str,
    principal_id: &str,
    role_definition_id: &str,
    scope: &str,
) -> String {
    format!(
        "deployment:azure:mgmt-role-assign:{resource_prefix}:uami:{principal_id}:{role_definition_id}:{scope}"
    )
}

pub(crate) fn resource_role_definition_key(custom_role_key: &str, scope: &str) -> String {
    format!("{custom_role_key}:{scope}")
}

pub(crate) fn existing_role_assignment_id_from_conflict(
    scope: &str,
    err: &AlienError<CloudClientErrorData>,
) -> Option<String> {
    let CloudClientErrorData::RemoteResourceConflict { message, .. } = err.error.as_ref()? else {
        return None;
    };

    let lower_message = message.to_ascii_lowercase();
    if !lower_message.contains("role assignment already exists")
        || !lower_message.contains("role assignment")
    {
        return None;
    }

    let normalized = message
        .chars()
        .map(|ch| {
            if ch.is_ascii_hexdigit() || ch == '-' {
                ch
            } else {
                ' '
            }
        })
        .collect::<String>();

    normalized
        .split_whitespace()
        .rev()
        .find_map(|candidate| Uuid::parse_str(candidate).ok())
        .map(|assignment_uuid| {
            format!(
                "{}/providers/Microsoft.Authorization/roleAssignments/{}",
                scope, assignment_uuid
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::{PermissionProfile, PermissionSetReference};

    fn permission_context() -> PermissionContext {
        PermissionContext::new()
            .with_subscription_id("sub-123".to_string())
            .with_resource_group("rg-123".to_string())
            .with_stack_prefix("e2e-01-azcr".to_string())
            .with_managing_subscription_id("sub-123".to_string())
            .with_managing_resource_group("rg-123".to_string())
    }

    #[test]
    fn role_assignment_conflict_parser_extracts_existing_assignment_id() {
        let err = AlienError::new(CloudClientErrorData::RemoteResourceConflict {
            resource_type: "Resource".to_string(),
            resource_name: "roleAssignments/requested".to_string(),
            message: "The role assignment already exists. The ID of the conflicting role assignment is 593d47719b195096804b7b96d6e5a5ac.".to_string(),
        });

        let existing_assignment_id = existing_role_assignment_id_from_conflict(
            "/subscriptions/sub-123/resourceGroups/rg-123",
            &err,
        )
        .expect("conflict should include an existing role assignment id");

        assert_eq!(
            existing_assignment_id,
            "/subscriptions/sub-123/resourceGroups/rg-123/providers/Microsoft.Authorization/roleAssignments/593d4771-9b19-5096-804b-7b96d6e5a5ac"
        );
    }

    #[test]
    fn management_role_assignment_key_includes_azure_immutable_fields() {
        let prefix = "e2e-03-azcr";
        let principal_id = "principal-a";
        let role_definition_id = "/subscriptions/sub-123/providers/Microsoft.Authorization/roleDefinitions/acdd72a7-3385-48ef-bd42-f606fba81ae7";
        let scope = "/subscriptions/sub-123/resourceGroups/rg-123";
        let base_key =
            management_role_assignment_key(prefix, principal_id, role_definition_id, scope);

        assert_ne!(
            base_key,
            management_role_assignment_key(prefix, "principal-b", role_definition_id, scope),
            "Azure rejects updating principalId on an existing role assignment ID"
        );
        assert_ne!(
            base_key,
            management_role_assignment_key(
                prefix,
                principal_id,
                "/subscriptions/sub-123/providers/Microsoft.Authorization/roleDefinitions/custom-role",
                scope,
            ),
            "Azure rejects updating roleDefinitionId on an existing role assignment ID"
        );
        assert_ne!(
            base_key,
            management_role_assignment_key(
                prefix,
                principal_id,
                role_definition_id,
                "/subscriptions/sub-123",
            ),
            "Azure rejects updating scope on an existing role assignment ID"
        );
    }

    #[test]
    fn stack_management_grant_plan_includes_global_heartbeat_reader_grants() {
        let profile = PermissionProfile::new().global([
            PermissionSetReference::from_name("worker/provision"),
            PermissionSetReference::from_name("storage/provision"),
            PermissionSetReference::from_name("artifact-registry/provision"),
            PermissionSetReference::from_name("azure-resource-group/heartbeat"),
            PermissionSetReference::from_name("service-account/heartbeat"),
        ]);

        let grant_plan =
            generate_stack_management_grant_plan(&profile, &permission_context()).unwrap();

        assert!(
            grant_plan.custom_roles.iter().any(|role| role
                .role_definition
                .actions
                .iter()
                .any(|action| { action == "Microsoft.App/containerApps/write" })),
            "worker/provision should still contribute residual Azure management actions"
        );
        assert_eq!(
            grant_plan
                .bindings
                .iter()
                .filter(|binding| matches!(
                    binding.role_definition,
                    AzureRoleDefinitionRef::Custom { .. }
                ))
                .count(),
            1,
            "all residual custom management actions share one combined custom role assignment"
        );

        let reader_bindings: Vec<_> = grant_plan
            .bindings
            .iter()
            .filter(|binding| {
                matches!(
                    &binding.role_definition,
                    AzureRoleDefinitionRef::Predefined { role_definition_id }
                        if role_definition_id.ends_with("/acdd72a7-3385-48ef-bd42-f606fba81ae7")
                )
            })
            .collect();

        assert_eq!(
            reader_bindings.len(),
            1,
            "resource-group and service-account heartbeats should share one deduped Reader assignment"
        );
        assert_eq!(
            reader_bindings[0].scope,
            "/subscriptions/sub-123/resourceGroups/rg-123"
        );
    }

    #[test]
    fn stack_management_grant_plan_includes_worker_dispatch_command_once() {
        let profile = PermissionProfile::new()
            .resource(
                "api",
                [PermissionSetReference::from_name("worker/dispatch-command")],
            )
            .resource(
                "jobs",
                [PermissionSetReference::from_name("worker/dispatch-command")],
            );

        let grant_plan =
            generate_stack_management_grant_plan(&profile, &permission_context()).unwrap();

        assert_eq!(
            grant_plan
                .bindings
                .iter()
                .filter(|binding| binding.permission_set_id == "worker/dispatch-command")
                .count(),
            1,
            "worker dispatch is a stack management transport grant and should be deduped"
        );
    }
}

pub(crate) fn emit_azure_remote_stack_management_heartbeat(
    ctx: &ResourceControllerContext<'_>,
    controller: &AzureRemoteStackManagementController,
) {
    let resource_id = ctx
        .desired_resource_config::<RemoteStackManagement>()
        .map(|config| config.id.clone())
        .unwrap_or_else(|_| "remote-stack-management".to_string());

    ctx.emit_heartbeat(ResourceHeartbeat {
        deployment_id: None,
        resource_id,
        resource_type: RemoteStackManagement::RESOURCE_TYPE,
        controller_platform: Platform::Azure,
        backend: HeartbeatBackend::Azure,
        observed_at: Utc::now(),
        data: ResourceHeartbeatData::RemoteStackManagement(
            RemoteStackManagementHeartbeatData::AzureManagedIdentity(
                AzureRemoteStackManagementHeartbeatData {
                    status: RemoteStackManagementHeartbeatStatus {
                        health: ObservedHealth::Healthy,
                        lifecycle: ProviderLifecycleState::Running,
                        message: controller.uami_resource_id.as_ref().map(|resource_id| {
                            format!("Azure management identity '{}' is reachable", resource_id)
                        }),
                        stale: false,
                        partial: false,
                        collection_issues: vec![],
                    },
                    uami_resource_id: controller.uami_resource_id.clone(),
                    uami_client_id: controller.uami_client_id.clone(),
                    uami_principal_id: controller.uami_principal_id.clone(),
                    tenant_id: controller.tenant_id.clone(),
                    fic_name: controller.fic_name.clone(),
                    role_definition_id: controller.role_definition_id.clone(),
                    role_assignment_ids: controller.role_assignment_ids.clone(),
                },
            ),
        ),
        raw: vec![],
    });
}

pub(crate) fn existing_vnet_reader_assignment_key(
    resource_prefix: &str,
    principal_kind: &str,
    principal_id: &str,
    vnet_resource_id: &str,
) -> String {
    format!(
        "deployment:azure:existing-vnet-reader:{resource_prefix}:{principal_kind}:{principal_id}:{vnet_resource_id}"
    )
}

pub(crate) fn existing_azure_vnet_resource_id(
    ctx: &ResourceControllerContext<'_>,
) -> Option<String> {
    match ctx.deployment_config.stack_settings.network.as_ref()? {
        NetworkSettings::ByoVnetAzure {
            vnet_resource_id, ..
        } => Some(vnet_resource_id.clone()),
        _ => None,
    }
}

pub(crate) fn generate_stack_management_grant_plan(
    management_profile: &PermissionProfile,
    permission_context: &PermissionContext,
) -> Result<AzureGrantPlan> {
    let mut custom_roles = Vec::new();
    let mut bindings = Vec::new();
    let generator = AzureRuntimePermissionsGenerator::new();

    if let Some(global_refs) = management_profile.0.get("*") {
        for permission_set_ref in global_refs {
            let Some(permission_set) =
                permission_set_ref.resolve(|name| get_permission_set(name).cloned())
            else {
                tracing::warn!(
                    permission_set_id = %permission_set_ref.id(),
                    "Management permission set not found, skipping"
                );
                continue;
            };
            if permission_set.platforms.azure.is_none() {
                continue;
            }

            let grant_plan = generator
                .generate_grant_plan(&permission_set, BindingTarget::Stack, permission_context)
                .context(ErrorData::InfrastructureError {
                    message: format!(
                        "Failed to generate Azure role definition for permission set '{}'",
                        permission_set.id
                    ),
                    operation: Some("generate_management_grant_plan".to_string()),
                    resource_id: Some("management".to_string()),
                })?;

            custom_roles.extend(grant_plan.custom_roles);
            bindings.extend(grant_plan.bindings);
        }
    }

    let mut seen_stack_management_refs = BTreeSet::new();
    for permission_set_ref in management_profile
        .0
        .iter()
        .filter(|(scope, _)| scope.as_str() != "*")
        .flat_map(|(_, refs)| refs.iter())
        .filter(|reference| reference.id() == "worker/dispatch-command")
    {
        let Some(permission_set) =
            permission_set_ref.resolve(|name| get_permission_set(name).cloned())
        else {
            tracing::warn!(
                permission_set_id = %permission_set_ref.id(),
                "Management permission set not found, skipping"
            );
            continue;
        };
        if !seen_stack_management_refs.insert(permission_set.id.clone()) {
            continue;
        }
        if permission_set.platforms.azure.is_none() {
            continue;
        }

        let grant_plan = generator
            .generate_grant_plan(&permission_set, BindingTarget::Stack, permission_context)
            .context(ErrorData::InfrastructureError {
                message: format!(
                    "Failed to generate Azure role definition for permission set '{}'",
                    permission_set.id
                ),
                operation: Some("generate_management_grant_plan".to_string()),
                resource_id: Some("management".to_string()),
            })?;

        custom_roles.extend(grant_plan.custom_roles);
        bindings.extend(grant_plan.bindings);
    }

    Ok(AzureGrantPlan {
        custom_roles,
        bindings: dedupe_management_role_bindings(bindings),
    })
}

fn dedupe_management_role_bindings(
    bindings: Vec<alien_permissions::generators::AzureRoleBinding>,
) -> Vec<alien_permissions::generators::AzureRoleBinding> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();

    for binding in bindings {
        let role_key = match &binding.role_definition {
            AzureRoleDefinitionRef::Predefined { role_definition_id } => {
                format!("predefined:{role_definition_id}")
            }
            AzureRoleDefinitionRef::Custom { .. } => "combined-custom-management-role".to_string(),
        };

        if seen.insert((binding.scope.clone(), role_key)) {
            deduped.push(binding);
        }
    }

    deduped
}

impl AzureRemoteStackManagementController {
    pub(super) fn desired_remote_storage_scopes(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Vec<String>> {
        desired_remote_storage_scopes(ctx)
    }

    pub(super) async fn delete_resource_role_definitions(
        &mut self,
        client: &std::sync::Arc<dyn alien_azure_clients::authorization::AuthorizationApi>,
        resource_group_name: &str,
        config_id: &str,
    ) -> Result<()> {
        super::super::azure_remote_storage::delete_resource_role_definitions(
            self,
            client,
            resource_group_name,
            config_id,
        )
        .await
    }

    pub(super) async fn create_remote_storage_role_definitions(
        &mut self,
        ctx: &ResourceControllerContext<'_>,
        client: &std::sync::Arc<dyn alien_azure_clients::authorization::AuthorizationApi>,
        azure_cfg: &alien_azure_clients::AzureClientConfig,
        resource_group_name: &str,
        config_id: &str,
    ) -> Result<()> {
        super::super::azure_remote_storage::create_remote_storage_role_definitions(
            self,
            ctx,
            client,
            azure_cfg,
            resource_group_name,
            config_id,
        )
        .await
    }

    /// Generate management role definition properties from /provision permission sets
    pub(super) fn generate_management_role_definition(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<Option<RoleDefinitionProperties>> {
        let grant_plan = self.generate_management_grant_plan(ctx)?;
        let custom_roles = custom_roles_for_combined_management_role(grant_plan);
        if custom_roles.is_empty() {
            return Ok(None);
        }

        let mut combined_actions = Vec::new();
        let mut combined_data_actions = Vec::new();
        let mut assignable_scopes = Vec::new();

        for custom_role in custom_roles {
            combined_actions.extend(custom_role.role_definition.actions);
            combined_data_actions.extend(custom_role.role_definition.data_actions);
            assignable_scopes.extend(custom_role.role_definition.assignable_scopes);
        }

        combined_actions.sort();
        combined_actions.dedup();
        combined_data_actions.sort();
        combined_data_actions.dedup();
        assignable_scopes.sort();
        assignable_scopes.dedup();

        let role_name = format!("{}-management-role", ctx.resource_prefix);
        let description = match ctx.deployment_name_for_metadata() {
            Some(deployment_name) => format!(
                "Management role for {deployment_name}. Resource prefix: {}.",
                ctx.resource_prefix
            ),
            None => format!("Management role. Resource prefix: {}.", ctx.resource_prefix),
        };

        Ok(Some(RoleDefinitionProperties {
            role_name: Some(role_name),
            description: Some(description),
            type_: Some("CustomRole".to_string()),
            permissions: vec![Permission {
                actions: combined_actions,
                not_actions: vec![],
                data_actions: combined_data_actions,
                not_data_actions: vec![],
            }],
            assignable_scopes,
            ..Default::default()
        }))
    }

    pub(super) fn generate_management_grant_plan(
        &self,
        ctx: &ResourceControllerContext<'_>,
    ) -> Result<AzureGrantPlan> {
        super::super::azure_remote_storage::generate_management_grant_plan(ctx)
    }

    pub(super) async fn create_role_assignment_by_scope(
        &mut self,
        client: &std::sync::Arc<dyn alien_azure_clients::authorization::AuthorizationApi>,
        assignment_uuid: &str,
        principal_id: &str,
        role_definition_id: &str,
        scope: &str,
        description: &str,
        config_id: &str,
    ) -> Result<()> {
        let full_assignment_id = format!(
            "{}/providers/Microsoft.Authorization/roleAssignments/{}",
            scope, assignment_uuid
        );

        let role_assignment = RoleAssignment {
            id: None,
            name: None,
            type_: None,
            properties: Some(RoleAssignmentProperties {
                principal_id: principal_id.to_string(),
                role_definition_id: role_definition_id.to_string(),
                scope: Some(scope.to_string()),
                principal_type: RoleAssignmentPropertiesPrincipalType::ServicePrincipal,
                description: Some(description.to_string()),
                condition: None,
                condition_version: None,
                created_by: None,
                created_on: None,
                delegated_managed_identity_resource_id: None,
                updated_by: None,
                updated_on: None,
            }),
        };

        let create_result = client
            .create_or_update_role_assignment_by_id(full_assignment_id.clone(), &role_assignment)
            .await;

        if let Err(err) = create_result {
            if let Some(existing_assignment_id) =
                existing_role_assignment_id_from_conflict(scope, &err)
            {
                info!(
                    assignment_id = %existing_assignment_id,
                    requested_assignment_id = %full_assignment_id,
                    principal_id = %principal_id,
                    role_definition_id = %role_definition_id,
                    "Role assignment already exists"
                );
                self.role_assignment_ids.push(existing_assignment_id);
                return Ok(());
            }

            return Err(err.context(ErrorData::CloudPlatformError {
                message: format!("Failed to create role assignment for {}", description),
                resource_id: Some(config_id.to_string()),
            }));
        }

        info!(
            assignment_id = %full_assignment_id,
            principal_id = %principal_id,
            "Role assignment created"
        );

        self.role_assignment_ids.push(full_assignment_id);
        Ok(())
    }

    #[cfg(feature = "test-utils")]
    pub fn mock_ready(prefix: &str) -> Self {
        Self {
            state: AzureRemoteStackManagementState::Ready,
            uami_resource_id: Some(format!(
                "/subscriptions/sub-1234/resourceGroups/test-rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{}-management-identity",
                prefix
            )),
            uami_client_id: Some("12345678-1234-1234-1234-123456789012".to_string()),
            uami_principal_id: Some("87654321-4321-4321-4321-210987654321".to_string()),
            tenant_id: Some("tenant-1234".to_string()),
            fic_name: Some(format!("{}-management-fic", prefix)),
            role_definition_id: Some(format!(
                "/subscriptions/sub-1234/providers/Microsoft.Authorization/roleDefinitions/{}-mgmt-role",
                prefix
            )),
            resource_role_definition_ids: HashMap::new(),
            role_assignment_ids: vec![],
            role_assignment_wait_until_epoch_secs: None,
            applied_management_grant_fingerprint: None,
            _internal_stay_count: None,
        }
    }
}
