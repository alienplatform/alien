mod common;

use alien_permissions::{
    generators::{GcpBindingResourceKind, GcpBindingTargetScope, GcpRuntimePermissionsGenerator},
    get_permission_set, list_permission_set_ids, BindingTarget,
};
use common::*;
use rstest::rstest;

#[rstest]
#[case::stack_binding(BindingTarget::Stack, GcpBindingTargetScope::Project)]
#[case::resource_binding(BindingTarget::Resource, GcpBindingTargetScope::CurrentResource)]
fn gcp_storage_data_read_uses_stack_scoped_custom_role(
    #[case] binding_target: BindingTarget,
    #[case] expected_target: GcpBindingTargetScope,
) {
    let generator = GcpRuntimePermissionsGenerator::new();
    let permission_set = create_gcp_storage_data_read_permission_set();
    let context = create_test_context();

    let result = generator
        .generate_bindings(&permission_set, binding_target, &context)
        .expect("should generate GCP IAM bindings successfully");

    assert_eq!(result.bindings.len(), 1);
    assert!(result.bindings[0]
        .role
        .starts_with("projects/my-project/roles/role_my_stack_storage_data_read"));
    assert_eq!(result.bindings[0].target, expected_target);
}

#[test]
fn gcp_custom_role_metadata_uses_stack_name_and_permission_description() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let permission_set = get_permission_set("storage/data-write").expect("permission set exists");
    let context = create_test_context();

    let roles = generator
        .generate_custom_roles(permission_set, &context)
        .expect("should generate storage data-write roles");

    let storage_role = roles
        .iter()
        .find(|role| role.role_id == "role_my_stack_storage_data_write_part1")
        .expect("storage permissions role exists");
    assert_eq!(
        storage_role.title,
        "byoc-database: Storage data write (part 1)"
    );
    assert_eq!(
        storage_role.description,
        "Allows reading and writing data to storage buckets and containers. Stack: byoc-database. Deployment prefix: my-stack. Permission set: storage/data-write."
    );
}

#[test]
fn gcp_permission_set_can_compile_explicit_command_permissions() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let permission_set =
        get_permission_set("worker/dispatch-command").expect("permission set exists");
    let context = create_test_context();

    let roles = generator
        .generate_custom_roles(permission_set, &context)
        .expect("should generate worker command dispatch plan");
    assert!(roles.is_empty());

    let result = generator
        .generate_bindings(permission_set, BindingTarget::Resource, &context)
        .expect("should generate queue writer bindings");

    assert_eq!(result.bindings.len(), 1);
    assert_eq!(result.bindings[0].role, "roles/pubsub.publisher");
    assert!(result
        .bindings
        .iter()
        .all(|binding| binding.target == GcpBindingTargetScope::CurrentResource));
}

#[rstest]
#[case::stack_binding(BindingTarget::Stack, GcpBindingTargetScope::Project)]
#[case::resource_binding(BindingTarget::Resource, GcpBindingTargetScope::CurrentResource)]
fn gcp_storage_heartbeat_custom_role_omits_object_permissions(
    #[case] binding_target: BindingTarget,
    #[case] expected_target: GcpBindingTargetScope,
) {
    let generator = GcpRuntimePermissionsGenerator::new();
    let permission_set = get_permission_set("storage/heartbeat").expect("permission set exists");
    let context = create_test_context();

    let result = generator
        .generate_bindings(permission_set, binding_target, &context)
        .expect("should generate safe storage heartbeat binding");

    let role = generator
        .generate_custom_role(permission_set, &context)
        .expect("should generate storage heartbeat role");
    assert!(!role
        .included_permissions
        .iter()
        .any(|permission| permission.starts_with("storage.objects.")));
    assert_eq!(result.bindings.len(), 1);
    assert_eq!(result.bindings[0].target, expected_target);
}

#[test]
fn gcp_storage_management_uses_exact_custom_role_permissions() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let permission_set = get_permission_set("storage/management").expect("permission set exists");
    let context = create_test_context();

    let result = generator
        .generate_bindings(permission_set, BindingTarget::Resource, &context)
        .expect("should generate conditioned storage management binding");

    assert_eq!(result.bindings.len(), 1);
    let binding = &result.bindings[0];
    assert!(binding
        .role
        .starts_with("projects/my-project/roles/role_my_stack_storage_management"));
    assert_eq!(binding.target, GcpBindingTargetScope::CurrentResource);
    let role = generator
        .generate_custom_role(permission_set, &context)
        .expect("should generate storage management role");
    assert!(!role
        .included_permissions
        .iter()
        .any(|permission| permission.starts_with("storage.objects.")));
}

#[test]
fn gcp_multi_entry_permission_sets_keep_project_and_resource_roles_separate() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let permission_set =
        get_permission_set("artifact-registry/pull").expect("permission set exists");
    let context = create_test_context().with_resource_name("app-images");

    let roles = generator
        .generate_custom_roles(permission_set, &context)
        .expect("should generate split custom roles");
    let bindings = generator
        .generate_bindings(permission_set, BindingTarget::Stack, &context)
        .expect("should generate split bindings");

    let project_binding = bindings
        .bindings
        .iter()
        .find(|binding| {
            binding.target == GcpBindingTargetScope::Project
                && roles
                    .iter()
                    .find(|role| role.name == binding.role)
                    .is_some_and(|role| {
                        role.included_permissions
                            == vec![
                                "iam.serviceAccounts.actAs",
                                "iam.serviceAccounts.getAccessToken",
                            ]
                    })
        })
        .expect("project-scoped helper binding");
    let project_role = roles
        .iter()
        .find(|role| role.name == project_binding.role)
        .expect("project role exists");

    assert_eq!(
        project_role.included_permissions,
        vec![
            "iam.serviceAccounts.actAs",
            "iam.serviceAccounts.getAccessToken"
        ]
    );
    assert!(!project_role
        .included_permissions
        .iter()
        .any(|permission| permission.starts_with("artifactregistry.")));
}

#[test]
fn gcp_queue_data_write_resource_bindings_split_topic_and_subscription() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let permission_set = get_permission_set("queue/data-write").expect("permission set exists");
    let context = create_test_context().with_resource_name("jobs");

    let result = generator
        .generate_bindings(permission_set, BindingTarget::Resource, &context)
        .expect("should generate split queue bindings");

    assert_eq!(result.bindings.len(), 3);
    let topic_roles: Vec<_> = result
        .bindings
        .iter()
        .filter(|binding| binding.resource_kind == Some(GcpBindingResourceKind::PubsubTopic))
        .map(|binding| binding.role.as_str())
        .collect();
    let subscription_roles: Vec<_> = result
        .bindings
        .iter()
        .filter(|binding| binding.resource_kind == Some(GcpBindingResourceKind::PubsubSubscription))
        .map(|binding| binding.role.as_str())
        .collect();

    assert_eq!(topic_roles, vec!["roles/pubsub.publisher"]);
    assert_eq!(
        subscription_roles,
        vec!["roles/pubsub.subscriber", "roles/pubsub.viewer"]
    );
    assert!(result
        .bindings
        .iter()
        .all(|binding| binding.target == GcpBindingTargetScope::CurrentResource));
}

#[test]
fn gcp_artifact_registry_pull_resource_binding_omits_project_helper_binding() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let permission_set =
        get_permission_set("artifact-registry/pull").expect("permission set exists");
    let context = create_test_context().with_resource_name("app-images");

    let result = generator
        .generate_bindings(permission_set, BindingTarget::Resource, &context)
        .expect("should generate repository-only resource binding");

    assert_eq!(result.bindings.len(), 1);
    assert_eq!(
        result.bindings[0].resource_kind,
        Some(GcpBindingResourceKind::ArtifactRegistryRepository)
    );
    assert_eq!(
        result.bindings[0].target,
        GcpBindingTargetScope::CurrentResource
    );
}

#[test]
fn gcp_artifact_registry_management_stack_binding_is_project_scoped() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let permission_set =
        get_permission_set("artifact-registry/management").expect("permission set exists");
    let context = create_test_context();

    let result = generator
        .generate_bindings(permission_set, BindingTarget::Stack, &context)
        .expect("should generate project-scoped stack binding");

    assert_eq!(result.bindings.len(), 1);
    assert_eq!(result.bindings[0].target, GcpBindingTargetScope::Project);
}

#[rstest]
#[case::data_read("storage/data-read")]
#[case::data_write("storage/data-write")]
fn gcp_storage_resource_grant_plan_isolates_project_sign_blob_helper(
    #[case] permission_set_id: &str,
) {
    let generator = GcpRuntimePermissionsGenerator::new();
    let permission_set = get_permission_set(permission_set_id).expect("permission set exists");
    let context = create_test_context().with_resource_name("app-bucket");

    let grant_plan = generator
        .generate_grant_plan(permission_set, BindingTarget::Resource, &context)
        .expect("should generate storage grant plan");

    let resource_bindings = grant_plan.bindings_for_target(GcpBindingTargetScope::CurrentResource);
    let project_bindings = grant_plan.bindings_for_target(GcpBindingTargetScope::Project);

    assert_eq!(resource_bindings.len(), 1);
    assert_eq!(project_bindings.len(), 1);

    let resource_roles = grant_plan.custom_roles_for_bindings(&resource_bindings);
    assert_eq!(resource_roles.len(), 1);
    assert!(resource_roles[0]
        .included_permissions
        .iter()
        .any(|permission| permission == "storage.objects.get"));

    let project_roles = grant_plan.custom_roles_for_bindings(&project_bindings);
    assert_eq!(project_roles.len(), 1);
    assert_eq!(
        project_roles[0].included_permissions,
        vec!["iam.serviceAccounts.signBlob"]
    );
    assert!(!project_roles[0]
        .included_permissions
        .iter()
        .any(|permission| permission.starts_with("storage.objects.")));
}

#[test]
fn gcp_resource_target_project_bindings_do_not_include_sensitive_data_permissions() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let context = create_test_context().with_resource_name("current-resource");
    let mut mixed_target_sets = Vec::new();

    for permission_set_id in list_permission_set_ids() {
        let permission_set = get_permission_set(permission_set_id).expect("permission set exists");
        if permission_set.platforms.gcp.is_none() {
            continue;
        }

        let grant_plan = generator
            .generate_grant_plan(permission_set, BindingTarget::Resource, &context)
            .expect("GCP resource grant plan should compile");
        let resource_bindings =
            grant_plan.bindings_for_target(GcpBindingTargetScope::CurrentResource);
        let project_bindings = grant_plan.bindings_for_target(GcpBindingTargetScope::Project);
        let is_mixed_target = !resource_bindings.is_empty() && !project_bindings.is_empty();
        if is_mixed_target {
            mixed_target_sets.push(permission_set_id.to_string());
        }

        if !is_mixed_target || !is_resource_data_permission_set(permission_set_id) {
            continue;
        }

        let project_roles = grant_plan.custom_roles_for_bindings(&project_bindings);
        for role in project_roles {
            assert!(
                !role
                    .included_permissions
                    .iter()
                    .any(|permission| is_sensitive_resource_data_permission(permission)),
                "permission set '{}' project-scoped role '{}' included sensitive data permissions: {:?}",
                permission_set_id,
                role.role_id,
                role.included_permissions
            );
        }
    }

    assert!(
        mixed_target_sets.contains(&"storage/data-read".to_string()),
        "storage/data-read should exercise mixed resource/project grant envelopes"
    );
    assert!(
        mixed_target_sets.contains(&"storage/data-write".to_string()),
        "storage/data-write should exercise mixed resource/project grant envelopes"
    );
}

#[test]
fn gcp_single_custom_role_helper_rejects_multi_entry_sets() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let permission_set =
        get_permission_set("artifact-registry/pull").expect("permission set exists");
    let context = create_test_context().with_resource_name("app-images");

    let error = generator
        .generate_custom_role(permission_set, &context)
        .expect_err("multi-entry permission set should not collapse into one custom role");

    assert!(error
        .to_string()
        .contains("generates multiple custom roles"));
}

#[rstest]
#[case::kv_heartbeat("kv/heartbeat", "datastore.entities.")]
#[case::kv_management("kv/management", "datastore.entities.")]
#[case::vault_heartbeat("vault/heartbeat", "secretmanager.versions.access")]
#[case::vault_management("vault/management", "secretmanager.versions.access")]
fn gcp_control_plane_sets_omit_sensitive_content_permissions(
    #[case] permission_set_id: &str,
    #[case] forbidden_prefix_or_permission: &str,
) {
    let generator = GcpRuntimePermissionsGenerator::new();
    let permission_set = get_permission_set(permission_set_id).expect("permission set exists");
    let context = create_test_context();

    let roles = generator
        .generate_custom_roles(permission_set, &context)
        .expect("should generate metadata role plan");

    assert!(!roles
        .iter()
        .flat_map(|role| role.included_permissions.iter())
        .any(|permission| permission.starts_with(forbidden_prefix_or_permission)));
}

#[test]
fn gcp_vault_data_write_resource_condition_uses_project_number_and_vault_prefix() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let permission_set = get_permission_set("vault/data-write")
        .expect("vault/data-write permission set should exist");
    let context = create_test_context().with_resource_name("customer-vault");

    let result = generator
        .generate_bindings(permission_set, BindingTarget::Resource, &context)
        .expect("should generate GCP vault data-write binding successfully");

    assert_eq!(result.bindings.len(), 3);
    let create_binding = result
        .bindings
        .iter()
        .find(|binding| binding.condition.is_none())
        .expect("project-scoped create binding exists");
    assert_eq!(create_binding.target, GcpBindingTargetScope::Project);

    let conditioned_binding = result
        .bindings
        .iter()
        .find(|binding| {
            binding.condition.is_some()
                && binding
                    .role
                    .starts_with("projects/my-project/roles/role_my_stack_vault_data_write")
        })
        .expect("prefix-conditioned write binding exists");
    assert!(conditioned_binding
        .role
        .starts_with("projects/my-project/roles/role_my_stack_vault_data_write"));
    assert_eq!(conditioned_binding.target, GcpBindingTargetScope::Project);
    let condition = conditioned_binding.condition.as_ref().unwrap();
    assert_eq!(condition.title, "ResourceVaultSecrets");
    assert_eq!(
        condition.expression,
        "(resource.type == \"secretmanager.googleapis.com/Secret\" || resource.type == \"secretmanager.googleapis.com/SecretVersion\") && resource.name.startsWith(\"projects/123456789012/secrets/customer-vault-\")"
    );
}

#[test]
fn gcp_vault_management_resource_binding_is_project_conditioned() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let permission_set =
        get_permission_set("vault/management").expect("vault/management permission set exists");
    let context = create_test_context().with_resource_name("customer-vault");

    let result = generator
        .generate_bindings(permission_set, BindingTarget::Resource, &context)
        .expect("should generate GCP vault management binding successfully");

    assert_eq!(result.bindings.len(), 1);
    let binding = &result.bindings[0];
    assert_eq!(binding.target, GcpBindingTargetScope::Project);
    assert_eq!(binding.role, "roles/secretmanager.viewer");
    let condition = binding.condition.as_ref().unwrap();
    assert_eq!(condition.title, "ResourceVaultSecretsManagement");
    assert_eq!(
        condition.expression,
        "resource.type == \"secretmanager.googleapis.com/Secret\" && resource.name.startsWith(\"projects/123456789012/secrets/customer-vault-\")"
    );
}

#[test]
fn gcp_service_account_member_generation_is_still_available_for_runtime_callers() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let permission_set = create_gcp_storage_data_read_permission_set();
    let context = create_test_context();

    let result = generator
        .generate_bindings(&permission_set, BindingTarget::Resource, &context)
        .expect("should generate GCP IAM bindings successfully");

    assert_eq!(
        result.bindings[0].members,
        vec!["serviceAccount:my-sa@my-project.iam.gserviceaccount.com"]
    );
}

#[test]
fn gcp_missing_platform_error() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let permission_set = create_aws_storage_data_read_permission_set();
    let context = create_test_context();

    let result = generator.generate_bindings(&permission_set, BindingTarget::Stack, &context);

    assert!(result.is_err());
    let error = result.unwrap_err();
    assert!(error
        .to_string()
        .contains("Platform 'gcp' is not supported"));
}

#[test]
fn gcp_missing_permissions_fail_closed() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let mut permission_set = create_gcp_storage_data_read_permission_set();
    permission_set.platforms.gcp.as_mut().unwrap()[0]
        .grant
        .permissions = None;
    let context = create_test_context();

    let result = generator.generate_bindings(&permission_set, BindingTarget::Stack, &context);

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("has no permissions"));
}

#[test]
fn gcp_permission_grant_parses_predefined_and_residual_fields() {
    let permission_set: alien_core::PermissionSet = json5::from_str(
        r#"{
            id: "queue/data-write",
            description: "Queue writer",
            platforms: {
                gcp: [{
                    binding: {
                        stack: { scope: "projects/${projectName}" },
                        resource: { scope: "projects/${projectName}/topics/${resourceName}" }
                    },
                    grant: {
                        predefinedRoles: ["roles/pubsub.publisher"],
                        residualPermissions: ["pubsub.topics.get"]
                    }
                }]
            }
        }"#,
    )
    .expect("permission set parses");

    let grant = &permission_set.platforms.gcp.unwrap()[0].grant;
    assert_eq!(
        grant.predefined_roles.as_deref(),
        Some(&["roles/pubsub.publisher".to_string()][..])
    );
    assert_eq!(
        grant.residual_permissions.as_deref(),
        Some(&["pubsub.topics.get".to_string()][..])
    );
}

#[test]
fn gcp_permission_grant_rejects_invalid_predefined_role_name() {
    let generator = GcpRuntimePermissionsGenerator::new();
    let mut permission_set = create_gcp_storage_data_read_permission_set();
    let grant = &mut permission_set.platforms.gcp.as_mut().unwrap()[0].grant;
    grant.permissions = None;
    grant.predefined_roles = Some(vec!["pubsub.publisher".to_string()]);
    let context = create_test_context();

    let result = generator.generate_bindings(&permission_set, BindingTarget::Stack, &context);

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("invalid predefined role"));
}

fn is_sensitive_resource_data_permission(permission: &str) -> bool {
    matches!(
        permission,
        "storage.objects.get"
            | "storage.objects.list"
            | "secretmanager.versions.access"
            | "datastore.entities.get"
            | "datastore.entities.list"
            | "pubsub.subscriptions.consume"
    )
}

fn is_resource_data_permission_set(permission_set_id: &str) -> bool {
    permission_set_id.starts_with("storage/")
        || permission_set_id.starts_with("vault/")
        || permission_set_id.starts_with("kv/")
        || permission_set_id.starts_with("queue/")
}
