//! What a protected GCP module lets the verifier do with the install's signing settings and
//! ledger, resolved from the roles it grants to the permissions GCP puts in them.

use alien_core::{Stack, StackSettings, StackSigning};
use alien_terraform::TerraformTarget;

use super::helpers::render;

/// Parameter Manager permissions of the predefined roles, as GCP lists them
/// (`gcloud iam roles describe`).
fn role_permissions(role: &str) -> &'static [&'static str] {
    match role {
        "roles/parametermanager.parameterAccessor" => &["parametermanager.parameterVersions.render"],
        "roles/parametermanager.parameterViewer" => &[
            "parametermanager.parameterVersions.get",
            "parametermanager.parameterVersions.list",
            "parametermanager.parameters.get",
            "parametermanager.parameters.list",
        ],
        "roles/artifactregistry.reader" => &[
            "artifactregistry.repositories.downloadArtifacts",
            "artifactregistry.repositories.get",
            "artifactregistry.repositories.list",
        ],
        "roles/parametermanager.parameterVersionAdder" => &[
            "parametermanager.parameterVersions.create",
            "parametermanager.parameters.get",
            "parametermanager.parameters.list",
        ],
        _ => &[],
    }
}

#[test]
fn the_verifier_may_read_its_settings_ledger_and_the_images_it_deploys() {
    let stack: Stack = serde_json::from_value(serde_json::json!({
        "id": "app",
        "resources": {
            "assets": {
                "config": {
                    "corsAllowedOrigins": [], "id": "assets", "lifecycleRules": [],
                    "publicRead": false, "type": "storage", "versioning": false
                },
                "lifecycle": "frozen", "dependencies": [], "remoteAccess": false
            }
        },
        "permissions": { "profiles": { "execution": {} }, "management": "auto" }
    }))
    .expect("stack");
    let settings = StackSettings {
        signing: Some(StackSigning {
            fingerprint: format!("SHA256:{}", "9f".repeat(32)),
            channel: "production".to_string(),
        }),
        ..StackSettings::default()
    };
    let module = render(&stack, TerraformTarget::Gcp, settings);
    let signing = module.get("signing.tf").expect("signing.tf");
    // The project-level roles granted to the verifier's account.
    let mut permissions = Vec::new();
    for block in signing.split("resource \"google_project_iam_member\"").skip(1) {
        let body = &block[..block.find("\n}").unwrap_or(block.len())];
        if !body.contains("google_service_account.signing_verifier.email") {
            continue;
        }
        let role = body
            .lines()
            .find_map(|line| line.trim().strip_prefix("role"))
            .map(|rest| rest.trim_start().trim_start_matches('=').trim().trim_matches('"'))
            .unwrap_or_default();
        permissions.extend(role_permissions(role));
    }
    // `parameter()` lists a parameter's versions and gets the newest with `view=FULL`, and
    // Cloud Run deploys an image only for a principal that may read it.
    for needed in [
        "parametermanager.parameterVersions.list",
        "parametermanager.parameterVersions.get",
        "artifactregistry.repositories.downloadArtifacts",
    ] {
        assert!(permissions.contains(&needed), "{needed}: {permissions:?}");
    }
}
