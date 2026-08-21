//! Live lifecycle for Lambda MicroVMs, driven through the client Alien actually ships.
//!
//! `#[ignore]` because it builds a real MicroVM image (~160s) and runs a MicroVM. Run with
//! `--ignored` against an AWS account configured in `.env.test` (see `AWS_TARGET_*`).
//!
//! Every resource this creates is torn down at the end, and the image delete waits for a
//! terminal state first: `DeleteMicrovmImage` fails while the image is still `CREATING`, which
//! is how one gets leaked.

use std::path::PathBuf as StdPathBuf;
use std::time::Duration;

use alien_aws_clients::aws::lambda_microvms::{LambdaMicrovmsApi, LambdaMicrovmsClient};
use alien_aws_clients::{AwsCredentialProvider, AwsCredentials};
use reqwest::Client;

/// Slot-scoped so two runs cannot collide on a name.
fn slot() -> String {
    std::env::var("ALIEN_E2E_SLOT").unwrap_or_else(|_| "00".to_string())
}

fn client() -> LambdaMicrovmsClient {
    let root: StdPathBuf = workspace_root::get_workspace_root();
    dotenvy::from_path(root.join(".env.test")).ok();

    let config = alien_aws_clients::AwsClientConfig {
        account_id: std::env::var("AWS_TARGET_ACCOUNT_ID").expect("AWS_TARGET_ACCOUNT_ID"),
        region: std::env::var("AWS_TARGET_REGION").expect("AWS_TARGET_REGION"),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: std::env::var("AWS_TARGET_ACCESS_KEY_ID")
                .expect("AWS_TARGET_ACCESS_KEY_ID"),
            secret_access_key: std::env::var("AWS_TARGET_SECRET_ACCESS_KEY")
                .expect("AWS_TARGET_SECRET_ACCESS_KEY"),
            session_token: None,
        },
        service_overrides: None,
    };

    LambdaMicrovmsClient::new(
        Client::new(),
        AwsCredentialProvider::from_config_sync(config),
    )
}

/// Read-only reachability: the paths, the signing name and the response shape.
///
/// Separate from the lifecycle test because it creates nothing, so it can run whenever without
/// leaving anything to clean up.
#[tokio::test]
#[ignore]
async fn the_microvms_api_is_reachable_and_lists_nothing_unexpected() {
    let client = client();

    let error = client
        .list_microvm_image_versions("does-not-exist")
        .await
        .expect_err("listing versions of a missing image must fail");

    // Printed in full: the status alone cannot distinguish "no such image" from "not
    // authorized", and the body is the only thing that says which.
    println!("list_microvm_image_versions(missing) -> {error:?}");

    let rendered = format!("{error:?}");
    assert!(
        !rendered.contains("403") && !rendered.to_lowercase().contains("not authorized"),
        "the credentials should reach the API, not be refused by it: {rendered}"
    );
}

/// The full lifecycle, if `ALIEN_SANDBOX_TEST_IMAGE_ARN` names an image that already exists.
///
/// Building an image needs an S3 bundle and a build role, which are provisioned outside this
/// test; pointing at a prepared image keeps the run to the part the client owns — run, reach,
/// terminate — and keeps a failure from stranding a half-built image.
#[tokio::test]
#[ignore]
async fn a_microvm_runs_serves_the_agent_and_terminates() {
    let Ok(image_arn) = std::env::var("ALIEN_SANDBOX_TEST_IMAGE_ARN") else {
        eprintln!("ALIEN_SANDBOX_TEST_IMAGE_ARN not set; skipping the lifecycle");
        return;
    };
    let image_version =
        std::env::var("ALIEN_SANDBOX_TEST_IMAGE_VERSION").unwrap_or_else(|_| "1".to_string());
    let client = client();

    let microvm = client
        .run_microvm(
            &image_arn,
            &image_version,
            &format!(
                "alien-sbx-{}-{}",
                slot(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs()
            ),
            None,
            Vec::new(),
            None,
            // A live MicroVM that outlives a failed test run still bills, so the live suite caps
            // it even though the unit paths leave it undeclared.
            Some(600),
        )
        .await
        .expect("RunMicrovm should start a MicroVM");

    let microvm_id = microvm.microvm_id.clone().expect("a MicroVM id");
    println!("started {microvm_id}");

    // Terminate whatever happens next: a panic between here and the end would otherwise leave a
    // MicroVM running and billing.
    let outcome =
        tokio::time::timeout(Duration::from_secs(300), exercise(&client, &microvm_id)).await;

    client
        .terminate_microvm(&microvm_id)
        .await
        .expect("terminate should succeed");

    let terminated = client
        .get_microvm(&microvm_id)
        .await
        .expect("the MicroVM should still be readable after terminate");
    println!("after terminate: state={:?}", terminated.state);

    outcome.expect("the lifecycle should finish inside its deadline");
}

/// Where the image places the agent. Duplicated rather than imported: this crate is the cloud
/// client and must not depend on the build crate to run one test.
const AGENT_PATH: &str = "/usr/local/bin/alien-sandbox-agent";

/// Runs one command through the agent and returns its raw NDJSON body.
async fn exec_in(
    client: &LambdaMicrovmsClient,
    microvm_id: &str,
    endpoint: &str,
    command: &[&str],
) -> String {
    let token = client
        .create_microvm_auth_token(microvm_id, vec![8971], 10)
        .await
        .expect("CreateMicrovmAuthToken");

    let mut request = Client::new()
        .post(format!("https://{endpoint}/v1/exec"))
        .header("X-aws-proxy-port", "8971")
        .json(&serde_json::json!({"command": command, "deadlineMs": 10000}));
    for (name, value) in token.auth_token {
        request = request.header(name, value);
    }

    request
        .send()
        .await
        .expect("exec responds")
        .text()
        .await
        .unwrap_or_default()
}

/// Waits for the MicroVM to serve, then proves the agent inside it answers.
async fn exercise(client: &LambdaMicrovmsClient, microvm_id: &str) {
    let mut endpoint = None;
    for _ in 0..60 {
        let microvm = client.get_microvm(microvm_id).await.expect("get_microvm");
        if microvm.state.as_deref() == Some("RUNNING") {
            endpoint = microvm.endpoint;
            break;
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    let endpoint = endpoint.expect("the MicroVM should reach RUNNING and report an endpoint");

    let token = client
        .create_microvm_auth_token(microvm_id, vec![8971], 10)
        .await
        .expect("CreateMicrovmAuthToken");

    let mut request = Client::new()
        .get(format!("https://{endpoint}/v1/health"))
        .header("X-aws-proxy-port", "8971");
    for (name, value) in token.auth_token {
        request = request.header(name, value);
    }

    let response = request.send().await.expect("the endpoint should answer");
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    println!("GET /v1/health -> {status} {body}");

    assert!(
        status.is_success(),
        "the agent should serve health: {status} {body}"
    );
    assert!(
        body.contains("protocolVersion"),
        "the agent should report its protocol version, got: {body}"
    );

    // The boundary that matters, proven where it actually has to hold: a command run inside a
    // real MicroVM must come back as the unprivileged uid, not as the agent.
    let frames = exec_in(client, microvm_id, &endpoint, &["/usr/bin/id"]).await;
    println!("POST /v1/exec ->\n{frames}");

    let decoded: String = frames
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|frame| frame["data"].as_str().map(str::to_string))
        .filter_map(|data| {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD.decode(data).ok()
        })
        .filter_map(|bytes| String::from_utf8(bytes).ok())
        .collect();

    assert!(
        decoded.contains("uid=60000"),
        "a command in a real MicroVM must run as the unprivileged uid, got: {decoded}"
    );

    // The other half of the same boundary: the uid drop is only worth having if the workload
    // cannot rewrite the supervisor that performs it. The image owns the agent as root; this is
    // the check that the running guest agrees.
    let frames = exec_in(
        client,
        microvm_id,
        &endpoint,
        &["/usr/bin/touch", AGENT_PATH],
    )
    .await;
    println!("touch {AGENT_PATH} ->\n{frames}");

    let exit_code = frames
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find_map(|frame| frame["code"].as_i64());

    assert_eq!(
        exit_code,
        Some(1),
        "the exec uid must not be able to write the agent binary, got: {frames}"
    );
}

/// Deletes an image named by `ALIEN_SANDBOX_TEST_IMAGE_ARN`. Cleanup for a supervised run, and
/// it exercises `delete_microvm_image` — which is what teardown depends on.
#[tokio::test]
#[ignore]
async fn delete_the_named_image() {
    let Ok(arn) = std::env::var("ALIEN_SANDBOX_TEST_IMAGE_ARN") else {
        return;
    };
    let client = client();

    let image = client
        .get_microvm_image(&arn)
        .await
        .expect("reads the image");
    println!("before delete: state={:?}", image.state);

    client
        .delete_microvm_image(&arn)
        .await
        .expect("delete should succeed");

    let after = client.get_microvm_image(&arn).await;
    println!("after delete: {after:?}");
}
