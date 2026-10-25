//! Opt-in Cloud Storage qualification against an isolated project.
//!
//! `ALIEN_STORAGE_LIVE_FIXTURE` names a JSON file containing projectId, projectNumber,
//! region, prefix, serviceAccount, observerMember, setupTokenFile, runtimeTokenFile and evidenceDir.
//! Run `export_grants` first and install its exact grant plan on a fresh test identity.
//! Setup credentials seed data and observe results; controller calls use only runtime
//! credentials. The caller must erase the exact bucket and identity even on test failure.
//! See `gcp_storage_live.md` for setup, execution and runnable cleanup commands.

#![cfg(feature = "gcp")]

use std::{fs, path::PathBuf, sync::Arc, time::Duration};

use alien_core::{
    ClientConfig, GcpClientConfig, GcpCredentials, Platform, ResourceStatus, Storage,
};
use alien_infra::{
    controller_test::SingleControllerExecutor, DefaultPlatformServiceProvider, GcpStorageController,
};
use alien_permissions::{
    generators::GcpRuntimePermissionsGenerator, get_permission_set, BindingTarget,
    PermissionContext,
};
use reqwest::{Client, Method, Response};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Fixture {
    project_id: String,
    project_number: String,
    region: String,
    prefix: String,
    service_account: String,
    observer_member: String,
    setup_token_file: PathBuf,
    runtime_token_file: PathBuf,
    evidence_dir: PathBuf,
}

fn fixture() -> Fixture {
    let path =
        std::env::var("ALIEN_STORAGE_LIVE_FIXTURE").expect("explicit isolated fixture required");
    let fixture: Fixture = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert!(fixture.prefix.starts_with("test-"));
    assert!(fixture
        .service_account
        .ends_with(&format!("@{}.iam.gserviceaccount.com", fixture.project_id)));
    fixture
}

#[test]
#[ignore = "exports grants for an explicitly configured isolated cloud test"]
fn export_grants() {
    let f = fixture();
    let context = PermissionContext::new()
        .with_stack_prefix(&f.prefix)
        .with_stack_name("storage-live-test")
        .with_resource_id("assets")
        .with_resource_name(format!("{}-assets", f.prefix))
        .with_project_name(&f.project_id)
        .with_project_number(&f.project_number)
        .with_region(&f.region)
        .with_service_account_name(f.service_account.split('@').next().unwrap());
    let plan = GcpRuntimePermissionsGenerator::new()
        .generate_grant_plan(
            get_permission_set("storage/provision").unwrap(),
            BindingTarget::Stack,
            &context,
        )
        .unwrap();
    fs::write(
        f.evidence_dir.join("grants.json"),
        serde_json::to_vec_pretty(&plan).unwrap(),
    )
    .unwrap();
}

async fn request(client: &Client, method: Method, url: &str, body: Option<Value>) -> Response {
    let mut request = client.request(method, url);
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await.unwrap();
    assert!(
        response.status().is_success(),
        "fixture request status {}",
        response.status()
    );
    response
}

async fn anonymous_read(client: &Client, url: &str, public: bool) {
    // IAM writes propagate asynchronously. Unique URLs and no-store objects avoid cached
    // public responses being mistaken for access after revocation.
    for attempt in 0..85 {
        let response = client
            .get(format!("{url}?probe={}", uuid::Uuid::new_v4()))
            .header("Cache-Control", "no-cache")
            .send()
            .await
            .unwrap();
        let status = response.status().as_u16();
        if public && status == 200 {
            assert_eq!(response.text().await.unwrap(), "storage-live-proof");
            return;
        }
        if !public && matches!(status, 401 | 403) {
            return;
        }
        assert!(
            matches!(status, 200 | 401 | 403),
            "unexpected anonymous status {status}"
        );
        assert!(attempt < 84, "IAM access did not converge");
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

#[tokio::test]
#[ignore = "creates one bucket using an explicitly configured isolated cloud identity"]
async fn public_private() {
    let f = fixture();
    let mut setup_headers = reqwest::header::HeaderMap::new();
    setup_headers.insert(
        reqwest::header::AUTHORIZATION,
        format!(
            "Bearer {}",
            fs::read_to_string(&f.setup_token_file).unwrap().trim()
        )
        .parse()
        .unwrap(),
    );
    let setup = Client::builder()
        .default_headers(setup_headers)
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    let anonymous = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap();
    let bucket = format!("{}-assets", f.prefix);
    let base = format!("https://storage.googleapis.com/storage/v1/b/{bucket}");
    assert_eq!(
        setup.get(&base).send().await.unwrap().status().as_u16(),
        404,
        "test must create a fresh bucket"
    );
    let config = GcpClientConfig {
        project_id: f.project_id.clone(),
        project_number: Some(f.project_number.clone()),
        region: f.region.clone(),
        service_overrides: None,
        credentials: GcpCredentials::AccessToken {
            token: fs::read_to_string(&f.runtime_token_file)
                .unwrap()
                .trim()
                .into(),
        },
    };
    let private = Storage::new("assets".into()).versioning(true).build();
    let mut executor = SingleControllerExecutor::builder()
        .resource(private.clone())
        .controller(GcpStorageController::default())
        .platform(Platform::Gcp)
        .resource_prefix(&f.prefix)
        .client_config(ClientConfig::Gcp(Box::new(config)))
        .service_provider(Arc::new(DefaultPlatformServiceProvider::default()))
        .real_delays()
        .build()
        .await
        .unwrap();
    executor.run_until_terminal().await.unwrap();
    assert_eq!(executor.status(), ResourceStatus::Running);
    let initial: Value = request(&setup, Method::GET, &base, None)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        initial["iamConfiguration"]["uniformBucketLevelAccess"]["enabled"],
        true
    );
    assert_eq!(
        initial["iamConfiguration"]["publicAccessPrevention"],
        "enforced"
    );
    // This disposable fixture must not leave soft-deleted test data after cleanup.
    // This setup-only change is not part of the controller qualification.
    let cleanup_config: Value = request(
        &setup,
        Method::PATCH,
        &base,
        Some(json!({"softDeletePolicy":{"retentionDurationSeconds":"0"}})),
    )
    .await
    .json()
    .await
    .unwrap();
    assert_eq!(
        cleanup_config["softDeletePolicy"]["retentionDurationSeconds"],
        "0"
    );
    tokio::time::sleep(Duration::from_secs(30)).await;
    let object = format!("proof-{}", uuid::Uuid::new_v4());
    let uploaded = setup
        .post(format!(
            "https://storage.googleapis.com/upload/storage/v1/b/{bucket}/o"
        ))
        .query(&[("uploadType", "media"), ("name", &object)])
        .header("Content-Type", "text/plain")
        .body("storage-live-proof")
        .send()
        .await
        .unwrap();
    assert!(uploaded.status().is_success());
    request(
        &setup,
        Method::PATCH,
        &format!("{base}/o/{object}"),
        Some(json!({"cacheControl":"no-store"})),
    )
    .await;
    let url = format!("https://storage.googleapis.com/{bucket}/{object}");
    anonymous_read(&anonymous, &url, false).await;
    let runtime_read = anonymous
        .get(&url)
        .bearer_auth(fs::read_to_string(&f.runtime_token_file).unwrap().trim())
        .send()
        .await
        .unwrap();
    assert_eq!(
        runtime_read.status().as_u16(),
        403,
        "provision identity must not read private data"
    );
    let iam = format!("{base}/iam?optionsRequestedPolicyVersion=3");
    let mut policy: Value = request(&setup, Method::GET, &iam, None)
        .await
        .json()
        .await
        .unwrap();
    let foreign = json!({"role":"roles/storage.objectViewer", "members":[format!("serviceAccount:{}", f.service_account)],
        "condition":{"title":"preserve-unrelated", "expression":"resource.name.endsWith('/unrelated')"}});
    let mixed = json!({"role":"roles/storage.objectViewer", "members":[f.observer_member]});
    policy["version"] = json!(3);
    policy["bindings"]
        .as_array_mut()
        .unwrap()
        .push(foreign.clone());
    policy["bindings"]
        .as_array_mut()
        .unwrap()
        .push(mixed.clone());
    request(&setup, Method::PUT, &iam, Some(policy)).await;
    let mut public = private.clone();
    public.public_read = true;
    public.cors_allowed_origins = vec!["https://app.example.com".into()];
    executor.update(public).unwrap();
    executor.run_until_terminal().await.unwrap();
    anonymous_read(&anonymous, &url, true).await;
    for method in ["GET", "HEAD"] {
        let preflight = anonymous
            .request(Method::OPTIONS, &url)
            .header("Origin", "https://app.example.com")
            .header("Access-Control-Request-Method", method)
            .header("Access-Control-Request-Headers", "range")
            .send()
            .await
            .unwrap();
        assert_eq!(preflight.status().as_u16(), 200);
        assert_eq!(
            preflight.headers()["access-control-allow-origin"],
            "https://app.example.com"
        );
        let mut req = anonymous
            .request(method.parse().unwrap(), &url)
            .header("Origin", "https://app.example.com");
        if method == "GET" {
            req = req.header("Range", "bytes=0-6");
        }
        let response = req.send().await.unwrap();
        assert_eq!(
            response.status().as_u16(),
            if method == "GET" { 206 } else { 200 }
        );
        assert_eq!(
            response.headers()["access-control-allow-origin"],
            "https://app.example.com"
        );
        let exposed = response.headers()["access-control-expose-headers"]
            .to_str()
            .unwrap()
            .to_ascii_lowercase();
        for header in ["etag", "content-range", "accept-ranges"] {
            assert!(exposed.split(',').any(|value| value.trim() == header));
        }
    }
    let denied = anonymous
        .request(Method::OPTIONS, &url)
        .header("Origin", "https://untrusted.example.com")
        .header("Access-Control-Request-Method", "GET")
        .send()
        .await
        .unwrap();
    assert!(!denied.headers().contains_key("access-control-allow-origin"));
    executor.update(private.clone()).unwrap();
    executor.run_until_terminal().await.unwrap();
    anonymous_read(&anonymous, &url, false).await;
    let final_bucket: Value = request(&setup, Method::GET, &base, None)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        final_bucket["iamConfiguration"]["uniformBucketLevelAccess"]["enabled"],
        true
    );
    assert_eq!(
        final_bucket["iamConfiguration"]["publicAccessPrevention"],
        "enforced"
    );
    assert!(final_bucket
        .get("cors")
        .is_none_or(|v| v.as_array().unwrap().is_empty()));
    let final_policy: Value = request(&setup, Method::GET, &iam, None)
        .await
        .json()
        .await
        .unwrap();
    let bindings = final_policy["bindings"].as_array().unwrap();
    assert!(bindings.contains(&foreign));
    assert!(
        bindings.contains(&mixed),
        "revocation must keep the other member of a mixed public binding"
    );
    assert!(!bindings.iter().any(|b| b["members"]
        .as_array()
        .unwrap()
        .contains(&json!("allUsers"))));
    executor.update(private).unwrap();
    executor.run_until_terminal().await.unwrap();
    let unchanged_policy: Value = request(&setup, Method::GET, &iam, None)
        .await
        .json()
        .await
        .unwrap();
    assert_eq!(
        unchanged_policy, final_policy,
        "no-op must preserve the policy and etag"
    );
    fs::write(
        f.evidence_dir.join("result.json"),
        serde_json::to_vec_pretty(&json!({
            "bucket": bucket, "initial": initial, "final": final_bucket, "policy": final_policy,
            "publicRead": 200, "privateReadDenied": true, "corsGetHeadRange": true,
            "foreignConditionalBindingPreserved": true, "runtimeIdentity": f.service_account
        }))
        .unwrap(),
    )
    .unwrap();
}
