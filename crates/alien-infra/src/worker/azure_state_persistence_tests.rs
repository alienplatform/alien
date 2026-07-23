#[test]
fn storage_trigger_teardown_progress_is_persisted_and_legacy_safe() {
    let mut controller = AzureWorkerController::mock_ready("worker");
    controller.storage_trigger_teardown_progress =
        AzureStorageTriggerTeardownProgress::ReceiverRoleAssignment;
    let serialized = serde_json::to_value(&controller).expect("controller state should serialize");
    let restored: AzureWorkerController =
        serde_json::from_value(serialized.clone()).expect("controller state should restore");
    assert_eq!(
        restored.storage_trigger_teardown_progress,
        AzureStorageTriggerTeardownProgress::ReceiverRoleAssignment
    );

    let mut legacy = serialized;
    legacy
        .as_object_mut()
        .expect("controller state should be an object")
        .remove("storageTriggerTeardownProgress");
    let restored: AzureWorkerController =
        serde_json::from_value(legacy).expect("legacy controller state should restore");
    assert_eq!(
        restored.storage_trigger_teardown_progress,
        AzureStorageTriggerTeardownProgress::EventSubscription
    );
}

#[test]
fn commands_sender_intent_is_persisted_in_camel_case_and_legacy_safe() {
    let mut controller = AzureWorkerController::mock_ready("worker");
    controller.commands_sender_role_assignment_intent =
        Some(AzureCommandsSenderRoleAssignmentIntent {
            assignment_id: "assignment-id".to_string(),
            assignment_name: "assignment-name".to_string(),
            principal_id: "principal-id".to_string(),
            resource_group_name: "resource-group".to_string(),
            namespace_name: "namespace".to_string(),
            queue_name: "queue".to_string(),
        });
    controller.commands_sender_role_assignment_discovery_complete = true;
    let serialized = serde_json::to_value(&controller).expect("controller state should serialize");
    let intent = serialized
        .get("commandsSenderRoleAssignmentIntent")
        .and_then(serde_json::Value::as_object)
        .expect("intent should use the controller's camelCase state contract");
    assert_eq!(
        intent
            .get("assignmentId")
            .and_then(serde_json::Value::as_str),
        Some("assignment-id")
    );
    assert!(!intent.contains_key("assignment_id"));

    let restored: AzureWorkerController =
        serde_json::from_value(serialized.clone()).expect("controller state should restore");
    assert_eq!(
        restored.commands_sender_role_assignment_intent,
        controller.commands_sender_role_assignment_intent
    );
    assert!(restored.commands_sender_role_assignment_discovery_complete);

    let mut legacy = serialized;
    legacy
        .as_object_mut()
        .expect("controller state should be an object")
        .remove("commandsSenderRoleAssignmentIntent");
    legacy
        .as_object_mut()
        .expect("controller state should be an object")
        .remove("commandsSenderRoleAssignmentDiscoveryComplete");
    let restored: AzureWorkerController =
        serde_json::from_value(legacy).expect("legacy controller state should restore");
    assert!(restored.commands_sender_role_assignment_intent.is_none());
    assert!(!restored.commands_sender_role_assignment_discovery_complete);
}

#[test]
fn clear_all_resets_commands_lro_and_retry_state() {
    let mut controller = AzureWorkerController::mock_ready("worker");
    controller.pending_operation_url = Some("operation".to_string());
    controller.pending_operation_retry_after = Some(30);
    controller.pending_dapr_component_deletion_name = Some("component".to_string());
    controller.commands_resource_group_name = Some("resource-group".to_string());
    controller.commands_namespace_name = Some("namespace".to_string());
    controller.commands_queue_name = Some("queue".to_string());
    controller.commands_dapr_component = Some("component".to_string());
    controller.commands_dapr_component_deletion_candidates = vec!["component".to_string()];
    controller.commands_sender_role_assignment_id = Some("assignment".to_string());
    controller.commands_sender_role_assignment_intent =
        Some(AzureCommandsSenderRoleAssignmentIntent {
            assignment_id: "planned-assignment".to_string(),
            assignment_name: "assignment-name".to_string(),
            principal_id: "principal".to_string(),
            resource_group_name: "resource-group".to_string(),
            namespace_name: "namespace".to_string(),
            queue_name: "queue".to_string(),
        });
    controller.commands_sender_role_assignment_discovery_complete = true;
    controller.commands_infrastructure_auth_wait_until_epoch_secs = Some(1);
    controller.container_apps_environment_wake_wait_until_epoch_secs = Some(2);
    controller.container_apps_environment_wake_retry_after_epoch_secs = Some(3);
    controller.pre_container_app_rbac_wait_until_epoch_secs = Some(4);
    controller.ready_rbac_wait_until_epoch_secs = Some(5);
    controller.update_rbac_wait_required = true;
    controller.update_dapr_components_deleted = true;
    controller.commands_update_teardown_candidates_initialized = true;
    controller.trigger_update_teardown_candidates_initialized = true;

    controller.clear_all();

    assert!(controller.pending_operation_url.is_none());
    assert!(controller.pending_operation_retry_after.is_none());
    assert!(controller.pending_dapr_component_deletion_name.is_none());
    assert!(controller.commands_resource_group_name.is_none());
    assert!(controller.commands_namespace_name.is_none());
    assert!(controller.commands_queue_name.is_none());
    assert!(controller.commands_dapr_component.is_none());
    assert!(controller
        .commands_dapr_component_deletion_candidates
        .is_empty());
    assert!(controller.commands_sender_role_assignment_id.is_none());
    assert!(controller.commands_sender_role_assignment_intent.is_none());
    assert!(!controller.commands_sender_role_assignment_discovery_complete);
    assert!(controller
        .commands_infrastructure_auth_wait_until_epoch_secs
        .is_none());
    assert!(controller
        .container_apps_environment_wake_wait_until_epoch_secs
        .is_none());
    assert!(controller
        .container_apps_environment_wake_retry_after_epoch_secs
        .is_none());
    assert!(controller
        .pre_container_app_rbac_wait_until_epoch_secs
        .is_none());
    assert!(controller.ready_rbac_wait_until_epoch_secs.is_none());
    assert!(!controller.update_rbac_wait_required);
    assert!(!controller.update_dapr_components_deleted);
    assert!(!controller.commands_update_teardown_candidates_initialized);
    assert!(!controller.trigger_update_teardown_candidates_initialized);
}
