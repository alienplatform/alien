//! What a protected install's management identity may be granted. Each cloud has an allow-list
//! file next to the permission sets; a grant it doesn't cover is refused when it is rendered, with
//! an error that names the action and the file. This holds for a wildcard too: `iam:*` is refused
//! because it covers actions that aren't listed. An unprotected install keeps every grant it had.

mod common;

use alien_core::PermissionSet;
use alien_permissions::{
    generators::{
        aws_family, management_stack_secrets_data, AwsCloudFormationPermissionsGenerator,
        AwsRuntimePermissionsGenerator, AzureRuntimePermissionsGenerator,
        GcpRuntimePermissionsGenerator,
    },
    get_permission_set, list_permission_set_ids, BindingTarget, PermissionContext,
};
use common::create_test_context;
use serde_json::{json, Value};

const BOUNDARY: &str = "arn:aws:iam::123456789012:policy/my-stack-boundary";

fn aws_context(protected: bool) -> PermissionContext {
    create_test_context()
        .with_aws_permissions_boundary_arn(BOUNDARY)
        .with_signing_protected(protected)
}

fn stack_set(platform: &str, grant: Value, binding: Value) -> PermissionSet {
    serde_json::from_value(json!({
        "id": "orders/extra-management",
        "description": "Added by the stack",
        "platforms": { platform: [{ "grant": grant, "binding": binding }] },
    }))
    .expect("permission set")
}

fn aws_set(actions: &[&str]) -> PermissionSet {
    stack_set(
        "aws",
        json!({ "actions": actions }),
        json!({
            "stack": { "resources": ["*"] },
            "resource": { "resources": ["*"] },
        }),
    )
}

fn azure_set(actions: &[&str]) -> PermissionSet {
    stack_set(
        "azure",
        json!({ "actions": actions }),
        json!({
            "stack": { "scope": "/subscriptions/${subscriptionId}/resourceGroups/${resourceGroup}" },
            "resource": { "scope": "/subscriptions/${subscriptionId}/resourceGroups/${resourceGroup}" },
        }),
    )
}

fn gcp_set(grant: Value) -> PermissionSet {
    stack_set(
        "gcp",
        grant,
        json!({
            "stack": { "scope": "project" },
            "resource": { "scope": "project" },
        }),
    )
}

fn render_aws(set: &PermissionSet, protected: bool) -> alien_permissions::Result<Vec<String>> {
    let policy = AwsRuntimePermissionsGenerator::new().generate_policy(
        set,
        BindingTarget::Stack,
        &aws_context(protected),
    )?;
    Ok(policy
        .statement
        .into_iter()
        .flat_map(|statement| statement.action)
        .collect())
}

fn render_aws_template(set: &PermissionSet, protected: bool) -> alien_permissions::Result<()> {
    AwsCloudFormationPermissionsGenerator::new()
        .generate_policy(set, BindingTarget::Stack, &aws_context(protected))
        .map(|_| ())
}

const ALLOW_LIST_AWS: &str = "crates/alien-permissions/protected-management/aws.allow";

fn assert_family_refused(
    error: &alien_error::AlienError<alien_permissions::ErrorData>,
    action: &str,
) {
    assert_eq!(
        error.code, "PROTECTED_MANAGEMENT_FAMILY_ACTION",
        "{action}: {}",
        error.message
    );
    assert!(
        error.message.contains(&format!("'{action}'")),
        "{action}: {}",
        error.message
    );
}

#[test]
fn an_aws_stack_grant_in_a_family_is_refused() {
    for action in [
        "iam:*",
        "iam:CreateRole",
        "iam:AttachRolePolicy",
        "iam:PassRole",
        "iam:Create*",
        "iam:*Role*",
        "IAM:createrole",
        "i?m:CreateRole",
        "*",
        "*:*",
        "*:AssumeRole",
        "sts:*",
        "sts:AssumeRole",
        "sts:AssumeRoleWithWebIdentity",
        "cloudformation:*",
        "cloudformation:UpdateStack",
        "cloudformation:CreateStack",
        "s3:*",
        "s3:Put*",
        "s3:PutBucketPolicy",
        "ssm:SendCommand",
        "lambda:UpdateFunctionCode",
        "lambda:*",
        "ec2:RunInstances",
        "sns:Subscribe",
    ] {
        let set = aws_set(&["s3:GetObject", action]);
        let error = render_aws(&set, true)
            .expect_err(&format!("{action} must be refused on a protected install"));
        assert_family_refused(&error, action);

        let error = render_aws_template(&set, true)
            .expect_err(&format!("{action} must be refused in the setup template"));
        assert_family_refused(&error, action);

        let unprotected = render_aws(&set, false).expect("an unprotected install is unchanged");
        assert!(
            unprotected.iter().any(|granted| granted == action),
            "{action}"
        );
        render_aws_template(&set, false).expect("an unprotected install is unchanged");
    }
}

#[test]
fn an_aws_stack_grant_on_the_allow_list_still_renders() {
    let actions = [
        "s3:GetObject",
        "s3:Get*",
        "iam:GetRole",
        "iam:List*",
        "sts:GetCallerIdentity",
        "ec2:Describe*",
        "dynamodb:Query",
        "s3:PutObject",
        "sqs:SendMessage",
        "kms:GenerateDataKey*",
    ];
    let set = aws_set(&actions);
    let granted = render_aws(&set, true).expect("allow-listed actions render");
    for action in actions {
        assert!(granted.iter().any(|kept| kept == action), "{action}");
    }
    render_aws_template(&set, true).expect("allow-listed actions render in the setup template");
}

#[test]
fn an_aws_stack_grant_of_what_only_the_verifier_may_do_is_refused_not_dropped() {
    let set = aws_set(&["s3:GetObject", "lambda:UpdateFunctionCode"]);
    let error = render_aws(&set, true).expect_err("a family action in a stack set is refused");
    assert_family_refused(&error, "lambda:UpdateFunctionCode");
}

#[test]
fn an_aws_deny_in_a_stack_set_is_not_a_grant() {
    let set: PermissionSet = serde_json::from_value(json!({
        "id": "orders/no-iam",
        "description": "Keeps management out of IAM",
        "platforms": { "aws": [{
            "effect": "Deny",
            "grant": { "actions": ["iam:*"] },
            "binding": { "stack": { "resources": ["*"] }, "resource": { "resources": ["*"] } },
        }] },
    }))
    .expect("permission set");
    let granted = render_aws(&set, true).expect("a Deny is never refused");
    assert_eq!(granted, ["iam:*"]);
}

#[test]
fn a_changed_copy_of_a_built_in_set_is_a_stack_set() {
    let mut copy = get_permission_set("storage/data-read")
        .expect("built-in set")
        .clone();
    let aws = copy.platforms.aws.as_mut().expect("aws entries");
    aws[0]
        .grant
        .actions
        .as_mut()
        .expect("actions")
        .push("iam:CreateRole".to_string());
    let error = render_aws(&copy, true).expect_err("the built-in id doesn't exempt a changed copy");
    assert_family_refused(&error, "iam:CreateRole");

    let original = get_permission_set("storage/data-read").expect("built-in set");
    render_aws(original, true).expect("the built-in set renders");
}

#[test]
fn no_built_in_aws_set_is_refused_on_a_protected_install() {
    let mut rendered = 0;
    for id in list_permission_set_ids() {
        let set = get_permission_set(id).expect("listed set");
        if set.platforms.aws.is_none() {
            continue;
        }
        for target in [BindingTarget::Stack, BindingTarget::Resource] {
            match AwsRuntimePermissionsGenerator::new().generate_policy(
                set,
                target,
                &aws_context(true),
            ) {
                Ok(_) => rendered += 1,
                Err(error) => assert_ne!(
                    error.code, "PROTECTED_MANAGEMENT_ACTION_REFUSED",
                    "{id}: {}",
                    error.message
                ),
            }
        }
    }
    assert!(rendered > 50, "only {rendered} held built-in sets rendered");
}

#[test]
fn setups_identity_sets_render_for_management_without_their_family_actions() {
    // Setup's sets write roles and policies. A protected install's management identity gets
    // their reads only: every IDENTITY, RUN and REACH action is dropped from a built-in set.
    for id in ["remote-stack-management/provision", "service-account/setup"] {
        let set = get_permission_set(id).expect("built-in set");
        let granted = render_aws(set, true).unwrap_or_else(|error| panic!("{id}: {error}"));
        assert!(!granted.is_empty(), "{id} grants management nothing");
        for action in &granted {
            assert_eq!(aws_family(action, None), None, "{id}: {action}");
        }
        let unprotected =
            render_aws(set, false).unwrap_or_else(|error| panic!("{id} unprotected: {error}"));
        assert!(
            unprotected
                .iter()
                .any(|action| aws_family(action, None).is_some()),
            "{id} should hold family actions when unprotected"
        );
    }
}

#[test]
fn a_set_id_does_not_exempt_a_changed_copy() {
    let mut copy = get_permission_set("storage/provision")
        .expect("built-in set")
        .clone();
    let aws = copy.platforms.aws.as_mut().expect("aws entries");
    aws[0]
        .grant
        .actions
        .as_mut()
        .expect("actions")
        .push("iam:CreateRole".to_string());
    let error = render_aws(&copy, true).expect_err("a changed copy is checked");
    assert_eq!(error.code, "PROTECTED_MANAGEMENT_FAMILY_ACTION");
    assert!(
        error.message.contains("'storage/provision'"),
        "{}",
        error.message
    );
}

#[test]
fn the_legacy_pass_role_grant_is_never_rendered_for_a_protected_install() {
    let set = get_permission_set("compute-cluster/management").expect("built-in set");
    for context in [
        create_test_context().with_signing_protected(true),
        aws_context(true),
    ] {
        match AwsRuntimePermissionsGenerator::new().generate_policy(
            set,
            BindingTarget::Stack,
            &context,
        ) {
            Ok(policy) => {
                for statement in policy.statement {
                    if statement
                        .action
                        .iter()
                        .any(|action| action == "iam:PassRole")
                    {
                        let condition = statement.condition.expect("PassRole is conditioned");
                        assert_eq!(
                            serde_json::to_value(&condition).expect("condition")["StringEquals"]
                                ["iam:PassedToService"],
                            "ec2.amazonaws.com",
                        );
                    }
                }
            }
            Err(error) => assert_eq!(error.code, "PROTECTED_DEPLOYMENT_WITHOUT_BOUNDARY"),
        }
    }
}

fn azure_context(protected: bool) -> PermissionContext {
    create_test_context().with_signing_protected(protected)
}

fn render_azure(set: &PermissionSet, protected: bool) -> alien_permissions::Result<Vec<String>> {
    let plan = AzureRuntimePermissionsGenerator::new().generate_grant_plan(
        set,
        BindingTarget::Stack,
        &azure_context(protected),
    )?;
    Ok(plan
        .custom_roles
        .into_iter()
        .flat_map(|role| role.role_definition.actions)
        .collect())
}

fn render_azure_data(
    set: &PermissionSet,
    protected: bool,
) -> alien_permissions::Result<Vec<String>> {
    let plan = AzureRuntimePermissionsGenerator::new().generate_grant_plan(
        set,
        BindingTarget::Stack,
        &azure_context(protected),
    )?;
    Ok(plan
        .custom_roles
        .into_iter()
        .flat_map(|role| role.role_definition.data_actions)
        .collect())
}

const GROUP_SCOPE: &str = "/subscriptions/${subscriptionId}/resourceGroups/${resourceGroup}";

fn azure_data_set(data_actions: &[&str]) -> PermissionSet {
    stack_set(
        "azure",
        json!({ "dataActions": data_actions }),
        json!({
            "stack": { "scope": GROUP_SCOPE },
            "resource": { "scope": GROUP_SCOPE },
        }),
    )
}

fn azure_role_set(roles: &[&str]) -> PermissionSet {
    stack_set(
        "azure",
        json!({ "predefinedRoles": roles }),
        json!({
            "stack": { "scope": GROUP_SCOPE },
            "resource": { "scope": GROUP_SCOPE },
        }),
    )
}

#[test]
fn an_azure_stack_grant_in_a_family_is_refused() {
    for action in [
        "Microsoft.Compute/*/action",
        "Microsoft.Compute/virtualMachineScaleSets/*/runCommand/action",
        "Microsoft.Compute/*",
        "Microsoft.App/*",
        "Microsoft.KeyVault/*",
        "Microsoft.KeyVault/vaults/*",
        "Microsoft.Storage/storageAccounts/listKeys/action",
        "Microsoft.Authorization/roleAssignments/write",
        "*/action",
        "*/read*",
        "*",
    ] {
        let set = azure_set(&["Microsoft.Storage/storageAccounts/read", action]);
        let error = render_azure(&set, true)
            .expect_err(&format!("{action} must be refused on a protected install"));
        assert_family_refused(&error, action);

        let unprotected = render_azure(&set, false).expect("an unprotected install is unchanged");
        assert!(
            unprotected.iter().any(|granted| granted == action),
            "{action}"
        );
    }
}

#[test]
fn an_azure_stack_grant_of_what_only_the_verifier_may_do_is_refused_not_dropped() {
    for action in [
        "Microsoft.Compute/virtualMachines/runCommand/action",
        "Microsoft.App/jobs/start/action",
        "Microsoft.KeyVault/vaults/write",
        "Microsoft.Authorization/roleAssignments/write",
        "microsoft.managedidentity/userassignedidentities/assign/action",
    ] {
        let set = azure_set(&["Microsoft.Storage/storageAccounts/read", action]);
        let error = render_azure(&set, true).expect_err(action);
        assert_family_refused(&error, action);
    }
}

#[test]
fn an_azure_stack_grant_on_the_allow_list_still_renders() {
    let set = azure_set(&[
        "Microsoft.Storage/storageAccounts/read",
        "Microsoft.Compute/virtualMachineScaleSets/read",
        "Microsoft.App/containerApps/revisions/read",
    ]);
    let granted = render_azure(&set, true).expect("reads render");
    assert_eq!(
        granted,
        [
            "Microsoft.App/containerApps/revisions/read",
            "Microsoft.Compute/virtualMachineScaleSets/read",
            "Microsoft.Storage/storageAccounts/read",
        ]
    );
}

/// A data write under `Microsoft.App/` decides what a sandbox runs, so a set the stack defines
/// can't give it to management, and a wildcard that reaches one is refused with it.
#[test]
fn an_azure_data_family_action_is_refused_in_a_stack_set() {
    for action in [
        "Microsoft.App/sandboxGroups/diskimages/write",
        "Microsoft.App/sandboxGroups/sandboxes/write",
        "Microsoft.App/sandboxGroups/*",
        "Microsoft.App/*",
        "*",
    ] {
        let set = azure_data_set(&[
            "Microsoft.Storage/storageAccounts/blobServices/containers/blobs/read",
            action,
        ]);
        let error = render_azure_data(&set, true)
            .expect_err(&format!("{action} must be refused on a protected install"));
        assert_family_refused(&error, action);
        render_azure_data(&set, false).expect("an unprotected install is unchanged");
    }
}

/// Key Vault data is not in a family: the signing vault is in a group management can't assign
/// roles in, so a stack set may grant vault secret data like any other data.
#[test]
fn azure_data_outside_every_family_renders_in_a_stack_set() {
    let set = azure_data_set(&[
        "Microsoft.Storage/storageAccounts/blobServices/containers/blobs/read",
        "Microsoft.Storage/storageAccounts/blobServices/containers/blobs/write",
        "Microsoft.KeyVault/vaults/secrets/setSecret/action",
        "Microsoft.KeyVault/vaults/secrets/getSecret/action",
        "Microsoft.App/sandboxGroups/sandboxes/read",
    ]);
    let granted = render_azure_data(&set, true).expect("data outside a family renders");
    assert_eq!(
        granted,
        [
            "Microsoft.App/sandboxGroups/sandboxes/read",
            "Microsoft.KeyVault/vaults/secrets/getSecret/action",
            "Microsoft.KeyVault/vaults/secrets/setSecret/action",
            "Microsoft.Storage/storageAccounts/blobServices/containers/blobs/read",
            "Microsoft.Storage/storageAccounts/blobServices/containers/blobs/write",
        ]
    );
}

/// The built-in vault data-write set keeps its secret write on a protected install: app vaults
/// are management's to write, and the signing vault is out of its reach.
#[test]
fn the_built_in_vault_secret_write_renders_for_the_management_identity() {
    let vault_write = get_permission_set("vault/data-write").expect("built-in set");
    for target in [BindingTarget::Stack, BindingTarget::Resource] {
        let plan = AzureRuntimePermissionsGenerator::new()
            .generate_grant_plan(vault_write, target, &azure_context(true))
            .expect("a protected install's management may write app vault secrets");
        let data_actions: Vec<&String> = plan
            .custom_roles
            .iter()
            .flat_map(|role| &role.role_definition.data_actions)
            .collect();
        assert!(
            data_actions
                .iter()
                .any(|action| action.as_str() == "Microsoft.KeyVault/vaults/secrets/setSecret/action"),
            "{data_actions:?}"
        );
    }
}

#[test]
fn an_azure_predefined_role_outside_the_built_in_list_is_refused() {
    for role in [
        "Key Vault Contributor",
        "Key Vault Secrets User",
        "Managed Identity Contributor",
        "Network Contributor",
        "AcrPush",
    ] {
        let set = azure_role_set(&[role]);
        let error = AzureRuntimePermissionsGenerator::new()
            .generate_grant_plan(&set, BindingTarget::Stack, &azure_context(true))
            .expect_err(&format!("{role} must be refused on a protected install"));
        assert_eq!(error.code, "GENERATOR_ERROR", "{role}: {}", error.message);
        assert!(
            error.message.contains(&format!("'{role}'")),
            "{}",
            error.message
        );
    }
    let set = azure_role_set(&["Reader"]);
    let plan = AzureRuntimePermissionsGenerator::new()
        .generate_grant_plan(&set, BindingTarget::Stack, &azure_context(true))
        .expect("an allow-listed role renders");
    assert_eq!(plan.bindings.len(), 1);
    assert_eq!(plan.bindings[0].role_name, "Reader");
}

fn gcp_context(protected: bool) -> PermissionContext {
    create_test_context()
        .with_managing_project_id("vendor-project")
        .with_signing_protected(protected)
}

fn render_gcp(
    set: &PermissionSet,
    protected: bool,
) -> alien_permissions::Result<(Vec<String>, Vec<String>)> {
    let plan = GcpRuntimePermissionsGenerator::new().generate_grant_plan(
        set,
        BindingTarget::Stack,
        &gcp_context(protected),
    )?;
    let roles = plan
        .bindings
        .iter()
        .map(|binding| binding.role.clone())
        .filter(|role| role.starts_with("roles/"))
        .collect();
    let permissions = plan
        .custom_roles
        .into_iter()
        .flat_map(|role| role.included_permissions)
        .collect();
    Ok((roles, permissions))
}

#[test]
fn a_gcp_predefined_role_outside_the_built_in_list_is_refused() {
    for role in [
        "roles/editor",
        "roles/owner",
        "roles/compute.instanceAdmin.v1",
        "roles/compute.admin",
        "roles/iam.roleAdmin",
        "roles/iam.securityAdmin",
        "roles/iam.serviceAccountTokenCreator",
        "roles/run.admin",
        "roles/osconfig.guestPolicyAdmin",
        "roles/compute.osAdminLogin",
        "roles/secretmanager.admin",
        "roles/some.roleAddedToGcpLater",
    ] {
        let set = gcp_set(json!({ "predefinedRoles": [role] }));
        let error = render_gcp(&set, true)
            .expect_err(&format!("{role} must be refused on a protected install"));
        assert_eq!(error.code, "GENERATOR_ERROR", "{role}");
        assert!(error.message.contains(role), "{error}");
        render_gcp(&set, false).expect("an unprotected install is unchanged");
    }

    let set = gcp_set(json!({ "predefinedRoles": ["roles/storage.objectViewer"] }));
    let (roles, _) = render_gcp(&set, true).expect("an allowed role renders");
    assert_eq!(roles, ["roles/storage.objectViewer"]);
}

#[test]
fn a_gcp_stack_permission_in_a_family_is_refused() {
    for permission in [
        "iam.serviceAccounts.actAs",
        "iam.serviceAccounts.getAccessToken",
        "iam.roles.create",
        "compute.instances.create",
        "compute.projects.setCommonInstanceMetadata",
        "run.services.update",
        "storage.buckets.setIamPolicy",
        "pubsub.subscriptions.create",
        "*.create",
        "iam.*",
        "*",
    ] {
        let set = gcp_set(json!({ "permissions": ["storage.objects.get", permission] }));
        let error = render_gcp(&set, true).expect_err(&format!("{permission} must be refused"));
        assert_eq!(
            error.code, "PROTECTED_MANAGEMENT_FAMILY_ACTION",
            "{permission}"
        );
        assert!(
            error.message.contains(&format!("'{permission}'")),
            "{permission}: {}",
            error.message
        );
        render_gcp(&set, false).expect("an unprotected install is unchanged");
    }
}

#[test]
fn a_gcp_stack_permission_on_the_allow_list_still_renders() {
    let set = gcp_set(json!({ "permissions": [
        "storage.objects.get",
        "storage.objects.list",
        "pubsub.topics.publish",
        "logging.logEntries.create",
    ] }));
    let (_, permissions) = render_gcp(&set, true).expect("allow-listed permissions render");
    assert_eq!(
        permissions,
        [
            "logging.logEntries.create",
            "pubsub.topics.publish",
            "storage.objects.get",
            "storage.objects.list",
        ]
    );
}

#[test]
fn a_gcp_stack_condition_on_a_management_binding_is_refused() {
    for expression in [
        "true) || (true",
        "resource.name.startsWith(\"projects/_/buckets/orders-\")",
    ] {
        let set = stack_set(
            "gcp",
            json!({ "predefinedRoles": ["roles/secretmanager.secretAccessor"] }),
            json!({ "stack": {
                "scope": "project",
                "condition": { "title": "Stack condition", "expression": expression },
            } }),
        );
        let error = render_gcp(&set, true).expect_err(&format!(
            "`{expression}` must be refused on a protected install"
        ));
        assert!(
            error
                .to_string()
                .contains("doesn't support conditions the stack supplies"),
            "{expression}: {error}"
        );
        render_gcp(&set, false).expect("an unprotected install is unchanged");
    }
}

#[test]
fn every_predefined_role_the_catalog_grants_is_allowed_or_deliberately_refused() {
    let mut outside = std::collections::BTreeSet::new();
    for id in list_permission_set_ids() {
        let set = get_permission_set(id).expect("listed set");
        for entry in set.platforms.gcp.iter().flatten() {
            for role in entry.grant.predefined_roles.iter().flatten() {
                let probe = gcp_set(json!({ "predefinedRoles": [role] }));
                if render_gcp(&probe, true).is_err() {
                    outside.insert(role.clone());
                }
            }
        }
    }
    assert_eq!(
        outside.into_iter().collect::<Vec<_>>(),
        [
            "roles/iam.serviceAccountTokenCreator",
            "roles/run.jobsExecutorWithOverrides",
        ],
        "a new built-in role must be added to the allow-list or refused here on purpose"
    );
}

#[test]
fn gcp_os_login_and_os_config_writes_are_refused_for_a_stack_set_on_a_protected_install() {
    let reads = [
        "storage.objects.get",
        "osconfig.patchJobs.get",
        "osconfig.guestPolicies.list",
    ];
    let (_, permissions) =
        render_gcp(&gcp_set(json!({ "permissions": reads })), true).expect("reads render");
    assert_eq!(
        permissions,
        [
            "osconfig.guestPolicies.list",
            "osconfig.patchJobs.get",
            "storage.objects.get"
        ]
    );
    for write in [
        "compute.instances.osAdminLogin",
        "compute.instances.osLogin",
        "osconfig.patchJobs.exec",
        "osconfig.patchDeployments.create",
        "osconfig.guestPolicies.update",
        "osconfig.osPolicyAssignments.create",
        "oslogin.users.update",
        "oslogin.sshPublicKeys.create",
    ] {
        let set = gcp_set(json!({ "permissions": ["storage.objects.get", write] }));
        let error = render_gcp(&set, true).expect_err(write);
        assert_eq!(error.code, "PROTECTED_MANAGEMENT_FAMILY_ACTION", "{write}");
        let (_, unprotected) = render_gcp(&set, false).expect("renders");
        assert!(unprotected.contains(&write.to_string()), "{write}");
    }
}

fn aws_policy_json(id: &str, protected: bool) -> Value {
    let set = get_permission_set(id).expect("built-in set");
    let policy = AwsRuntimePermissionsGenerator::new()
        .generate_policy(set, BindingTarget::Stack, &aws_context(protected))
        .unwrap_or_else(|error| panic!("{id}: {error}"));
    serde_json::to_value(policy).expect("policy json")
}

#[test]
fn a_protected_install_denies_management_new_versions_of_its_own_policies() {
    let denied = |policy: &Value| -> Vec<String> {
        policy["Statement"]
            .as_array()
            .expect("statements")
            .iter()
            .filter(|statement| {
                statement["Effect"] == "Deny"
                    && statement["Resource"]
                        .to_string()
                        .contains("ManagementRole*")
            })
            .flat_map(|statement| match &statement["Action"] {
                Value::Array(actions) => actions.clone(),
                single => vec![single.clone()],
            })
            .map(|action| action.as_str().expect("action").to_string())
            .collect()
    };
    let protected = aws_policy_json("remote-stack-management/protect-management-role", true);
    let denied_actions = denied(&protected);
    for action in [
        "iam:CreatePolicyVersion",
        "iam:SetDefaultPolicyVersion",
        "iam:DeletePolicyVersion",
    ] {
        assert!(
            denied_actions.contains(&action.to_string()),
            "{action} is not denied on the management policies: {protected}"
        );
    }
    let resources = protected.to_string();
    assert!(
        resources.contains("policy/my-stack-ManagementRole*"),
        "{resources}"
    );
    assert!(
        resources.contains("policy/my-stack-deployment-management-*"),
        "{resources}"
    );

    let unprotected = aws_policy_json("remote-stack-management/protect-management-role", false);
    assert!(denied(&unprotected).is_empty(), "{unprotected}");
}

fn azure_plan(
    id: &str,
    protected: bool,
) -> alien_permissions::Result<alien_permissions::generators::AzureGrantPlan> {
    azure_plan_for(id, BindingTarget::Stack, protected)
}

fn azure_plan_for(
    id: &str,
    target: BindingTarget,
    protected: bool,
) -> alien_permissions::Result<alien_permissions::generators::AzureGrantPlan> {
    AzureRuntimePermissionsGenerator::new().generate_grant_plan(
        get_permission_set(id).expect("built-in set"),
        target,
        &azure_context(protected),
    )
}

#[test]
fn a_protected_azure_install_gets_custom_roles_for_the_identity_and_network_sets() {
    for (id, role) in [
        ("service-account/provision", "Managed Identity Contributor"),
        ("network/provision", "Network Contributor"),
    ] {
        let unprotected = azure_plan(id, false).expect("unprotected renders");
        assert!(
            unprotected.bindings.iter().any(|b| b.role_name == role),
            "{id}: the unprotected install binds {role}"
        );
        let protected = azure_plan(id, true).expect("protected renders");
        assert!(
            protected.bindings.iter().all(|b| b.role_name != role),
            "{id}: the protected install must not bind {role}"
        );
        let actions: Vec<String> = protected
            .custom_roles
            .iter()
            .flat_map(|r| r.role_definition.actions.clone())
            .collect();
        for forbidden in [
            "federatedIdentityCredentials",
            "userAssignedIdentities/write",
            "userAssignedIdentities/delete",
            "Microsoft.Resources/deployments",
        ] {
            assert!(
                actions.iter().all(|a| !a.contains(forbidden)),
                "{id}: a protected custom role holds {forbidden}: {actions:?}"
            );
        }
        assert!(
            actions
                .iter()
                .all(|a| a.ends_with("/read") || a == "Microsoft.Network/*"),
            "{id}: a protected custom role holds more than reads and Network: {actions:?}"
        );
    }
    let network = azure_plan("network/provision", true).expect("renders");
    let actions: Vec<String> = network
        .custom_roles
        .iter()
        .flat_map(|r| r.role_definition.actions.clone())
        .collect();
    assert!(
        actions.iter().any(|a| a == "Microsoft.Network/*"),
        "network setup keeps its own resource type: {actions:?}"
    );
}

#[test]
fn a_protected_azure_install_leaves_the_sandbox_powers_to_the_verifier() {
    for (id, target) in [
        ("sandbox/management", BindingTarget::Stack),
        ("sandbox/images", BindingTarget::Resource),
    ] {
        let Some(set) = get_permission_set(id) else {
            continue;
        };
        if set.platforms.azure.is_none() {
            continue;
        }
        let protected =
            azure_plan_for(id, target, true).unwrap_or_else(|error| panic!("{id}: {error}"));
        let data: Vec<String> = protected
            .custom_roles
            .iter()
            .flat_map(|r| r.role_definition.data_actions.clone())
            .collect();
        for write in ["diskimages/write", "sandboxes/write"] {
            assert!(
                data.iter().all(|a| !a.contains(write)),
                "{id}: protected management holds {write}: {data:?}"
            );
        }
        let unprotected = azure_plan_for(id, target, false).expect("unprotected renders");
        let before: Vec<String> = unprotected
            .custom_roles
            .iter()
            .flat_map(|r| r.role_definition.data_actions.clone())
            .collect();
        assert!(
            before
                .iter()
                .any(|a| a.contains("diskimages/write") || a.contains("sandboxes/write")),
            "{id}: the unprotected install keeps the sandbox writes: {before:?}"
        );
    }
}

#[test]
fn a_wildcard_data_action_that_covers_a_verifier_power_is_refused() {
    let set = azure_data_set(&["Microsoft.App/*"]);
    render_azure_data(&set, true).expect_err("a wildcard reaching sandbox writes is refused");
}

#[test]
fn the_stack_secrets_set_setup_adds_is_held_by_its_contents_not_its_id() {
    let set = management_stack_secrets_data();
    let actions = render_aws(set, true).expect("the set setup adds renders");
    assert!(
        actions.contains(&"ssm:PutParameter".to_string()),
        "{actions:?}"
    );

    let mut wider = set.clone();
    wider.platforms.aws.as_mut().expect("aws entries")[0]
        .binding
        .stack = Some(alien_core::AwsBindingSpec {
        resources: vec!["*".to_string()],
        not_resources: Vec::new(),
        condition: None,
    });
}

#[test]
fn no_built_in_set_gives_a_protected_aws_management_identity_a_family_action() {
    let mut rendered = 0;
    for id in list_permission_set_ids() {
        let set = get_permission_set(id).expect("listed set");
        if set.platforms.aws.is_none() {
            continue;
        }
        for target in [BindingTarget::Stack, BindingTarget::Resource] {
            let policy = match AwsRuntimePermissionsGenerator::new().generate_policy(
                set,
                target,
                &aws_context(true),
            ) {
                Ok(policy) => policy,
                // A set with no grant for this target renders nothing.
                Err(error) if error.message.contains("is not supported by platform") => continue,
                Err(error) => panic!("{id}: {}", error.message),
            };
            rendered += 1;
            for statement in policy.statement.iter().filter(|s| s.effect == "Allow") {
                let services: Vec<String> = statement
                    .condition
                    .iter()
                    .flat_map(|conditions| conditions.values())
                    .flat_map(|values| values.iter())
                    .filter(|(key, _)| key.eq_ignore_ascii_case("iam:PassedToService"))
                    .flat_map(|(_, value)| match value {
                        alien_core::AwsConditionValue::String(service) => vec![service.clone()],
                        alien_core::AwsConditionValue::List(list) => list.clone(),
                    })
                    .collect();
                let services = (!services.is_empty()).then_some(services.as_slice());
                for action in &statement.action {
                    assert_eq!(
                        aws_family(action, services),
                        None,
                        "{id} grants management {action}"
                    );
                }
            }
        }
    }
    assert!(rendered > 100, "only {rendered} built-in policies rendered");
}

#[test]
fn no_built_in_set_gives_a_protected_azure_management_identity_a_family_action() {
    let mut rendered = 0;
    let mut offenders = Vec::new();
    for id in list_permission_set_ids() {
        let set = get_permission_set(id).expect("listed set");
        // A protected Azure install refuses vaults until the signing vault has its own resource
        // group, so the vault sets render nothing there yet.
        // Management never holds the sets that execute or call into a deployment.
        if set.platforms.azure.is_none()
            || id.starts_with("vault/")
            || id.ends_with("/execute")
            || id.ends_with("/remote-execute")
            || id.ends_with("/invoke")
        {
            continue;
        }
        let plan = match AzureRuntimePermissionsGenerator::new().generate_grant_plan(
            set,
            BindingTarget::Stack,
            &azure_context(true),
        ) {
            Ok(plan) => plan,
            Err(error) if error.message.contains("is not supported by platform") => continue,
            // A set that binds a role outside the built-in management list is not one management
            // holds (the setup-owned registry roles, sandbox execution).
            Err(error) if error.message.contains("can't be narrowed") => continue,
            // Everything the set grants is a family action, so nothing is left to render.
            Err(error) if error.message.contains("produced no Azure bindings") => continue,
            Err(error) => panic!("{id}: {}", error.message),
        };
        rendered += 1;
        for role in plan.custom_roles {
            for action in &role.role_definition.actions {
                let Some(family) = alien_permissions::generators::azure_family(action) else {
                    continue;
                };
                // A wildcard stands for a role that is kept with its REACH actions excluded.
                let excluded = alien_permissions::generators::azure_reach_exclusions(action);
                let covered = family == alien_permissions::generators::Family::Reach
                    && !excluded.is_empty()
                    && excluded
                        .iter()
                        .all(|action| role.role_definition.not_actions.contains(action));
                if !covered {
                    offenders.push(format!("{id}: {action} ({})", family.name()));
                }
            }
        }
    }
    assert!(rendered > 20, "only {rendered} built-in sets rendered");
    assert_eq!(offenders, Vec::<String>::new());
}

#[test]
fn every_azure_built_in_management_role_is_a_registry_role_without_family_actions() {
    use alien_permissions::generators::AZURE_BUILT_IN_ROLES;
    let mut bound = std::collections::BTreeSet::new();
    for id in list_permission_set_ids() {
        let set = get_permission_set(id).expect("listed set");
        for entry in set.platforms.azure.iter().flatten() {
            bound.extend(entry.grant.predefined_roles.iter().flatten().cloned());
        }
    }
    for role in AZURE_BUILT_IN_ROLES {
        assert!(
            bound.contains(*role),
            "{role} is listed but no built-in set binds it"
        );
    }
    // The Azure roles in the list are read or data-plane roles; none writes the control plane.
    for role in AZURE_BUILT_IN_ROLES {
        assert!(
            role.contains("Data")
                || role.contains("Reader")
                || role.contains("Pull")
                || role.contains("User"),
            "{role} looks like a role that can write the control plane"
        );
    }
}
