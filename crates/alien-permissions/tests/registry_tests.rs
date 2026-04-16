use alien_permissions::{get_permission_set, has_permission_set, list_permission_set_ids};

#[test]
fn test_registry_basic_functionality() {
    // Test that the registry contains expected permission sets
    assert!(has_permission_set("storage/data-read"));
    assert!(has_permission_set("function/execute"));
    assert!(has_permission_set("build/provision"));

    // Test non-existent permission set
    assert!(!has_permission_set("nonexistent/permission"));
}

#[test]
fn test_get_permission_set_storage_data_read() {
    let perm_set = get_permission_set("storage/data-read");
    assert!(perm_set.is_some());

    let perm_set = perm_set.unwrap();
    assert_eq!(perm_set.id, "storage/data-read");
    assert!(perm_set.description.contains("reading data"));

    // Verify it has AWS permissions
    assert!(perm_set.platforms.aws.is_some());
    let aws_perms = perm_set.platforms.aws.as_ref().unwrap();
    assert!(!aws_perms.is_empty());

    let first_aws_perm = &aws_perms[0];
    assert!(first_aws_perm.grant.actions.is_some());
    let actions = first_aws_perm.grant.actions.as_ref().unwrap();
    assert!(actions.contains(&"s3:GetObject".to_string()));
    assert!(actions.contains(&"s3:ListBucket".to_string()));
}

#[test]
fn test_get_permission_set_function_execute() {
    let perm_set = get_permission_set("function/execute");
    assert!(perm_set.is_some());

    let perm_set = perm_set.unwrap();
    assert_eq!(perm_set.id, "function/execute");
    assert!(perm_set.description.contains("executing"));

    // Verify AWS permissions
    if let Some(aws_perms) = &perm_set.platforms.aws {
        let first_perm = &aws_perms[0];
        if let Some(actions) = &first_perm.grant.actions {
            assert!(actions.contains(&"logs:PutLogEvents".to_string()));
        }
    }
}

#[test]
fn test_list_permission_set_ids() {
    let ids = list_permission_set_ids();
    assert!(!ids.is_empty());

    // Should contain some expected IDs
    assert!(ids.contains(&"storage/data-read"));
    assert!(ids.contains(&"storage/data-write"));
    assert!(ids.contains(&"storage/management"));
    assert!(ids.contains(&"storage/provision"));
    assert!(ids.contains(&"function/execute"));
    assert!(ids.contains(&"function/management"));
    assert!(ids.contains(&"function/provision"));
    assert!(ids.contains(&"build/execute"));
    assert!(ids.contains(&"build/management"));
    assert!(ids.contains(&"build/provision"));

    println!("Found {} permission sets: {:?}", ids.len(), ids);
}

#[test]
fn test_permission_set_has_valid_bindings() {
    let perm_set = get_permission_set("storage/data-read").unwrap();

    // Test AWS bindings
    if let Some(aws_perms) = &perm_set.platforms.aws {
        let first_perm = &aws_perms[0];
        assert!(!first_perm.binding.is_empty());

        // Should have both stack and resource bindings
        assert!(first_perm.binding.stack.is_some());
        assert!(first_perm.binding.resource.is_some());

        let stack_binding = first_perm.binding.stack.as_ref().unwrap();
        assert!(!stack_binding.resources.is_empty());

        // Should contain variables for interpolation
        let resource_arn = &stack_binding.resources[0];
        assert!(resource_arn.contains("${stackPrefix}"));
    }
}

#[test]
fn test_all_permission_sets_parseable() {
    let ids = list_permission_set_ids();

    for id in ids {
        let perm_set = get_permission_set(id);
        assert!(
            perm_set.is_some(),
            "Permission set '{}' should be parseable",
            id
        );

        let perm_set = perm_set.unwrap();
        assert_eq!(perm_set.id, id, "Permission set ID should match");
        assert!(
            !perm_set.description.is_empty(),
            "Permission set '{}' should have a description",
            id
        );
    }
}

#[test]
fn test_azure_container_apps_environment_naming_conventions() {
    // Test that both hyphenated and underscore formats work for azure-container-apps-environment

    // Test provision permission set with hyphenated format (canonical)
    assert!(has_permission_set(
        "azure-container-apps-environment/provision"
    ));
    let perm_set_hyphenated = get_permission_set("azure-container-apps-environment/provision");
    assert!(perm_set_hyphenated.is_some());

    // Test provision permission set with underscore format (alternative)
    assert!(has_permission_set(
        "azure_container_apps_environment/provision"
    ));
    let perm_set_underscore = get_permission_set("azure_container_apps_environment/provision");
    assert!(perm_set_underscore.is_some());

    // Both should return the same permission set
    let hyphenated = perm_set_hyphenated.unwrap();
    let underscore = perm_set_underscore.unwrap();
    assert_eq!(hyphenated.id, underscore.id);
    assert_eq!(hyphenated.description, underscore.description);

    // Test management permission set with hyphenated format (canonical)
    assert!(has_permission_set(
        "azure-container-apps-environment/management"
    ));
    let mgmt_perm_set_hyphenated =
        get_permission_set("azure-container-apps-environment/management");
    assert!(mgmt_perm_set_hyphenated.is_some());

    // Test management permission set with underscore format (alternative)
    assert!(has_permission_set(
        "azure_container_apps_environment/management"
    ));
    let mgmt_perm_set_underscore =
        get_permission_set("azure_container_apps_environment/management");
    assert!(mgmt_perm_set_underscore.is_some());

    // Both should return the same permission set
    let mgmt_hyphenated = mgmt_perm_set_hyphenated.unwrap();
    let mgmt_underscore = mgmt_perm_set_underscore.unwrap();
    assert_eq!(mgmt_hyphenated.id, mgmt_underscore.id);
    assert_eq!(mgmt_hyphenated.description, mgmt_underscore.description);
}

#[test]
fn test_other_azure_resources_naming_conventions() {
    // Test that both hyphenated and underscore formats work for other Azure resources

    // Test azure-resource-group
    assert!(has_permission_set("azure-resource-group/provision"));
    assert!(has_permission_set("azure_resource_group/provision"));
    assert!(has_permission_set("azure-resource-group/management"));
    assert!(has_permission_set("azure_resource_group/management"));

    // Test azure-storage-account
    assert!(has_permission_set("azure-storage-account/provision"));
    assert!(has_permission_set("azure_storage_account/provision"));
    assert!(has_permission_set("azure-storage-account/management"));
    assert!(has_permission_set("azure_storage_account/management"));

    // Verify they return the same permission sets
    let rg_hyphenated = get_permission_set("azure-resource-group/provision").unwrap();
    let rg_underscore = get_permission_set("azure_resource_group/provision").unwrap();
    assert_eq!(rg_hyphenated.id, rg_underscore.id);

    let sa_hyphenated = get_permission_set("azure-storage-account/provision").unwrap();
    let sa_underscore = get_permission_set("azure_storage_account/provision").unwrap();
    assert_eq!(sa_hyphenated.id, sa_underscore.id);
}

#[test]
fn test_artifact_registry_naming_conventions() {
    // Test that both hyphenated and underscore formats work for artifact-registry

    assert!(has_permission_set("artifact-registry/provision"));
    assert!(has_permission_set("artifact_registry/provision"));
    assert!(has_permission_set("artifact-registry/management"));
    assert!(has_permission_set("artifact_registry/management"));
    assert!(has_permission_set("artifact-registry/push"));
    assert!(has_permission_set("artifact_registry/push"));
    assert!(has_permission_set("artifact-registry/pull"));
    assert!(has_permission_set("artifact_registry/pull"));

    // Verify they return the same permission sets
    let ar_hyphenated = get_permission_set("artifact-registry/provision").unwrap();
    let ar_underscore = get_permission_set("artifact_registry/provision").unwrap();
    assert_eq!(ar_hyphenated.id, ar_underscore.id);
}
