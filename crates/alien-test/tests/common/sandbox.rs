//! Checks for the `sandbox-session` app: a command run in a session of a source-built Live
//! sandbox, and, on AWS, where the image-build bundle lives.

use std::io::Read;
use std::time::Duration;

use alien_aws_clients::s3::{GetObjectRequest, S3Api, S3Client};
use alien_aws_clients::AwsCredentialProvider;
use alien_core::sandbox_build_role::SANDBOX_BUNDLE_STORAGE_ID;
use alien_core::{AwsClientConfig, AwsCredentials, Platform, StackState, StorageOutputs};
use alien_test::config::TestConfig;
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

/// The bucket the deployment's Storage `storage_id` was created as, from its stack state.
async fn storage_bucket(ctx: &TestContext, storage_id: &str) -> anyhow::Result<String> {
    let response = ctx
        .deployment
        .manager()
        .client()
        .get_deployment()
        .id(&ctx.deployment.id)
        .send()
        .await
        .map_err(|error| anyhow::anyhow!("get_deployment failed: {error}"))?;
    let state: StackState = serde_json::from_value(
        response
            .into_inner()
            .stack_state
            .context("deployment is missing stack_state")?,
    )
    .context("failed to parse stack_state")?;
    Ok(state
        .get_resource_outputs::<StorageOutputs>(storage_id)
        .with_context(|| format!("the deployment has no Storage '{storage_id}'"))?
        .bucket_name
        .clone())
}

#[derive(Debug, Deserialize)]
struct WriteResponse {
    written: bool,
    code: Option<String>,
}

/// Asks the Worker to write an object with its own credentials.
async fn workload_write(
    ctx: &TestContext,
    bucket: &str,
    key: &str,
) -> anyhow::Result<WriteResponse> {
    let url = ctx
        .deployment
        .url
        .as_deref()
        .context("the deployment has no public URL")?;
    let response = reqwest::Client::new()
        .post(format!("{}/write-object", url.trim_end_matches('/')))
        .bearer_auth(&ctx.deployment.id)
        .json(&serde_json::json!({ "bucket": bucket, "key": key }))
        .timeout(Duration::from_secs(60))
        .send()
        .await
        .context("POST /write-object")?;
    let status = response.status();
    let body = response
        .text()
        .await
        .context("read /write-object response")?;
    ensure!(
        status.is_success(),
        "/write-object answered {status}: {body}"
    );
    serde_json::from_str(&body).with_context(|| format!("parse /write-object response: {body}"))
}

/// A bundle decides what a sandbox runs, so only the deployment's management role may write one.
/// The Worker holds a stack-wide storage write grant: it writes the ordinary bucket, and is refused
/// on the bundle bucket.
pub async fn check_workload_cannot_write_bundles(ctx: &TestContext) -> anyhow::Result<()> {
    if ctx.platform != Platform::Aws {
        return Ok(());
    }
    let uploads = storage_bucket(ctx, "uploads").await?;
    let control = workload_write(ctx, &uploads, "probe/workload.txt").await?;
    ensure!(
        control.written,
        "the Worker's stack-wide grant should write '{uploads}': {:?}",
        control.code
    );
    let bundles = storage_bucket(ctx, SANDBOX_BUNDLE_STORAGE_ID).await?;
    let attempt =
        workload_write(ctx, &bundles, "sandbox-bundle/agents/workload/bundle.zip").await?;
    ensure!(
        !attempt.written && attempt.code.as_deref() == Some("AccessDenied"),
        "the Worker must be refused on the bundle bucket '{bundles}', got {attempt:?}"
    );
    Ok(())
}

/// On AWS the controller renders the bundle into the deployment's bundle Storage, in the
/// deployment's own account, and the bundle builds `FROM` the release's image in the manager's
/// registry. Reads the bucket with the target account's credentials, which is where it must be.
///
/// Returns the bucket's name.
pub async fn check_bundle_in_deployment_bucket(ctx: &TestContext) -> anyhow::Result<String> {
    let config = TestConfig::from_env();
    let management = config
        .aws_mgmt
        .as_ref()
        .context("AWS management credentials are required")?;
    let management_account = management
        .account_id
        .as_deref()
        .context("AWS_MANAGEMENT_ACCOUNT_ID is required")?;
    let s3 = target_s3().await?;

    let bucket = storage_bucket(ctx, SANDBOX_BUNDLE_STORAGE_ID).await?;
    let listed = s3
        .list_objects_v2(&bucket, Some("sandbox-bundle/agents/".to_string()), None)
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
    Ok(bucket)
}

/// After teardown the bundle bucket must be gone: unlike the deployment's other Storage it holds
/// only build inputs, so setup deletes it with the stack.
pub async fn check_bundle_bucket_deleted(bucket: &str) -> anyhow::Result<()> {
    match target_s3().await?.head_bucket(bucket).await {
        Err(error) if error.code == "REMOTE_RESOURCE_NOT_FOUND" => {
            tracing::info!(%bucket, "sandbox bundle bucket deleted with the deployment");
            Ok(())
        }
        Ok(()) => bail!("the sandbox bundle bucket '{bucket}' outlived the deployment"),
        Err(error) => bail!("HeadBucket '{bucket}' failed: {error}"),
    }
}

/// An S3 client with the target account's credentials.
async fn target_s3() -> anyhow::Result<S3Client> {
    let config = TestConfig::from_env();
    let target = config
        .aws_target
        .as_ref()
        .context("AWS target credentials are required")?;
    Ok(S3Client::new(
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
    ))
}
