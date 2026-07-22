use alien_permissions::{get_permission_set, has_permission_set, list_permission_set_ids};

#[test]
fn test_ai_permission_sets_resolve_via_registry() {
    assert!(has_permission_set("ai/provision"));
    assert!(has_permission_set("ai/management"));
    assert!(has_permission_set("ai/heartbeat"));
    assert!(has_permission_set("ai/invoke"));

    let ids = list_permission_set_ids();
    assert!(ids.contains(&"ai/provision"));
    assert!(ids.contains(&"ai/management"));
    assert!(ids.contains(&"ai/heartbeat"));
    assert!(ids.contains(&"ai/invoke"));
}

#[test]
fn test_ai_invoke_is_inference_only() {
    let invoke = get_permission_set("ai/invoke").expect("ai/invoke must resolve");

    // AWS: inference actions only, enforced as an explicit allowlist. A substring
    // denylist ("contains Create/Delete/Deploy") both over- and under-fires: it
    // rejects `bedrock-mantle:CreateInference`, which IS an inference call (it
    // creates an inference — it runs the model), while still passing any
    // control-plane action whose name happens to lack those words. Adding an entry
    // here is deliberate: it must be a data-plane inference call, never a
    // deployment or other control-plane write, which belong in ai/provision.
    const AWS_INFERENCE_ACTIONS: &[&str] = &[
        "bedrock:InvokeModel",
        "bedrock:InvokeModelWithResponseStream",
        "bedrock-mantle:CreateInference",
    ];
    if let Some(aws) = &invoke.platforms.aws {
        for entry in aws {
            if let Some(actions) = &entry.grant.actions {
                for action in actions {
                    assert!(
                        AWS_INFERENCE_ACTIONS.contains(&action.as_str()),
                        "ai/invoke AWS grant must contain only inference actions; found \
                         {action}, which is not in {AWS_INFERENCE_ACTIONS:?}. If it is a \
                         data-plane inference call, add it deliberately; deployment and \
                         control-plane actions belong in ai/provision."
                    );
                }
            }
        }
    }

    // GCP: must use a custom role (permissions list), never a predefined role.
    // roles/aiplatform.user includes control-plane writes and sensitive-data reads
    // that a workload must never hold.
    if let Some(gcp) = &invoke.platforms.gcp {
        for (i, entry) in gcp.iter().enumerate() {
            assert!(
                entry.grant.predefined_roles.is_none(),
                "ai/invoke GCP entry {i} must not use predefinedRoles (grants over-broad control-plane access); use a permissions list instead"
            );
            let permissions = entry.grant.permissions.as_ref().unwrap_or_else(|| {
                panic!("ai/invoke GCP entry {i} must have a permissions list")
            });
            for perm in permissions {
                // No control-plane write actions.
                assert!(
                    !perm.contains("deploy") && !perm.contains("upload") && !perm.contains("delete"),
                    "ai/invoke GCP grant must not contain control-plane write permissions, found: {perm}"
                );
                // No sensitive-data read actions.
                assert!(
                    !perm.starts_with("datasets.")
                        && !perm.contains("featurestores")
                        && !perm.contains("ragCorpora")
                        && !perm.contains("sessions"),
                    "ai/invoke GCP grant must not contain sensitive-data read permissions, found: {perm}"
                );
            }
        }
    }

    // Azure: must NOT grant deployments/write (the key deploy-on-demand invariant)
    if let Some(azure) = &invoke.platforms.azure {
        for entry in azure {
            if let Some(actions) = &entry.grant.actions {
                for action in actions {
                    assert_ne!(
                        action,
                        "Microsoft.CognitiveServices/accounts/deployments/write",
                        "ai/invoke must not grant deployments/write; that belongs in ai/provision"
                    );
                }
            }
            // predefinedRoles on invoke must not be management-class roles
            if let Some(roles) = &entry.grant.predefined_roles {
                for role in roles {
                    assert_ne!(
                        role, "Contributor",
                        "ai/invoke must not use Contributor role"
                    );
                    assert_ne!(role, "Owner", "ai/invoke must not use Owner role");
                }
            }
        }
    }
}

#[test]
fn test_ai_provision_has_deployment_writes() {
    // The predefined model set is deployed at provision time, so deployments/{write,read}
    // live in ai/provision (control plane), not in ai/management or ai/invoke.
    let provision = get_permission_set("ai/provision").expect("ai/provision must resolve");
    let azure = provision
        .platforms
        .azure
        .as_ref()
        .expect("ai/provision must have Azure platform");
    let has_write = azure.iter().any(|entry| {
        entry
            .grant
            .actions
            .as_ref()
            .map(|actions| {
                actions.contains(
                    &"Microsoft.CognitiveServices/accounts/deployments/write".to_string(),
                )
            })
            .unwrap_or(false)
    });
    assert!(
        has_write,
        "ai/provision Azure must grant deployments/write for the predefined model set"
    );

    // ai/management is read-only metadata; deployment writes live in ai/provision.
    let management = get_permission_set("ai/management").expect("ai/management must resolve");
    let mgmt_azure = management
        .platforms
        .azure
        .as_ref()
        .expect("ai/management must have Azure platform");
    let mgmt_has_write = mgmt_azure.iter().any(|entry| {
        entry
            .grant
            .actions
            .as_ref()
            .map(|actions| {
                actions.contains(
                    &"Microsoft.CognitiveServices/accounts/deployments/write".to_string(),
                )
            })
            .unwrap_or(false)
    });
    assert!(
        !mgmt_has_write,
        "ai/management must not grant deployments/write"
    );
}

#[test]
fn test_ai_invoke_uses_openai_user_role() {
    let invoke = get_permission_set("ai/invoke").expect("ai/invoke must resolve");
    let azure = invoke
        .platforms
        .azure
        .as_ref()
        .expect("ai/invoke must have Azure platform");
    let uses_openai_user = azure.iter().any(|entry| {
        entry
            .grant
            .predefined_roles
            .as_ref()
            .map(|roles| roles.contains(&"Cognitive Services OpenAI User".to_string()))
            .unwrap_or(false)
    });
    assert!(
        uses_openai_user,
        "ai/invoke Azure must use the least-privilege 'Cognitive Services OpenAI User' role"
    );
}

#[test]
fn test_openai_user_role_id_resolves() {
    use alien_permissions::generators::azure_runtime::azure_predefined_role_id;
    assert_eq!(
        azure_predefined_role_id("Cognitive Services OpenAI User"),
        Some("5e0bd9bd-7b93-4f28-af87-19fc36ad61bd"),
        "the OpenAI-User role GUID must be registered"
    );
}

#[test]
fn test_ai_permission_sets_have_all_platforms() {
    for id in ["ai/provision", "ai/management", "ai/heartbeat", "ai/invoke"] {
        let perm_set = get_permission_set(id).unwrap_or_else(|| panic!("{id} must resolve"));
        assert_eq!(perm_set.id, id);
        assert!(!perm_set.description.is_empty(), "{id} must have a description");
        assert!(perm_set.platforms.aws.is_some(), "{id} must have AWS platform");
        assert!(perm_set.platforms.gcp.is_some(), "{id} must have GCP platform");
        assert!(perm_set.platforms.azure.is_some(), "{id} must have Azure platform");
    }
}

#[test]
fn test_registry_basic_functionality() {
    // Test that the registry contains expected permission sets
    assert!(has_permission_set("storage/data-read"));
    assert!(has_permission_set("worker/execute"));
    assert!(has_permission_set("build/provision"));
    assert!(has_permission_set("kubernetes-public-endpoint/management"));
    assert!(has_permission_set("observe/observe"));

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
    let perm_set = get_permission_set("worker/execute");
    assert!(perm_set.is_some());

    let perm_set = perm_set.unwrap();
    assert_eq!(perm_set.id, "worker/execute");
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
    assert!(ids.contains(&"worker/execute"));
    assert!(ids.contains(&"worker/management"));
    assert!(ids.contains(&"worker/provision"));
    assert!(ids.contains(&"kubernetes-public-endpoint/management"));
    assert!(ids.contains(&"observe/observe"));
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
