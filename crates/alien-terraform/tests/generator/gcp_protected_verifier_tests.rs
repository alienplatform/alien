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
    for block in signing
        .split("resource \"google_project_iam_member\"")
        .skip(1)
    {
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

/// Custom role label to its permissions, and the custom roles bound to the verifier with and
/// without a condition, from a rendered `signing.tf`.
fn verifier_custom_roles(signing: &str) -> (Vec<(String, Vec<String>)>, Vec<(String, bool)>) {
    let mut roles = Vec::new();
    for block in signing
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
    for block in signing
        .split("resource \"google_project_iam_member\"")
        .skip(1)
    {
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

/// A protected GCP stack with worker `api` running as profile `execution`.
fn stack_with_worker() -> Stack {
    let permissions: Stack = serde_json::from_value(serde_json::json!({
        "id": "app",
        "resources": {},
        "permissions": { "profiles": { "execution": {} }, "management": "auto" }
    }))
    .expect("stack");
    Stack::new("app".to_string())
        .permissions(permissions.permissions().clone())
        .add(
            alien_core::ServiceAccount::new("execution-sa".to_string()).build(),
            alien_core::ResourceLifecycle::Frozen,
        )
        .add(
            alien_core::RemoteStackManagement::new("management".to_string()).build(),
            alien_core::ResourceLifecycle::Frozen,
        )
        .add(
            alien_core::Worker::new("api".to_string())
                .code(alien_core::WorkerCode::Image {
                    image: format!("us-docker.pkg.dev/acme/app/api@sha256:{}", "a".repeat(64)),
                })
                .permissions("execution".to_string())
                .build(),
            alien_core::ResourceLifecycle::Live,
        )
        .build()
}

fn protected() -> StackSettings {
    StackSettings {
        signing: Some(StackSigning {
            fingerprint: format!("SHA256:{}", "9f".repeat(32)),
            channel: "production".to_string(),
        }),
        ..StackSettings::default()
    }
}

/// Setup creates each worker's Cloud Run service, closed, running as the worker's account; the
/// verifier's Cloud Run role holds no create, and is bound on that service alone, so it reaches
/// no other service in the project. Evaluated with Terraform against mocked providers.
#[test]
fn setup_creates_each_workers_service_and_the_verifier_reaches_only_those() {
    let module = render(&stack_with_worker(), TerraformTarget::Gcp, protected());
    let mut files = super::helpers::linter_files(&module);
    files.insert(
        "tests/workers.tftest.hcl".to_string(),
        r#"
mock_provider "google" {
  mock_resource "google_service_account" {
    defaults = {
      id = "projects/acme/serviceAccounts/account@acme.iam.gserviceaccount.com"
      name = "projects/acme/serviceAccounts/account@acme.iam.gserviceaccount.com"
      email = "account@acme.iam.gserviceaccount.com"
    }
    override_during = plan
  }
}
override_resource {
  target = google_service_account.execution_sa
  values = {
    email = "execution@acme.iam.gserviceaccount.com"
    name = "projects/acme/serviceAccounts/execution@acme.iam.gserviceaccount.com"
  }
  override_during = plan
}
override_resource {
  target = google_service_account.signing_verifier
  values = {
    email = "verifier@acme.iam.gserviceaccount.com"
    name = "projects/acme/serviceAccounts/verifier@acme.iam.gserviceaccount.com"
  }
  override_during = plan
}
override_resource {
  target = google_project_iam_custom_role.signing_verifier_cloud_run
  values = { name = "projects/acme/roles/acme_verifier_cloud_run" }
  override_during = plan
}
override_resource {
  target = google_cloud_run_v2_service.signing_worker_api
  values = { name = "acme-api" }
  override_during = plan
}
variables {
  name = "app"
  resource_prefix = "acme"
  gcp_project = "acme"
  gcp_region = "us-central1"
  deployment_id = "dep_test"
  managing_oidc_issuer = "https://issuer.example.com"
  managing_oidc_subject = "manager"
}
run "workers" {
  command = plan
  assert {
    condition = google_cloud_run_v2_service.signing_worker_api.ingress == "INGRESS_TRAFFIC_INTERNAL_ONLY"
    error_message = "Setup creates the service closed."
  }
  assert {
    condition = startswith(google_cloud_run_v2_service.signing_worker_api.template[0].containers[0].image, "us-docker.pkg.dev/cloudrun/container/hello@sha256:")
    error_message = "Until a signed release lands the service runs the pinned placeholder."
  }
  assert {
    condition = google_cloud_run_v2_service.signing_worker_api.template[0].service_account == "execution@acme.iam.gserviceaccount.com"
    error_message = "The service runs as the worker's account."
  }
  assert {
    condition = google_cloud_run_v2_service_iam_member.signing_worker_api_verifier.name == "acme-api" && google_cloud_run_v2_service_iam_member.signing_worker_api_verifier.role == "projects/acme/roles/acme_verifier_cloud_run" && google_cloud_run_v2_service_iam_member.signing_worker_api_verifier.member == "serviceAccount:verifier@acme.iam.gserviceaccount.com"
    error_message = "The verifier's Cloud Run role is bound on the worker's service."
  }
  assert {
    condition = toset(google_project_iam_custom_role.signing_verifier_cloud_run.permissions) == toset(["run.services.get", "run.services.update", "run.services.getIamPolicy", "run.services.setIamPolicy"])
    error_message = "The verifier may not create services."
  }
}
"#
        .to_string(),
    );
    super::helpers::test_utils::terraform_test(&files).assert_ok("protected GCP workers' services");

    // No project-level binding grants any Cloud Run permission: every project role the module
    // gives the verifier, custom or predefined, holds none.
    let signing = module.get("signing.tf").expect("signing.tf");
    for block in signing
        .split("resource \"google_project_iam_member\"")
        .skip(1)
    {
        let body = &block[..block.find("\n}").unwrap_or(block.len())];
        assert!(
            !body.contains("signing_verifier_cloud_run"),
            "the Cloud Run role is bound project-wide: {body}"
        );
    }
}
