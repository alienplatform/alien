use super::*;

#[test]
fn aws_remote_stack_management_import_preserves_setup_ownership() {
    let entry = entry(RemoteStackManagement::new("rsm".to_string()).build());
    let data = AwsRemoteStackManagementImportData {
        role_arn: "arn:aws:iam::123456789012:role/alien-stack-mgmt".to_string(),
        role_name: "alien-stack-mgmt".to_string(),
        worker_gateway_cleanup_function_arn: None,
        remote_bindings_role_arn: Some(
            "arn:aws:iam::123456789012:role/alien-stack-remote-bindings".to_string(),
        ),
        management_permissions_applied: true,
        installed_management_grants: Some(std::collections::BTreeMap::from([(
            "*".to_string(),
            vec!["installed-revision".to_string()],
        )])),
    };
    for applied in [false, true] {
        let mut legacy_data = serde_json::to_value(&data).unwrap();
        legacy_data["managementPermissionsApplied"] = json!(applied);
        if applied {
            legacy_data
                .as_object_mut()
                .unwrap()
                .remove("installedManagementGrants");
        }
        let imported = run_through_registry(
            &RemoteStackManagement::RESOURCE_TYPE,
            Platform::Aws,
            legacy_data,
            &entry,
            "us-east-1",
            &aws_management_config(),
        );
        assert_eq!(
            internal_state(&imported)["installedManagementGrants"],
            serde_json::Value::Null,
            "unapplied grants and legacy imports must not claim installed permissions",
        );
    }
    let state = run_through_registry(
        &RemoteStackManagement::RESOURCE_TYPE,
        Platform::Aws,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "us-east-1",
        &aws_management_config(),
    );
    assert_eq!(state.status, ResourceStatus::Running);
    assert_eq!(
        internal_state(&state)["installedManagementGrants"],
        json!({"*": ["installed-revision"]})
    );
    let internal = internal_state(&state);
    assert!(
        internal
            .as_object()
            .expect("internal_state must serialize as object")
            .contains_key("type"),
        "serialize_controller must inject a `type` discriminator"
    );
    assert_eq!(internal["state"], "ready");
    assert_eq!(internal["managementPermissionsApplied"], true);
    state
        .outputs
        .as_ref()
        .and_then(|outputs| outputs.downcast_ref::<RemoteStackManagementOutputs>())
        .expect("AWS remote-stack-management import must produce outputs");
    assert_eq!(
        internal["appliedManagementGrantFingerprint"],
        serde_json::Value::Null,
        "import must not claim setup-created grants are runtime-owned"
    );
}

#[test]
fn aws_gateway_cleanup_capability_survives_import_and_controller_reload() {
    let resource = frozen_entry(RemoteStackManagement::new("management".to_string()).build());
    let function = "arn:aws:lambda:us-east-1:123456789012:function:_gateway-cleanup-example:7";
    let payload = json!({
        "roleName": "example-management",
        "roleArn": "arn:aws:iam::123456789012:role/example-management",
        "managementPermissionsApplied": true,
        "workerGatewayCleanupFunctionArn": function,
    });
    let state = run_through_registry(
        &RemoteStackManagement::RESOURCE_TYPE,
        Platform::Aws,
        payload,
        &resource,
        "us-east-1",
        &aws_management_config(),
    );
    assert_eq!(state.status, ResourceStatus::Running);
    assert_eq!(
        internal_state(&state)["workerGatewayCleanupFunctionArn"],
        function
    );
    let restored = alien_infra::deserialize_controller(internal_state(&state).clone())
        .expect("capability must survive a manager restart");
    let outputs = restored.get_outputs().expect("imported management output");
    let outputs = outputs
        .downcast_ref::<RemoteStackManagementOutputs>()
        .unwrap();
    assert_eq!(
        outputs.aws_worker_gateway_cleanup_function_arn.as_deref(),
        Some(function)
    );
}

#[test]
fn aws_gateway_cleanup_import_rejects_foreign_or_mutable_functions() {
    let resource = frozen_entry(RemoteStackManagement::new("management".to_string()).build());
    let settings = settings();
    let management = aws_management_config();
    let ctx = ImportContext {
        resource_id: "management",
        platform: Platform::Aws,
        region: "us-east-1",
        stack_settings: &settings,
        management_config: Some(&management),
        resource: &resource,
    };
    let registry = ImporterRegistry::built_in();
    registry.run(&RemoteStackManagement::RESOURCE_TYPE, Platform::Aws, json!({
        "roleName": "example-management",
        "roleArn": "arn:aws:iam::123456789012:role/example-management",
        "managementPermissionsApplied": false,
        "workerGatewayCleanupFunctionArn": "arn:aws:lambda:us-east-1:123456789012:function:_gateway-cleanup-example:7",
    }), &ctx).expect_err("an unapplied invocation grant must not be exposed to runtime");
    for function in [
        "arn:aws:lambda:us-east-1:999999999999:function:_gateway-cleanup-example:7",
        "arn:aws:lambda:us-west-2:123456789012:function:_gateway-cleanup-example:7",
        "arn:aws:lambda:us-east-1:123456789012:function:_gateway-cleanup-other:7",
        "arn:aws:lambda:us-east-1:123456789012:function:_gateway-cleanup-example:$LATEST",
    ] {
        registry
            .run(
                &RemoteStackManagement::RESOURCE_TYPE,
                Platform::Aws,
                json!({
                    "roleName": "example-management",
                    "roleArn": "arn:aws:iam::123456789012:role/example-management",
                    "managementPermissionsApplied": true,
                    "workerGatewayCleanupFunctionArn": function,
                }),
                &ctx,
            )
            .expect_err("setup cannot register a foreign or mutable cleanup capability");
    }
}

#[test]
fn gcp_remote_stack_management_import_preserves_setup_ownership() {
    let entry = entry(RemoteStackManagement::new("rsm".to_string()).build());
    let data = GcpRemoteStackManagementImportData {
        custom_role_namespace: None,
        project_id: "my-project".to_string(),
        project_number: Some("123456789012".to_string()),
        service_account_email: "management@my-project.iam.gserviceaccount.com".to_string(),
        service_account_unique_id: "123456789012345678901".to_string(),
        remote_bindings_service_account_email: Some(
            "remote-bindings@my-project.iam.gserviceaccount.com".to_string(),
        ),
        management_permissions_applied: true,
        installed_management_grants: Some(std::collections::BTreeMap::from([(
            "*".to_string(),
            vec!["installed-revision".to_string()],
        )])),
    };
    for applied in [false, true] {
        let mut legacy_data = serde_json::to_value(&data).unwrap();
        legacy_data["managementPermissionsApplied"] = json!(applied);
        if applied {
            legacy_data
                .as_object_mut()
                .unwrap()
                .remove("installedManagementGrants");
        }
        let imported = run_through_registry(
            &RemoteStackManagement::RESOURCE_TYPE,
            Platform::Gcp,
            legacy_data,
            &entry,
            "us-central1",
            &gcp_management_config(),
        );
        assert_eq!(
            internal_state(&imported)["installedManagementGrants"],
            serde_json::Value::Null,
            "unapplied grants and legacy imports must not claim installed permissions",
        );
    }
    let state = run_through_registry(
        &RemoteStackManagement::RESOURCE_TYPE,
        Platform::Gcp,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "us-central1",
        &gcp_management_config(),
    );

    assert_eq!(state.status, ResourceStatus::Running);
    assert_eq!(
        internal_state(&state)["installedManagementGrants"],
        json!({"*": ["installed-revision"]})
    );
    assert_eq!(internal_state(&state)["state"], "ready");
    let internal = internal_state(&state);
    assert_eq!(internal["roleBound"], true);
    assert_eq!(internal["impersonationGranted"], true);
    state
        .outputs
        .as_ref()
        .and_then(|outputs| outputs.downcast_ref::<RemoteStackManagementOutputs>())
        .expect("GCP remote-stack-management import must produce outputs");
}

#[test]
fn azure_remote_stack_management_round_trip_includes_access_outputs() {
    let entry = entry(RemoteStackManagement::new("rsm".to_string()).build());
    let data = AzureRemoteStackManagementImportData {
        subscription_id: "00000000-0000-0000-0000-000000000000".to_string(),
        resource_group: "rg-alien".to_string(),
        identity_id: "/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-alien/providers/Microsoft.ManagedIdentity/userAssignedIdentities/alien-management".to_string(),
        client_id: "11111111-1111-1111-1111-111111111111".to_string(),
        remote_bindings_identity_id: Some("/subscriptions/00000000-0000-0000-0000-000000000000/resourceGroups/rg-alien/providers/Microsoft.ManagedIdentity/userAssignedIdentities/alien-remote-bindings".to_string()),
        remote_bindings_client_id: Some("44444444-4444-4444-4444-444444444444".to_string()),
        principal_id: "22222222-2222-2222-2222-222222222222".to_string(),
        tenant_id: "33333333-3333-3333-3333-333333333333".to_string(),
        management_permissions_applied: true,
        installed_management_grants: Some(std::collections::BTreeMap::from([("*".to_string(), vec!["installed-revision".to_string()])])),
    };
    for applied in [false, true] {
        let mut legacy_data = serde_json::to_value(&data).unwrap();
        legacy_data["managementPermissionsApplied"] = json!(applied);
        if applied {
            legacy_data
                .as_object_mut()
                .unwrap()
                .remove("installedManagementGrants");
        }
        let imported = run_through_registry(
            &RemoteStackManagement::RESOURCE_TYPE,
            Platform::Azure,
            legacy_data,
            &entry,
            "eastus",
            &azure_management_config(),
        );
        assert_eq!(
            internal_state(&imported)["installedManagementGrants"],
            serde_json::Value::Null,
            "unapplied grants and legacy imports must not claim installed permissions",
        );
    }
    let state = run_through_registry(
        &RemoteStackManagement::RESOURCE_TYPE,
        Platform::Azure,
        serde_json::to_value(&data).unwrap(),
        &entry,
        "eastus",
        &azure_management_config(),
    );
    assert_eq!(state.status, ResourceStatus::Provisioning);
    assert_eq!(
        internal_state(&state)["installedManagementGrants"],
        json!({"*": ["installed-revision"]})
    );
    assert_eq!(internal_state(&state)["state"], "waitingForRbacPropagation");
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

#[test]
fn remote_bindings_imports_are_first_class_on_every_cloud() {
    let cases = [
        (
            Platform::Aws,
            serde_json::to_value(AwsRemoteBindingsImportData {
                role_name: "alien-bindings".to_string(),
                role_arn: "arn:aws:iam::123456789012:role/alien-bindings".to_string(),
            })
            .unwrap(),
            "arn:aws:iam::123456789012:role/alien-bindings",
        ),
        (
            Platform::Gcp,
            serde_json::to_value(GcpRemoteBindingsImportData {
                project_id: "my-project".to_string(),
                service_account_email: "bindings@my-project.iam.gserviceaccount.com".to_string(),
                service_account_unique_id: "123456789012345678901".to_string(),
            })
            .unwrap(),
            "bindings@my-project.iam.gserviceaccount.com",
        ),
        (
            Platform::Azure,
            serde_json::to_value(AzureRemoteBindingsImportData {
                identity_id: "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/bindings".to_string(),
                client_id: "11111111-1111-1111-1111-111111111111".to_string(),
                principal_id: "22222222-2222-2222-2222-222222222222".to_string(),
                tenant_id: "33333333-3333-3333-3333-333333333333".to_string(),
            })
            .unwrap(),
            "/subscriptions/sub/resourceGroups/rg/providers/Microsoft.ManagedIdentity/userAssignedIdentities/bindings",
        ),
    ];

    for (platform, import_data, expected_id) in cases {
        let entry = entry(RemoteBindings::new("remote-bindings".to_string()).build());
        let state = run_through_registry(
            &RemoteBindings::RESOURCE_TYPE,
            platform,
            import_data,
            &entry,
            "us-east-1",
            &match platform {
                Platform::Aws => aws_management_config(),
                Platform::Gcp => gcp_management_config(),
                Platform::Azure => azure_management_config(),
                _ => unreachable!(),
            },
        );
        let outputs = state
            .outputs
            .as_ref()
            .and_then(|outputs| outputs.downcast_ref::<RemoteBindingsOutputs>())
            .expect("Remote Bindings import must produce first-class outputs");
        assert_eq!(outputs.resource_id, expected_id);
    }
}
