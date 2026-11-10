//! What a stack may add to a protected install's management identity. The limits on a protected
//! install bind the management identity itself, so a grant that lets it mint a role, assume
//! another role or run a command on a machine would let it act without them. The built-in sets are
//! reviewed together with the limits and render as before; a set the stack defines is refused when
//! one of its actions reaches such a grant. An unprotected install keeps every grant it had.

mod common;

use alien_core::PermissionSet;
use alien_permissions::{
    generators::{
        AwsCloudFormationPermissionsGenerator, AwsRuntimePermissionsGenerator,
        AzureRuntimePermissionsGenerator, GcpRuntimePermissionsGenerator,
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

#[test]
fn an_aws_stack_grant_that_reaches_iam_writes_or_role_assumption_is_refused() {
    for action in [
        "iam:*",
        "iam:CreateRole",
        "iam:AttachRolePolicy",
        "iam:PutRolePolicy",
        "iam:UpdateAssumeRolePolicy",
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
        "sts:Assume*",
        "sts:AssumeRoleWithWebIdentity",
        "STS:assumerole",
    ] {
        let set = aws_set(&["s3:GetObject", action]);
        let error = render_aws(&set, true)
            .expect_err(&format!("{action} must be refused on a protected install"));
        assert_eq!(
            error.code, "PROTECTED_MANAGEMENT_ACTION_REFUSED",
            "{action}"
        );
        assert!(
            error.message.contains(action),
            "{action}: {}",
            error.message
        );
        assert!(error.message.contains("orders/extra-management"));

        let error = render_aws_template(&set, true)
            .expect_err(&format!("{action} must be refused in the setup template"));
        assert_eq!(
            error.code, "PROTECTED_MANAGEMENT_ACTION_REFUSED",
            "{action}"
        );

        let unprotected = render_aws(&set, false).expect("an unprotected install is unchanged");
        assert!(
            unprotected.iter().any(|granted| granted == action),
            "{action}"
        );
        render_aws_template(&set, false).expect("an unprotected install is unchanged");
    }
}

#[test]
fn an_aws_stack_grant_of_reads_and_other_services_is_kept() {
    let set = aws_set(&[
        "s3:*",
        "iam:GetRole",
        "iam:List*",
        "iam:SimulatePrincipalPolicy",
        "sts:GetCallerIdentity",
        "ec2:Describe*",
    ]);
    let granted = render_aws(&set, true).expect("reads and other services render");
    for action in ["s3:*", "iam:GetRole", "iam:List*", "sts:GetCallerIdentity"] {
        assert!(granted.iter().any(|kept| kept == action), "{action}");
    }
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
    assert_eq!(error.code, "PROTECTED_MANAGEMENT_ACTION_REFUSED");

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
    assert!(rendered > 50, "only {rendered} built-in sets rendered");
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

#[test]
fn an_azure_stack_grant_that_reaches_a_verifier_action_is_refused() {
    for action in [
        "Microsoft.Compute/virtualMachineScaleSets/virtualMachines/runCommand/action",
        "Microsoft.Compute/virtualMachines/runCommand/action",
        "Microsoft.Compute/*/action",
        "Microsoft.Compute/virtualMachineScaleSets/*/runCommand/action",
        "Microsoft.Compute/*",
        "Microsoft.App/jobs/start/action",
        "Microsoft.App/containerApps/revisions/*/action",
        "Microsoft.App/*",
        "microsoft.managedidentity/userassignedidentities/assign/action",
        "*/action",
        "*",
    ] {
        let set = azure_set(&["Microsoft.Storage/storageAccounts/read", action]);
        let error = render_azure(&set, true)
            .expect_err(&format!("{action} must be refused on a protected install"));
        assert_eq!(
            error.code, "PROTECTED_MANAGEMENT_ACTION_REFUSED",
            "{action}"
        );
        assert!(
            error.message.contains(action),
            "{action}: {}",
            error.message
        );

        let unprotected = render_azure(&set, false).expect("an unprotected install is unchanged");
        assert!(
            unprotected.iter().any(|granted| granted == action),
            "{action}"
        );
    }
}

#[test]
fn an_azure_stack_grant_that_reaches_no_verifier_action_is_kept() {
    let set = azure_set(&[
        "Microsoft.Storage/storageAccounts/read",
        "Microsoft.Compute/virtualMachineScaleSets/read",
        "Microsoft.Network/virtualNetworks/write",
        "Microsoft.App/containerApps/revisions/read",
    ]);
    let granted = render_azure(&set, true).expect("these render");
    assert!(granted
        .iter()
        .any(|action| action == "Microsoft.Network/virtualNetworks/write"));
    assert!(granted
        .iter()
        .any(|action| action == "Microsoft.Storage/storageAccounts/read"));
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
fn a_gcp_predefined_role_outside_the_allowed_list_is_refused() {
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
        "roles/some.roleAddedToGcpLater",
    ] {
        let set = gcp_set(json!({ "predefinedRoles": [role] }));
        let error = render_gcp(&set, true)
            .expect_err(&format!("{role} must be refused on a protected install"));
        assert_eq!(error.code, "PROTECTED_MANAGEMENT_ROLE_REFUSED", "{role}");
        assert!(error.message.contains(role), "{role}: {}", error.message);
        render_gcp(&set, false).expect("an unprotected install is unchanged");
    }

    let set = gcp_set(json!({ "predefinedRoles": ["roles/storage.objectViewer"] }));
    let (roles, _) = render_gcp(&set, true).expect("an allowed role renders");
    assert_eq!(roles, ["roles/storage.objectViewer"]);
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
        "a new built-in role must be added to the allowed list or refused here on purpose"
    );
}

#[test]
fn gcp_os_login_and_os_config_writes_are_left_out_of_a_protected_role() {
    let set = gcp_set(json!({ "permissions": [
        "storage.objects.get",
        "compute.instances.osAdminLogin",
        "compute.instances.osLogin",
        "osconfig.patchJobs.exec",
        "osconfig.patchDeployments.create",
        "osconfig.guestPolicies.update",
        "osconfig.osPolicyAssignments.create",
        "oslogin.users.update",
        "oslogin.sshPublicKeys.create",
        "osconfig.patchJobs.get",
        "osconfig.guestPolicies.list",
        "oslogin.users.getLoginProfile",
    ] }));
    let (_, permissions) = render_gcp(&set, true).expect("renders");
    assert_eq!(
        permissions,
        [
            "osconfig.guestPolicies.list",
            "osconfig.patchJobs.get",
            "oslogin.users.getLoginProfile",
            "storage.objects.get",
        ]
    );
    let (_, unprotected) = render_gcp(&set, false).expect("renders");
    assert!(unprotected.contains(&"osconfig.patchJobs.exec".to_string()));
    assert!(unprotected.contains(&"compute.instances.osAdminLogin".to_string()));
}
