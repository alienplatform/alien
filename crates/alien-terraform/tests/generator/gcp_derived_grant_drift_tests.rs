//! A Frozen bucket's IAM grants are rendered by setup; the verifier derives the same grants for
//! its own writes (`gcp_resource_scoped_bindings`). The two must not drift: a grant setup emits
//! that the verifier would refuse, or the reverse, breaks protected installs or widens them.
//!
//! The stack goes through the real preparation (preflights add the profile service accounts).

use super::helpers::{normalized, render};
use alien_core::{
    DeploymentConfig, EnvironmentVariablesSnapshot, ExternalBindings, GcpManagementConfig,
    ManagementConfig, PermissionProfile, Platform, ResourceLifecycle, Stack, StackSettings,
    StackState, Storage,
};
use alien_permissions::gcp_resource_grants::{gcp_resource_scoped_bindings, GcpInstall};
use alien_preflights::runner::PreflightRunner;
use alien_terraform::TerraformTarget;
use std::collections::BTreeSet;

fn stack() -> Stack {
    Stack::new("acme".to_string())
        .permission(
            "execution",
            PermissionProfile::new().resource("assets", ["storage/data-write"]),
        )
        .permission("reader", PermissionProfile::new().global(["storage/data-read"]))
        .permission("unrelated", PermissionProfile::new().global(["queue/data-read"]))
        .add(
            Storage::new("assets".to_string()).build(),
            ResourceLifecycle::Frozen,
        )
        .build()
}

async fn prepare(stack: Stack) -> Stack {
    let runner = PreflightRunner::new();
    runner
        .run_template_preflights(&stack, Platform::Gcp)
        .await
        .expect("template preflights pass");
    let config = DeploymentConfig::builder()
        .deployment_name(stack.id().to_string())
        .stack_settings(StackSettings::default())
        .management_config(ManagementConfig::Gcp(GcpManagementConfig {
            federation_trust: None,
            service_account_email: "manager@vendor-project.iam.gserviceaccount.com".to_string(),
        }))
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: Vec::new(),
            hash: "empty".to_string(),
            created_at: "1970-01-01T00:00:00Z".to_string(),
        })
        .allow_frozen_changes(false)
        .external_bindings(ExternalBindings::default())
        .build();
    runner
        .apply_mutations(stack, &StackState::new(Platform::Gcp), &config)
        .await
        .expect("template mutations apply")
}

/// What a rendered `google_storage_bucket_iam_member` block grants: `(profile, role, condition)`.
/// A custom role is named by its permission-set part, which both sides share.
fn emitted_grants(main: &str) -> BTreeSet<(String, String, String)> {
    let mut grants = BTreeSet::new();
    for block in main
        .split("resource \"")
        .filter(|block| block.starts_with("google_storage_bucket_iam_member\""))
    {
        let block = block.split(" data \"").next().unwrap_or(block);
        let member = block
            .split("google_service_account.")
            .nth(1)
            .and_then(|rest| rest.split(".email").next())
            .expect("a service account member");
        let Some(profile) = member.strip_suffix("_sa") else {
            // The management identity's grants are setup's alone; the verifier never derives them.
            assert_eq!(member, "management", "unexpected member {member}");
            continue;
        };
        let role = if block.contains("role = var.gcp_manage_custom_roles") {
            let name = block
                .split("role_%s_")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .expect("custom role name");
            format!("custom:{name}")
        } else {
            block
                .split("role = \"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .expect("a literal role")
                .to_string()
        };
        let condition = block
            .split("expression = ")
            .nth(1)
            .map(|rest| rest.split(" }").next().unwrap_or(rest).to_string())
            .unwrap_or_default();
        grants.insert((profile.to_string(), role, condition));
    }
    grants
}

fn derived_grants(stack: &Stack) -> BTreeSet<(String, String, String)> {
    let install = GcpInstall {
        prefix: "acme",
        project_id: "p",
        project_number: None,
        region: "us-central1",
    };
    gcp_resource_scoped_bindings(stack, &install, "assets", "storage", "acme-assets")
        .expect("derived")
        .into_iter()
        .map(|binding| {
            let member = &binding.members[0];
            let profile = ["execution", "reader", "unrelated"]
                .into_iter()
                .find(|profile| member.contains(&format!("-{profile}-sa-")))
                .unwrap_or_else(|| panic!("unknown member {member}"));
            let role = match binding.role.strip_prefix("projects/p/roles/role_acme_") {
                Some(name) => format!("custom:{name}"),
                None => binding.role.clone(),
            };
            let condition = binding
                .condition
                .map(|condition| condition.expression)
                .unwrap_or_default();
            (profile.to_string(), role, condition)
        })
        .collect()
}

#[tokio::test]
async fn setup_renders_the_bucket_grants_the_verifier_derives() {
    let stack = prepare(stack()).await;
    let module = render(&stack, TerraformTarget::Gcp, StackSettings::default());
    let emitted = emitted_grants(&normalized(&module));
    let derived = derived_grants(&stack);
    assert!(
        derived.len() >= 3,
        "the fixture must exercise predefined and custom roles: {derived:?}"
    );
    assert_eq!(
        emitted, derived,
        "setup's bucket grants and the verifier's derived grants drifted apart"
    );
}
