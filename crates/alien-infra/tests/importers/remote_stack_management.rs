use super::*;

fn assert_updating_with_internal_state(state: &alien_core::StackResourceState) {
    assert_eq!(
        state.status,
        ResourceStatus::Updating,
        "imported management identity must reconcile runtime-owned grants before Running"
    );
    let internal = internal_state(state)
        .as_object()
        .expect("internal_state must serialize as object");
    assert!(
        internal.contains_key("type"),
        "serialize_controller must inject a `type` discriminator"
    );
}

#[test]
fn aws_remote_stack_management_round_trip() {
    let entry = entry(RemoteStackManagement::new("rsm".to_string()).build());
    let data = AwsRemoteStackManagementImportData {
        role_arn: "arn:aws:iam::123456789012:role/alien-stack-mgmt".to_string(),
        role_name: "alien-stack-mgmt".to_string(),
        management_permissions_applied: true,
    };
    let state = run_through_registry(
        &RemoteStackManagement::RESOURCE_TYPE,
        Platform::Aws,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "us-east-1",
        &aws_management_config(),
    );
    assert_updating_with_internal_state(&state);
    assert_eq!(internal_state(&state)["state"], "updateStart");
    assert_eq!(
        internal_state(&state)["appliedManagementGrantFingerprint"],
        serde_json::Value::Null,
        "imported management identities must force one runtime grant reconciliation"
    );
}

#[test]
fn gcp_remote_stack_management_import_requires_runtime_grant_reconciliation() {
    let entry = entry(RemoteStackManagement::new("rsm".to_string()).build());
    let data = GcpRemoteStackManagementImportData {
        project_id: "my-project".to_string(),
        project_number: Some("123456789012".to_string()),
        service_account_email: "management@my-project.iam.gserviceaccount.com".to_string(),
        service_account_unique_id: "123456789012345678901".to_string(),
        management_permissions_applied: true,
    };
    let state = run_through_registry(
        &RemoteStackManagement::RESOURCE_TYPE,
        Platform::Gcp,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "us-central1",
        &gcp_management_config(),
    );

    assert_updating_with_internal_state(&state);
    assert_eq!(internal_state(&state)["state"], "updateStart");
    let internal = internal_state(&state);
    assert_eq!(
        internal["appliedManagementGrantFingerprint"],
        serde_json::Value::Null,
    );
    assert_eq!(internal["remoteStorageBucketNames"], json!([]));
}

#[test]
fn azure_remote_stack_management_round_trip_includes_access_outputs() {
    let entry = entry(RemoteStackManagement::new("rsm".to_string()).build());
    let data = AzureRemoteStackManagementImportData {
        subscription_id: "00000000-0000-0000-0000-000000000000".to_string(),
        resource_group: "rg-alien".to_string(),
        identity_id: "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-alien/providers/Microsoft.ManagedIdentity/userAssignedIdentities/alien-management".to_string(),
        client_id: "11111111-1111-1111-1111-111111111111".to_string(),
        principal_id: "22222222-2222-2222-2222-222222222222".to_string(),
        tenant_id: "33333333-3333-3333-3333-333333333333".to_string(),
        management_permissions_applied: true,
    };
    let state = run_through_registry(
        &RemoteStackManagement::RESOURCE_TYPE,
        Platform::Azure,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "eastus",
        &azure_management_config(),
    );
    assert_eq!(state.status, ResourceStatus::Provisioning);
    assert_eq!(internal_state(&state)["state"], "waitingForRbacPropagation");
    assert_eq!(internal_state(&state)["setupManaged"], true);
    assert_eq!(
        internal_state(&state)["appliedManagementGrantFingerprint"],
        serde_json::Value::Null,
        "import must not claim setup-created grants are runtime-owned"
    );
    assert_eq!(
        internal_state(&state)["resourceRoleDefinitionIds"],
        json!({}),
    );
    assert_eq!(internal_state(&state)["roleAssignmentIds"], json!([]));

    let outputs = state
        .outputs
        .as_ref()
        .and_then(|outputs| outputs.downcast_ref::<RemoteStackManagementOutputs>())
        .expect("Azure remote-stack-management import must produce outputs");
    assert_eq!(outputs.management_resource_id, data.identity_id);

    let access_config: serde_json::Value =
        serde_json::from_str(&outputs.access_configuration).unwrap();
    assert_eq!(
        access_config,
        json!({
            "uamiClientId": data.client_id,
            "tenantId": data.tenant_id,
        })
    );
}
