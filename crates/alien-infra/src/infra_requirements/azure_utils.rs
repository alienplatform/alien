use crate::error::{ErrorData, Result};
use alien_core::{
    AzureContainerAppsEnvironment, AzureContainerAppsEnvironmentOutputs, AzureResourceGroup,
    AzureResourceGroupOutputs, AzureStorageAccount, AzureStorageAccountOutputs, ResourceRef,
    StackState,
};
use alien_error::{AlienError, Context};

/// Helper to extract the Azure Resource Group name from the dependency outputs.
/// This is used by all Azure resource controllers.
pub fn get_resource_group_name(state: &StackState) -> Result<String> {
    let rg_ref = ResourceRef::new(AzureResourceGroup::RESOURCE_TYPE, "default-resource-group");
    let rg_outputs = state
        .get_resource_outputs::<AzureResourceGroupOutputs>(rg_ref.id())
        .context(ErrorData::DependencyNotReady {
            resource_id: "default-resource-group".to_string(),
            dependency_id: rg_ref.id().to_string(),
        })?;
    Ok(rg_outputs.name.clone())
}

/// Helper to extract the Azure Storage Account name from the dependency outputs.
/// This is used by Azure resource controllers that depend on a storage account.
pub fn get_storage_account_name(state: &StackState) -> Result<String> {
    let sa_ref = ResourceRef::new(
        AzureStorageAccount::RESOURCE_TYPE,
        "default-storage-account",
    );
    let sa_outputs = state
        .get_resource_outputs::<AzureStorageAccountOutputs>(sa_ref.id())
        .context(ErrorData::DependencyNotReady {
            resource_id: "default-storage-account".to_string(),
            dependency_id: sa_ref.id().to_string(),
        })?;
    Ok(sa_outputs.account_name.clone())
}

/// Helper to extract Azure Container Apps Environment outputs from the dependency outputs.
pub fn get_container_apps_environment_outputs(
    state: &StackState,
) -> Result<&AzureContainerAppsEnvironmentOutputs> {
    let env_ref = ResourceRef::new(
        AzureContainerAppsEnvironment::RESOURCE_TYPE,
        "default-container-env",
    );
    state
        .get_resource_outputs::<AzureContainerAppsEnvironmentOutputs>(env_ref.id())
        .context(ErrorData::DependencyNotReady {
            resource_id: "default-container-env".to_string(),
            dependency_id: env_ref.id().to_string(),
        })
}

/// Helper to extract the Azure Container Apps Environment name from the dependency outputs.
/// This is used by Azure resource controllers that depend on a container apps environment.
pub fn get_container_apps_environment_name(state: &StackState) -> Result<String> {
    Ok(get_container_apps_environment_outputs(state)?
        .environment_name
        .clone())
}

/// Helper to extract the full Azure resource ID of the Container Apps Environment.
/// This should be used instead of constructing the resource ID manually, since the
/// environment may be in a different resource group than the stack (e.g., when using
/// external bindings for a pre-provisioned shared environment).
pub fn get_container_apps_environment_resource_id(state: &StackState) -> Result<String> {
    Ok(get_container_apps_environment_outputs(state)?
        .resource_id
        .clone())
}

/// Add the identities and the environment the stack's Builds use to `context`, so a stack-wide
/// `build/execute` grant binds on those resources and no others. The identities are the ones
/// the service accounts get (`<prefix>-<profile>-sa`). The environment is read from state when it
/// exists (an externally provisioned one is there from the start); until then it is the one this
/// stack creates, named the way its controller names it. A stack with no Build adds nothing.
pub fn with_build_scopes(
    context: alien_permissions::PermissionContext,
    stack: &alien_core::Stack,
    state: &StackState,
    resource_prefix: &str,
    subscription_id: &str,
    resource_group: &str,
) -> alien_permissions::PermissionContext {
    let mut profiles: Vec<String> = stack
        .resources()
        .filter_map(|(_id, entry)| entry.config.downcast_ref::<alien_core::Build>())
        .map(|build| build.get_permissions().to_string())
        .collect();
    if profiles.is_empty() {
        return context;
    }
    profiles.sort();
    profiles.dedup();
    let group = azure_resource_group_resource_id(subscription_id, resource_group);
    let mut context = context;
    for profile in profiles {
        context = context.with_build_identity_id(format!(
            "{group}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{resource_prefix}-{profile}-sa"
        ));
    }
    let environment_id = get_container_apps_environment_resource_id(state).unwrap_or_else(|_| {
        azure_container_apps_environment_resource_id(
            subscription_id,
            resource_group,
            &super::generate_container_apps_environment_name(
                resource_prefix,
                "default-container-env",
            ),
        )
    });
    context.with_build_environment_id(environment_id)
}

/// True while a Build's join grant would name an environment that does not exist yet: the stack
/// has a Build and an environment of its own, and the environment has no outputs in state.
/// Service accounts have no dependencies, so their controller waits on this instead.
pub fn build_environment_pending(stack: &alien_core::Stack, state: &StackState) -> bool {
    let has_build = stack
        .resources()
        .any(|(_id, entry)| entry.config.downcast_ref::<alien_core::Build>().is_some());
    let has_environment = stack.resources().any(|(_id, entry)| {
        entry
            .config
            .downcast_ref::<AzureContainerAppsEnvironment>()
            .is_some()
    });
    has_build && has_environment && get_container_apps_environment_resource_id(state).is_err()
}

/// Helper to extract the resource group name of the Container Apps Environment.
/// This may differ from the stack's default resource group when the environment
/// is externally provisioned in a separate resource group.
pub fn get_container_apps_environment_resource_group(state: &StackState) -> Result<String> {
    Ok(get_container_apps_environment_outputs(state)?
        .resource_group_name
        .clone())
}

/// Azure role assignments are eventually consistent. A freshly-created role
/// assignment may exist while ARM still rejects reads/writes with 401/403 and
/// "refresh your credentials" messages. Controllers use this to stay in their
/// current state while waiting for RBAC propagation instead of failing setup
/// immediately.
pub fn is_azure_authorization_propagation_error(error: &AlienError<ErrorData>) -> bool {
    const AUTHORIZATION_MESSAGE_MARKERS: &[&str] = &[
        "AuthorizationFailed",
        "Unauthorized",
        "does not have authorization",
        "refresh your credentials",
        "HTTP 401",
        "HTTP 403",
    ];

    fn context_http_status(context: Option<&serde_json::Value>) -> Option<u16> {
        context
            .and_then(|value| value.get("http_status"))
            .and_then(|value| value.as_u64())
            .and_then(|status| u16::try_from(status).ok())
    }

    fn context_contains_auth_marker(context: Option<&serde_json::Value>) -> bool {
        context.is_some_and(|value| {
            let context_text = value.to_string();
            AUTHORIZATION_MESSAGE_MARKERS
                .iter()
                .any(|marker| context_text.contains(marker))
        })
    }

    fn matches_layer(
        code: &str,
        message: &str,
        http_status_code: Option<u16>,
        context: Option<&serde_json::Value>,
    ) -> bool {
        let http_status_code = http_status_code.or_else(|| context_http_status(context));
        let authorization_status = matches!(http_status_code, Some(401 | 403));
        let authorization_code = matches!(
            code,
            "REMOTE_ACCESS_DENIED" | "AUTHENTICATION_ERROR" | "HTTP_RESPONSE_ERROR"
        );
        let authorization_message = AUTHORIZATION_MESSAGE_MARKERS
            .iter()
            .any(|marker| message.contains(marker))
            || context_contains_auth_marker(context);

        (authorization_status || authorization_code) && authorization_message
    }

    if matches_layer(
        &error.code,
        &error.message,
        error.http_status_code,
        error.context.as_ref(),
    ) {
        return true;
    }

    let mut source = error.source.as_deref();
    while let Some(layer) = source {
        if matches_layer(
            &layer.code,
            &layer.message,
            layer.http_status_code,
            layer.context.as_ref(),
        ) {
            return true;
        }
        source = layer.source.as_deref();
    }

    false
}

/// Azure ARM resource IDs are case-insensitive. Azure APIs may return provider
/// path segments with different casing than Terraform/import data, for example
/// `resourceGroups` vs `resourcegroups`.
pub(crate) fn azure_resource_ids_equal(expected: &str, actual: &str) -> bool {
    expected
        .trim_end_matches('/')
        .eq_ignore_ascii_case(actual.trim_end_matches('/'))
}

pub(crate) fn azure_resource_group_resource_id(
    subscription_id: &str,
    resource_group: &str,
) -> String {
    format!("/subscriptions/{subscription_id}/resourceGroups/{resource_group}")
}

pub(crate) fn azure_storage_account_resource_id(
    subscription_id: &str,
    resource_group: &str,
    account_name: &str,
) -> String {
    format!(
        "{}/providers/Microsoft.Storage/storageAccounts/{account_name}",
        azure_resource_group_resource_id(subscription_id, resource_group)
    )
}

pub(crate) fn azure_container_apps_environment_resource_id(
    subscription_id: &str,
    resource_group: &str,
    environment_name: &str,
) -> String {
    format!(
        "{}/providers/Microsoft.App/managedEnvironments/{environment_name}",
        azure_resource_group_resource_id(subscription_id, resource_group)
    )
}

pub(crate) fn azure_service_bus_namespace_resource_id(
    subscription_id: &str,
    resource_group: &str,
    namespace_name: &str,
) -> String {
    format!(
        "{}/providers/Microsoft.ServiceBus/namespaces/{namespace_name}",
        azure_resource_group_resource_id(subscription_id, resource_group)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack_with_build() -> alien_core::Stack {
        alien_core::Stack::new("app".to_string())
            .add(
                alien_core::Build::new("builder".to_string())
                    .permissions("execution".to_string())
                    .build(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .build()
    }

    /// The stack-wide build/execute grant is rendered by the service account, which does not
    /// have the Build's controller: it gets the Build's identity and environment from the stack,
    /// before the environment exists and after it does.
    #[test]
    fn stack_wide_build_execute_renders_from_the_stack_and_its_state() {
        use alien_permissions::{
            generators::AzureRuntimePermissionsGenerator, get_permission_set, BindingTarget,
            PermissionContext,
        };
        let stack = stack_with_build();
        let set = get_permission_set("build/execute").unwrap();
        let render = |state: &StackState| {
            let context = with_build_scopes(
                PermissionContext::new()
                    .with_subscription_id("sub")
                    .with_resource_group("rg")
                    .with_stack_prefix("pre"),
                &stack,
                state,
                "pre",
                "sub",
                "rg",
            );
            let plan = AzureRuntimePermissionsGenerator::new()
                .generate_grant_plan(set, BindingTarget::Stack, &context)
                .expect("build/execute renders without the Build controller");
            let mut scopes: Vec<String> = plan.bindings.into_iter().map(|b| b.scope).collect();
            scopes.sort();
            scopes
        };

        let mut state = StackState::new(alien_core::Platform::Azure);
        let group = "/subscriptions/sub/resourceGroups/rg";
        assert_eq!(
            render(&state),
            [
                group.to_string(),
                format!("{group}/providers/Microsoft.App/managedEnvironments/pre-default-container-env"),
                format!("{group}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/pre-execution-sa"),
            ]
        );

        let external = "/subscriptions/sub/resourceGroups/shared/providers/Microsoft.App/managedEnvironments/shared-env";
        state.resources.insert(
            "default-container-env".to_string(),
            alien_core::StackResourceState::new_pending(
                AzureContainerAppsEnvironment::RESOURCE_TYPE.to_string(),
                alien_core::Resource::new(
                    AzureContainerAppsEnvironment::new("default-container-env".to_string()).build(),
                ),
                Some(alien_core::ResourceLifecycle::Frozen),
                Vec::new(),
            )
            .with_updates(|r| {
                r.outputs = Some(alien_core::ResourceOutputs::new(
                    alien_core::AzureContainerAppsEnvironmentOutputs {
                        environment_name: "shared-env".to_string(),
                        resource_id: external.to_string(),
                        resource_group_name: "shared".to_string(),
                        default_domain: "d".to_string(),
                        static_ip: None,
                        custom_domain_verification_id: None,
                    },
                ));
            }),
        );
        assert!(render(&state).contains(&external.to_string()));

        // The join role is assignable in the shared group (never at the environment); the
        // jobs and the identity assignment stay in the stack's group.
        let context = with_build_scopes(
            PermissionContext::new()
                .with_subscription_id("sub")
                .with_resource_group("rg")
                .with_stack_prefix("pre"),
            &stack,
            &state,
            "pre",
            "sub",
            "rg",
        );
        let plan = AzureRuntimePermissionsGenerator::new()
            .generate_grant_plan(set, BindingTarget::Stack, &context)
            .unwrap();
        for binding in &plan.bindings {
            let alien_permissions::generators::AzureRoleDefinitionRef::Custom { key } =
                &binding.role_definition
            else {
                continue;
            };
            let role = plan.custom_roles.iter().find(|r| &r.key == key).unwrap();
            let expected = if binding.scope == external {
                "/subscriptions/sub/resourceGroups/shared"
            } else {
                group
            };
            assert_eq!(
                role.role_definition.assignable_scopes,
                [expected.to_string()],
                "{}",
                binding.scope
            );
        }
    }

    /// The service account waits for the environment a Build joins, only while it is missing.
    #[test]
    fn a_build_environment_is_pending_until_it_has_outputs() {
        let stack = alien_core::Stack::new("app".to_string())
            .add(
                alien_core::Build::new("builder".to_string())
                    .permissions("execution".to_string())
                    .build(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .add(
                AzureContainerAppsEnvironment::new("default-container-env".to_string()).build(),
                alien_core::ResourceLifecycle::Frozen,
            )
            .build();
        let mut state = StackState::new(alien_core::Platform::Azure);
        assert!(build_environment_pending(&stack, &state));
        // No Build, or no environment of the stack's own (nothing to wait for): not pending.
        assert!(!build_environment_pending(
            &alien_core::Stack::new("app".to_string()).build(),
            &state
        ));
        assert!(!build_environment_pending(&stack_with_build(), &state));

        state.resources.insert(
            "default-container-env".to_string(),
            alien_core::StackResourceState::new_pending(
                AzureContainerAppsEnvironment::RESOURCE_TYPE.to_string(),
                alien_core::Resource::new(
                    AzureContainerAppsEnvironment::new("default-container-env".to_string()).build(),
                ),
                Some(alien_core::ResourceLifecycle::Frozen),
                Vec::new(),
            )
            .with_updates(|r| {
                r.outputs = Some(alien_core::ResourceOutputs::new(
                    alien_core::AzureContainerAppsEnvironmentOutputs {
                        environment_name: "env".to_string(),
                        resource_id: "/subscriptions/s/resourceGroups/g/providers/Microsoft.App/managedEnvironments/env".to_string(),
                        resource_group_name: "g".to_string(),
                        default_domain: "d".to_string(),
                        static_ip: None,
                        custom_domain_verification_id: None,
                    },
                ));
            }),
        );
        assert!(!build_environment_pending(&stack, &state));
    }

    /// A stack with no Build adds nothing: no grant names an environment or an identity.
    #[test]
    fn a_stack_without_a_build_adds_no_build_scopes() {
        let stack = alien_core::Stack::new("app".to_string()).build();
        let context = with_build_scopes(
            alien_permissions::PermissionContext::new(),
            &stack,
            &StackState::new(alien_core::Platform::Azure),
            "pre",
            "sub",
            "rg",
        );
        assert!(context.build_identity_ids.is_empty());
        assert!(context.build_environment_id.is_none());
    }

    #[test]
    fn azure_resource_ids_match_arm_resource_id_format() {
        let subscription_id = "sub-123";
        let resource_group = "alien-e2e-rg";

        assert_eq!(
            azure_resource_group_resource_id(subscription_id, resource_group),
            "/subscriptions/sub-123/resourceGroups/alien-e2e-rg"
        );
        assert_eq!(
            azure_storage_account_resource_id(subscription_id, resource_group, "alienstorage"),
            "/subscriptions/sub-123/resourceGroups/alien-e2e-rg/providers/Microsoft.Storage/storageAccounts/alienstorage"
        );
        assert_eq!(
            azure_container_apps_environment_resource_id(
                subscription_id,
                resource_group,
                "alien-env"
            ),
            "/subscriptions/sub-123/resourceGroups/alien-e2e-rg/providers/Microsoft.App/managedEnvironments/alien-env"
        );
        assert_eq!(
            azure_service_bus_namespace_resource_id(subscription_id, resource_group, "alien-bus"),
            "/subscriptions/sub-123/resourceGroups/alien-e2e-rg/providers/Microsoft.ServiceBus/namespaces/alien-bus"
        );
    }

    #[test]
    fn azure_resource_ids_equal_ignores_arm_path_casing() {
        let expected = "/subscriptions/sub-id/resourceGroups/rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/app-sa";
        let actual = "/subscriptions/sub-id/resourcegroups/rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/app-sa";

        assert!(azure_resource_ids_equal(expected, actual));
    }

    #[test]
    fn azure_resource_ids_equal_rejects_different_resource_names() {
        let expected = "/subscriptions/sub-id/resourceGroups/rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/app-sa";
        let actual = "/subscriptions/sub-id/resourceGroups/rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/other-sa";

        assert!(!azure_resource_ids_equal(expected, actual));
    }
}
