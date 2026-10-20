//! Scratch (not committed): drives the real Azure Live vault controller against a real setup,
//! as a stand-in principal holding the management identity's roles.
use crate::core::{DefaultPlatformServiceProvider, ResourceController, StackExecutor, StackResourceStateExt};
use crate::infra_requirements::{AzureResourceGroupController, AzureResourceGroupState};
use crate::remote_stack_management::AzureRemoteStackManagementController;
use crate::service_account::AzureServiceAccountController;
use crate::vault::AzureVaultController;
use alien_core::{
    AzureClientConfig, AzureCredentials, ClientConfig, DeploymentConfig,
    EnvironmentVariablesSnapshot, ExternalBindings, InitialSetupAuthority, Platform, Resource,
    ResourceLifecycle, ResourceStatus, Stack, StackResourceState, StackSettings, StackState,
};
use std::sync::Arc;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

#[tokio::test]
#[ignore]
async fn azure_live_vault_compat() {
    let phase = env("PHASE");
    let state_file = env("STATE_FILE");
    let subscription = env("SUB");
    let rg = env("RG");
    let prefix = env("PREFIX");
    let keep = ["app-secrets", "execution-sa", "management", "default-resource-group"];
    let prepared: Stack = serde_json::from_str(&std::fs::read_to_string(env("STACK_FILE")).unwrap()).unwrap();
    let mut stack = prepared.clone();
    stack.resources.retain(|id, _| keep.contains(&id.as_str()));
    for entry in stack.resources.values_mut() {
        entry.dependencies.retain(|dep| keep.contains(&dep.id()));
    }
    let config = DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: String::new(),
            created_at: String::new(),
        })
        .external_bindings(ExternalBindings::default())
        .allow_frozen_changes(false)
        .build();
    let client = ClientConfig::Azure(Box::new(AzureClientConfig {
        subscription_id: subscription.clone(),
        tenant_id: env("TENANT"),
        region: Some("eastus".to_string()),
        credentials: AzureCredentials::ServicePrincipal {
            client_id: env("STANDIN_APP"),
            client_secret: env("STANDIN_SECRET"),
        },
        service_overrides: None,
    }));
    let executor = StackExecutor::builder(&stack, client)
        .deployment_config(&config)
        .service_provider(Arc::new(DefaultPlatformServiceProvider::default()))
        .initial_setup_authority(InitialSetupAuthority::ImportedHandoff)
        .step_running_resources(phase == "refresh")
        .build()
        .unwrap();

    let mut state: StackState = match std::fs::read_to_string(&state_file) {
        Ok(saved) => serde_json::from_str(&saved).unwrap(),
        Err(_) => {
            let mut state = StackState::with_resource_prefix(Platform::Azure, prefix.clone());
            let rg_id = format!("/subscriptions/{subscription}/resourceGroups/{rg}");
            let mut insert = |id: &str, controller: Box<dyn ResourceController>| {
                let entry = &stack.resources[id];
                let mut resource = StackResourceState::new_pending(
                    entry.config.resource_type().to_string(),
                    entry.config.clone(),
                    Some(ResourceLifecycle::Frozen),
                    vec![],
                );
                resource.status = ResourceStatus::Running;
                resource.outputs = controller.get_outputs();
                resource.set_internal_controller(Some(controller)).unwrap();
                state.resources.insert(id.to_string(), resource);
            };
            insert(
                "default-resource-group",
                Box::new(AzureResourceGroupController {
                    state: AzureResourceGroupState::Ready,
                    resource_group_name: Some(rg.clone()),
                    resource_id: Some(rg_id.clone()),
                    location: Some("eastus".to_string()),
                    _internal_stay_count: None,
                }),
            );
            let mut account = AzureServiceAccountController::mock_ready("execution-sa");
            account.identity_resource_id = Some(env("WORKLOAD_ID"));
            account.identity_principal_id = Some(env("WORKLOAD_OID"));
            account.identity_client_id = Some(env("WORKLOAD_CLIENT"));
            account.custom_role_definition_ids = vec![];
            account.role_assignment_ids = vec![];
            insert("execution-sa", Box::new(account));
            let mut management = AzureRemoteStackManagementController::mock_ready(&prefix);
            management.uami_principal_id = Some(env("STANDIN_OID"));
            management.uami_resource_id = Some(format!(
                "{rg_id}/providers/Microsoft.ManagedIdentity/userAssignedIdentities/{prefix}-management-identity"
            ));
            insert("management", Box::new(management));
            state
        }
    };
    if phase == "update" {
        let resource = state.resources.get_mut("app-secrets").unwrap();
        let mut controller = resource.get_internal_controller_typed::<AzureVaultController>().unwrap();
        controller.permissions_revision = Some("before-this-release".to_string());
        resource.set_internal_controller(Some(Box::new(controller))).unwrap();
    }
    for step in 0..40 {
        let plan = executor.plan(&state).unwrap();
        let vault = state.resources.get("app-secrets");
        let settled = vault.is_some_and(|v| v.status == ResourceStatus::Running)
            && plan.creates.is_empty()
            && plan.updates.is_empty();
        if settled && (phase != "refresh" || step >= 2) {
            break;
        }
        let result = executor.step(state).await.unwrap();
        state = result.next_state;
        let vault = &state.resources["app-secrets"];
        println!("[{phase}] step {step}: app-secrets {:?} {:?}", vault.status, vault.error.as_ref().map(|e| e.to_string()));
        if matches!(
            vault.status,
            ResourceStatus::ProvisionFailed | ResourceStatus::UpdateFailed | ResourceStatus::RefreshFailed
        ) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    }
    let vault = &state.resources["app-secrets"];
    println!("[{phase}] RESULT app-secrets status={:?} error={:?}", vault.status, vault.error.as_ref().map(|e| e.to_string()));
    std::fs::write(&state_file, serde_json::to_string_pretty(&state).unwrap()).unwrap();
}
