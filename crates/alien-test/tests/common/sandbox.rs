//! Checks for the `sandbox-session` app: a command run in a session of a source-built Live
//! sandbox, and, on AWS, where the image-build bundle lives.

use std::io::Read;
use std::time::Duration;

use alien_aws_clients::s3::{GetObjectRequest, S3Api, S3Client};
use alien_aws_clients::AwsCredentialProvider;
use alien_core::{AwsClientConfig, AwsCredentials, Platform};
use alien_test::config::{e2e_resource_prefix, TestConfig};
use alien_test::TestContext;
use anyhow::{bail, ensure, Context};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunResponse {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Calls the app's `/run` endpoint, which opens a sandbox session, reads the image's marker file
/// and ends the session, and checks that it ran in the image built from `sandbox/Dockerfile`.
pub async fn check_session_runs_a_command(ctx: &TestContext) -> anyhow::Result<()> {
    let url = ctx
        .deployment
        .url
        .as_deref()
        .context("the deployment has no public URL")?;
    let response = reqwest::Client::new()
        .post(format!("{}/run", url.trim_end_matches('/')))
        .bearer_auth(&ctx.deployment.id)
        // A cold MicroVM restore is seconds; the Lambda itself allows five minutes.
        .timeout(Duration::from_secs(300))
        .send()
        .await
        .context("POST /run")?;
    let status = response.status();
    let body = response.text().await.context("read /run response")?;
    ensure!(status.is_success(), "/run answered {status}: {body}");
    let run: RunResponse =
        serde_json::from_str(&body).with_context(|| format!("parse /run response: {body}"))?;
    ensure!(
        run.exit_code == Some(0),
        "the command exited {:?}: {}",
        run.exit_code,
        run.stderr
    );
    ensure!(
        run.stdout.trim() == "sandbox-session v1",
        "the session did not run the released image: {:?}",
        run.stdout
    );
    Ok(())
}

/// On AWS the controller renders the bundle into `<prefix>-agents-sbx` in the deployment's own
/// account, and the bundle builds `FROM` the release's image in the manager's registry. Reads the
/// bucket with the target account's credentials, which is where it must be.
pub async fn check_bundle_in_deployment_bucket(ctx: &TestContext) -> anyhow::Result<()> {
    if ctx.platform != Platform::Aws {
        return Ok(());
    }
    let config = TestConfig::from_env();
    let target = config
        .aws_target
        .as_ref()
        .context("AWS target credentials are required")?;
    let management = config
        .aws_mgmt
        .as_ref()
        .context("AWS management credentials are required")?;
    let management_account = management
        .account_id
        .as_deref()
        .context("AWS_MANAGEMENT_ACCOUNT_ID is required")?;
    let s3 = S3Client::new(
        reqwest::Client::new(),
        AwsCredentialProvider::from_config(AwsClientConfig {
            account_id: target.account_id.clone().unwrap_or_default(),
            region: target.region.clone(),
            credentials: AwsCredentials::AccessKeys {
                access_key_id: target.access_key_id.clone(),
                secret_access_key: target.secret_access_key.clone(),
                session_token: target.session_token.clone(),
            },
            service_overrides: None,
        })
        .await
        .map_err(|error| anyhow::anyhow!("target S3 credentials: {error}"))?,
    );

    let bucket = format!("{}-agents-sbx", e2e_resource_prefix()?);
    let listed = s3
        .list_objects_v2(&bucket, Some("sandbox-bundle/".to_string()), None)
        .await
        .map_err(|error| anyhow::anyhow!("list {bucket}: {error}"))?;
    let [object] = listed.contents.as_slice() else {
        bail!(
            "expected exactly one bundle in {bucket}, found {:?}",
            listed.contents.iter().map(|o| &o.key).collect::<Vec<_>>()
        );
    };
    let zip = s3
        .get_object(&GetObjectRequest {
            bucket: bucket.clone(),
            key: object.key.clone(),
            version_id: None,
            range: None,
        })
        .await
        .map_err(|error| anyhow::anyhow!("get {bucket}/{}: {error}", object.key))?
        .body;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip)).context("bundle is a zip")?;
    let mut dockerfile = String::new();
    archive
        .by_name("Dockerfile")
        .context("the bundle holds a Dockerfile at its root")?
        .read_to_string(&mut dockerfile)
        .context("read Dockerfile")?;
    let from = dockerfile
        .trim()
        .strip_prefix("FROM ")
        .with_context(|| format!("the Dockerfile is one FROM line: {dockerfile:?}"))?;
    ensure!(
        !from.contains(char::is_whitespace)
            && from.starts_with(&format!("{management_account}.dkr.ecr."))
            && from.contains("@sha256:"),
        "the bundle must build from the release's digest-pinned image in the manager's registry: \
         {dockerfile:?}"
    );
    Ok(())
}
