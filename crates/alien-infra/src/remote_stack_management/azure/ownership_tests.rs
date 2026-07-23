use super::*;

fn legacy_controller(
    state: AzureRemoteStackManagementState,
) -> AzureRemoteStackManagementController {
    AzureRemoteStackManagementController {
        setup_managed: None,
        state,
        uami_resource_id: Some("/subscriptions/sub/resourceGroups/rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/management".to_string()),
        uami_client_id: Some("client".to_string()),
        uami_principal_id: Some("principal".to_string()),
        tenant_id: Some("tenant".to_string()),
        fic_name: None,
        role_definition_id: None,
        resource_role_definition_ids: HashMap::new(),
        role_assignment_ids: Vec::new(),
        role_assignment_wait_until_epoch_secs: None,
        applied_management_grant_fingerprint: None,
        _internal_stay_count: None,
    }
}

#[test]
fn legacy_import_without_owned_ids_remains_setup_managed() {
    let controller = legacy_controller(AzureRemoteStackManagementState::Ready);
    assert!(controller.setup_managed_resources());

    let controller = legacy_controller(AzureRemoteStackManagementState::WaitingForRbacPropagation);
    assert!(controller.setup_managed_resources());
}

#[test]
fn legacy_direct_controller_remains_runtime_owned() {
    let mut ready = legacy_controller(AzureRemoteStackManagementState::Ready);
    ready.fic_name = Some("stack-management-fic".to_string());
    assert!(!ready.setup_managed_resources());

    let failed = legacy_controller(AzureRemoteStackManagementState::CreateFailed);
    assert!(!failed.setup_managed_resources());
}

#[test]
fn explicit_ownership_overrides_legacy_inference() {
    let mut controller = legacy_controller(AzureRemoteStackManagementState::Ready);
    controller.setup_managed = Some(false);
    assert!(!controller.setup_managed_resources());

    controller.fic_name = Some("stack-management-fic".to_string());
    controller.setup_managed = Some(true);
    assert!(controller.setup_managed_resources());
}
