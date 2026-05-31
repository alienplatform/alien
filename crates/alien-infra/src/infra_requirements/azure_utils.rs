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
