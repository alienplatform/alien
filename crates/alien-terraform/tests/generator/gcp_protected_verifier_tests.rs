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
        "roles/secretmanager.secretAccessor" => &["secretmanager.versions.access"],
        "roles/secretmanager.secretVersionAdder" => &["secretmanager.versions.add"],
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
    // No project-wide Secret Manager grant: the ledger is the only secret it writes.
    assert!(
        !permissions
            .iter()
            .any(|permission| permission.starts_with("secretmanager.")),
        "{permissions:?}"
    );
    // The ledger is the secret the verifier reads (`signed-<prefix>-state`), and on it alone the
    // verifier reads the newest version and adds one.
    let ledger = text
        .split("resource \"google_secret_manager_secret\" \"signing_state\"")
        .nth(1)
        .expect("the ledger secret");
    let ledger = &ledger[..ledger.find("\n}").unwrap()];
    assert!(
        ledger.contains(&format!(
            "secret_id = \"{}\"",
            alien_signing::settings::names::gcp_ledger("${local.resource_prefix}")
        )),
        "{ledger}"
    );
    let mut on_ledger: Vec<&str> = Vec::new();
    for block in text
        .split("resource \"google_secret_manager_secret_iam_member\"")
        .skip(1)
    {
        let body = &block[..block.find("\n}").unwrap_or(block.len())];
        assert!(
            body.contains("google_secret_manager_secret.signing_state.secret_id"),
            "{body}"
        );
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
        on_ledger.extend(role_permissions(role));
    }
    on_ledger.sort();
    assert_eq!(
        on_ledger,
        [
            "secretmanager.versions.access",
            "secretmanager.versions.add"
        ]
    );

    // `verify` reads that ledger with the auditor's own credentials, which Viewer roles can't do:
    // the README says which role on which secret, with the line that grants it.
    let readme = module
        .iter()
        .find_map(|(path, contents)| (path == "README.md").then_some(contents))
        .expect("README.md");
    assert!(
        readme.contains(
            "gcloud secrets add-iam-policy-binding signed-<resource_prefix>-state --project=<project> --member=<principal> --role=roles/secretmanager.secretAccessor"
        ),
        "{readme}"
    );
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

/// The verifier sets bucket IAM policies and public access prevention, only on the install's own
/// buckets: Cloud Storage evaluates `resource.name` of a bucket, so the binding carries the name
/// condition, and no other verifier role holds a bucket permission.
#[test]
fn the_verifier_holds_its_bucket_permissions_only_on_the_installs_own_buckets() {
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
    let text = module_text(&render(&stack, TerraformTarget::Gcp, settings));
    let (roles, bindings) = verifier_custom_roles(&text);
    for permission in [
        "storage.buckets.getIamPolicy",
        "storage.buckets.setIamPolicy",
        "storage.buckets.update",
    ] {
        let holders: Vec<&String> = roles
            .iter()
            .filter(|(_, permissions)| permissions.iter().any(|held| held == permission))
            .map(|(label, _)| label)
            .collect();
        assert_eq!(holders, ["signing_verifier_buckets"], "{permission}");
    }
    assert!(
        bindings.contains(&("signing_verifier_buckets".to_string(), true)),
        "the bucket role is bound without a name condition: {bindings:?}"
    );
    assert!(
        text.contains(
            r#"resource.name.startsWith(\"projects/_/buckets/${local.resource_prefix}-\")"#
        ),
        "the condition names the install's bucket prefix"
    );
}

/// The verifier creates the install's topics and subscriptions and sets who may reach them, only
/// on resources named with the install's prefix, and no other verifier role holds those
/// permissions.
#[test]
fn the_verifier_holds_its_pubsub_permissions_only_on_the_installs_own_queues() {
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
    let text = module_text(&render(&stack, TerraformTarget::Gcp, settings));
    let (roles, bindings) = verifier_custom_roles(&text);
    for (permission, role) in [
        ("pubsub.topics.create", "signing_verifier_queue_create"),
        (
            "pubsub.subscriptions.create",
            "signing_verifier_queue_create",
        ),
        ("pubsub.topics.setIamPolicy", "signing_verifier_queues"),
        (
            "pubsub.subscriptions.setIamPolicy",
            "signing_verifier_queues",
        ),
    ] {
        let holders: Vec<&String> = roles
            .iter()
            .filter(|(_, permissions)| permissions.iter().any(|held| held == permission))
            .map(|(label, _)| label)
            .collect();
        assert_eq!(holders, [role], "{permission}");
    }
    assert!(
        bindings.contains(&("signing_verifier_queues".to_string(), true)),
        "the IAM role is bound under a name condition: {bindings:?}"
    );
    assert!(
        bindings.contains(&("signing_verifier_queue_create".to_string(), false)),
        "creates are bound without a condition, which Pub/Sub does not evaluate for a new name: {bindings:?}"
    );
    assert!(
        text.contains(r#"resource.name.startsWith(\"projects/${var.gcp_project}/topics/${local.resource_prefix}-\")"#),
        "the condition names the install's topic prefix"
    );
}

#[test]
fn the_verifier_alone_holds_the_permissions_that_create_sandbox_engines_and_templates() {
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
    let text = module_text(&render(&stack, TerraformTarget::Gcp, settings));
    let (roles, bindings) = verifier_custom_roles(&text);
    for permission in [
        "aiplatform.reasoningEngines.create",
        "aiplatform.sandboxEnvironmentTemplates.create",
    ] {
        let holders: Vec<&String> = roles
            .iter()
            .filter(|(_, permissions)| permissions.iter().any(|held| held == permission))
            .map(|(label, _)| label)
            .collect();
        assert_eq!(holders, ["signing_verifier_sandboxes"], "{permission}");
    }
    assert!(
        bindings
            .iter()
            .any(|(role, _)| role == "signing_verifier_sandboxes"),
        "the sandbox role is bound: {bindings:?}"
    );
}

#[test]
fn the_verifier_alone_holds_the_permissions_that_create_schedule_jobs() {
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
    let text = module_text(&render(&stack, TerraformTarget::Gcp, settings));
    let (roles, bindings) = verifier_custom_roles(&text);
    for permission in ["cloudscheduler.jobs.create"] {
        let holders: Vec<&String> = roles
            .iter()
            .filter(|(_, permissions)| permissions.iter().any(|held| held == permission))
            .map(|(label, _)| label)
            .collect();
        assert_eq!(holders, ["signing_verifier_schedules"], "{permission}");
    }
    assert!(
        bindings
            .iter()
            .any(|(role, _)| role == "signing_verifier_schedules"),
        "the schedule role is bound: {bindings:?}"
    );
}

#[test]
fn the_verifier_acts_as_exactly_the_runtime_service_accounts() {
    let stack: Stack = serde_json::from_value(serde_json::json!({
        "id": "app",
        "resources": {
            "verifier": {
                "config": { "id": "verifier", "type": "verifier" },
                "lifecycle": "frozen", "dependencies": [], "remoteAccess": false
            },
            "execution-sa": {
                "config": { "id": "execution-sa", "type": "service-account", "stackPermissionSets": [] },
                "lifecycle": "frozen", "dependencies": [], "remoteAccess": false
            },
            "reader-sa": {
                "config": { "id": "reader-sa", "type": "service-account", "stackPermissionSets": [] },
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
    let text = module_text(&render(&stack, TerraformTarget::Gcp, settings));
    let mut grants: Vec<String> = text
        .split("resource \"google_service_account_iam_member\" \"")
        .skip(1)
        .map(|block| block.split('"').next().unwrap_or_default().to_string())
        .filter(|label| label.starts_with("signing_verifier_acts_as_"))
        .collect();
    grants.sort();
    assert_eq!(
        grants,
        [
            "signing_verifier_acts_as_execution_sa",
            "signing_verifier_acts_as_reader_sa"
        ],
        "one actAs grant per runtime service account and none for management or the verifier"
    );
    // Each grant is the user role (it carries iam.serviceAccounts.actAs) for the verifier's own member.
    assert_eq!(
        text.matches("roles/iam.serviceAccountUser").count(),
        2,
        "no other account is granted the user role"
    );
}

/// Management dispatches commands to workers through topics the worker controller creates. On a
/// protected install setup grants it publishing once, by name, so a worker a later release adds
/// needs no setup update; an install whose management profile has no dispatch set gets nothing.
#[test]
fn management_publishes_to_commands_topics_by_name_only_when_it_dispatches_commands() {
    let stack_with = |management: serde_json::Value| -> Stack {
        serde_json::from_value(serde_json::json!({
            "id": "app",
            "resources": {
                "verifier": {
                    "config": { "id": "verifier", "type": "verifier" },
                    "lifecycle": "frozen", "dependencies": [], "remoteAccess": false
                },
                "management": {
                    "config": { "id": "management", "type": "remote-stack-management" },
                    "lifecycle": "frozen", "dependencies": [], "remoteAccess": false
                }
            },
            "permissions": { "profiles": { "execution": {} }, "management": management }
        }))
        .expect("stack")
    };
    let settings = |signed: bool| StackSettings {
        signing: signed.then(|| StackSigning {
            fingerprint: format!("SHA256:{}", "9f".repeat(32)),
            channel: "production".to_string(),
        }),
        ..StackSettings::default()
    };
    let dispatching = serde_json::json!({ "extend": { "*": ["worker/dispatch-command"] } });

    let text = module_text(&render(&stack_with(dispatching.clone()), TerraformTarget::Gcp, settings(true)));
    let block = text
        .split("resource \"google_project_iam_member\"")
        .skip(1)
        .find(|block| block.contains("commands_topic_publisher"))
        .expect("a publisher grant for the commands topics");
    let block = &block[..block.find("\n}").unwrap_or(block.len())];
    assert!(block.contains("roles/pubsub.publisher"), "{block}");
    assert!(
        block.contains("google_service_account.") && !block.contains("signing_verifier"),
        "bound to management, not the verifier: {block}"
    );
    assert!(
        block.contains(r#"resource.name.startsWith(\"projects/${var.gcp_project}/topics/${local.resource_prefix}-\")"#)
            && block.contains(r#"resource.name.endsWith(\"-rq\")"#),
        "limited by name to this install's commands topics: {block}"
    );

    let unsigned = module_text(&render(&stack_with(dispatching), TerraformTarget::Gcp, settings(false)));
    assert!(!unsigned.contains("commands_topic_publisher"), "unsigned installs are unchanged");
    let without = module_text(&render(&stack_with(serde_json::json!("auto")), TerraformTarget::Gcp, settings(true)));
    assert!(!without.contains("commands_topic_publisher"), "no dispatch set, no grant");
}
