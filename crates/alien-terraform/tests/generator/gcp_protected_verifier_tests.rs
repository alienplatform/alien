//! What a protected GCP module lets the verifier do with the install's signing settings and
//! ledger, resolved from the roles it grants to the permissions GCP puts in them.

use alien_core::{Stack, StackSettings, StackSigning};
use alien_terraform::TerraformTarget;

use super::helpers::render;

/// Parameter Manager permissions of the predefined roles, as GCP lists them
/// (`gcloud iam roles describe`).
fn role_permissions(role: &str) -> &'static [&'static str] {
    match role {
        "roles/parametermanager.parameterAccessor" => {
            &["parametermanager.parameterVersions.render"]
        }
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
            },
            "verifier": {
                "config": { "id": "verifier", "type": "verifier" },
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
    let text = module_text(&module);
    // The project-level roles granted to the verifier's account.
    let mut permissions = Vec::new();
    for block in text.split("resource \"google_project_iam_member\"").skip(1) {
        let body = &block[..block.find("\n}").unwrap_or(block.len())];
        if !body.contains("google_service_account.signing_verifier.email") {
            continue;
        }
        let role = body
            .lines()
            .find_map(|line| line.trim().strip_prefix("role"))
            .map(|rest| {
                rest.trim_start()
                    .trim_start_matches('=')
                    .trim()
                    .trim_matches('"')
            })
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

/// Every permission a protected GCP module puts in a custom role is one GCP accepts there:
/// one unknown name and `terraform apply` fails creating the role, so no protected install
/// gets set up. Asks GCP which permissions custom roles support on the project.
/// Needs `GCP_ACCESS_TOKEN` and `GCP_PROJECT_ID`.
#[tokio::test]
#[ignore = "needs GCP credentials"]
async fn every_custom_role_permission_exists_on_gcp() {
    let token = std::env::var("GCP_ACCESS_TOKEN").expect("GCP_ACCESS_TOKEN");
    let project = std::env::var("GCP_PROJECT_ID").expect("GCP_PROJECT_ID");
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
    let permission = regex::Regex::new(r#""([a-z]+\.[A-Za-z]+\.[A-Za-z]+)""#).unwrap();
    let mut used = std::collections::BTreeSet::new();
    for (_, file) in module.iter() {
        for block in file
            .split("resource \"google_project_iam_custom_role\"")
            .skip(1)
        {
            let body = &block[..block.find("\n}").unwrap_or(block.len())];
            used.extend(permission.captures_iter(body).map(|c| c[1].to_string()));
        }
    }
    assert!(!used.is_empty(), "the module has no custom roles");

    let client = reqwest::Client::new();
    let mut supported = std::collections::BTreeSet::new();
    let mut page_token: Option<String> = None;
    loop {
        let mut body = serde_json::json!({
            "fullResourceName": format!("//cloudresourcemanager.googleapis.com/projects/{project}"),
            "pageSize": 1000,
        });
        if let Some(page) = &page_token {
            body["pageToken"] = serde_json::Value::from(page.clone());
        }
        let page: serde_json::Value = client
            .post("https://iam.googleapis.com/v1/permissions:queryTestablePermissions")
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .await
            .unwrap();
        for entry in page["permissions"].as_array().into_iter().flatten() {
            if entry["customRolesSupportLevel"] != "NOT_SUPPORTED" {
                supported.insert(entry["name"].as_str().unwrap_or_default().to_string());
            }
        }
        page_token = page["nextPageToken"].as_str().map(str::to_string);
        if page_token.is_none() {
            break;
        }
    }
    let unknown: Vec<_> = used.difference(&supported).collect();
    assert!(
        unknown.is_empty(),
        "GCP refuses these in a custom role: {unknown:?}"
    );
}

/// Every Terraform file of `module`, one after the other: the verifier's grants live in its own
/// file and the signing settings' in `signing.tf`.
fn module_text(module: &alien_terraform::ModuleFiles) -> String {
    module
        .iter()
        .filter(|(path, _)| path.ends_with(".tf"))
        .map(|(_, contents)| format!("{contents}\n"))
        .collect()
}

/// Custom role label to its permissions, and the custom roles bound to the verifier with and
/// without a condition, from the rendered module.
fn verifier_custom_roles(text: &str) -> (Vec<(String, Vec<String>)>, Vec<(String, bool)>) {
    let mut roles = Vec::new();
    for block in text
        .split("resource \"google_project_iam_custom_role\" \"")
        .skip(1)
    {
        let label = block[..block.find('"').unwrap()].to_string();
        let body = &block[..block.find("\n}").unwrap_or(block.len())];
        let permissions = body
            .split("permissions")
            .nth(1)
            .map(|list| {
                list[..list.find(']').unwrap()]
                    .split('"')
                    .skip(1)
                    .step_by(2)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        roles.push((label, permissions));
    }
    let mut bindings = Vec::new();
    for block in text.split("resource \"google_project_iam_member\"").skip(1) {
        let body = &block[..block.find("\n}").unwrap_or(block.len())];
        if !body.contains("google_service_account.signing_verifier.email") {
            continue;
        }
        if let Some(rest) = body.split("google_project_iam_custom_role.").nth(1) {
            let label = rest[..rest.find('.').unwrap()].to_string();
            bindings.push((label, body.contains("condition {")));
        }
    }
    (roles, bindings)
}

/// Cloud Run doesn't evaluate `resource.name` in IAM conditions: on real GCP a binding of the
/// verifier's role conditioned on the service's name denied even a get of a service it names,
/// and a create is checked against the parent location. So the verifier's Cloud Run permissions
/// must come from a binding without a condition.
#[test]
fn the_verifier_holds_its_cloud_run_permissions_without_a_name_condition() {
    let stack: Stack = serde_json::from_value(serde_json::json!({
        "id": "app",
        "resources": {
            "verifier": {
                "config": { "id": "verifier", "type": "verifier" },
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
    let (roles, bindings) = verifier_custom_roles(&module_text(&module));
    let unconditioned: Vec<&String> = roles
        .iter()
        .filter(|(label, _)| {
            bindings
                .iter()
                .any(|(bound, conditioned)| bound == label && !conditioned)
        })
        .flat_map(|(_, permissions)| permissions)
        .collect();
    for needed in [
        "run.services.create",
        "run.services.get",
        "run.services.update",
        "run.services.getIamPolicy",
        "run.services.setIamPolicy",
    ] {
        assert!(
            unconditioned.iter().any(|held| *held == needed),
            "{needed} is not granted without a condition: roles {roles:?}, bindings {bindings:?}"
        );
    }
}
