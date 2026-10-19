//! A sandbox on a real Lambda MicroVM image, taken through pause and resume. Ignored: it needs the
//! `AWS_TARGET_*` credentials and an image already built from an agent bundle, named by
//! `AWS_SANDBOX_IMAGE_ARN` and `AWS_SANDBOX_IMAGE_VERSION`. It starts one billable MicroVM and
//! terminates it on every path.
//!
//! ```text
//! cargo test -p alien-bindings --test aws_sandbox_lifecycle_live -- --ignored --nocapture
//! ```

#![cfg(feature = "aws")]

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use alien_aws_clients::lambda_microvms::{LambdaMicrovmsApi, LambdaMicrovmsClient};
use alien_aws_clients::AwsCredentialProvider;
use alien_bindings::traits::{CreateSandboxRequest, SandboxState};
use alien_bindings::{BindingsProvider, BindingsProviderApi};
use alien_core::{AwsClientConfig, AwsCredentials, ClientConfig};

/// Generous against a suspend that took about 15s when measured.
const SUSPEND_TIMEOUT: Duration = Duration::from_secs(120);

fn env(name: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| panic!("{name} must be set"))
}

/// What the sandbox is asked to keep, and what each step proves:
///
/// - a file written before the pause reads back after the resume, so pause preserves the disk;
/// - that read is the first request after `resume`, so it can only succeed once `resume` waits
///   for the MicroVM to serve rather than for AWS to accept the call;
/// - `terminate` returns only once AWS reports the MicroVM terminated.
#[tokio::test]
#[ignore = "needs a live AWS account and a built MicroVM image"]
async fn a_sandbox_keeps_its_files_across_pause_and_resume() {
    let region = env("AWS_TARGET_REGION");
    let binding = serde_json::json!({
        "service": "sandbox-aws",
        "imageArn": env("AWS_SANDBOX_IMAGE_ARN"),
        "imageVersion": env("AWS_SANDBOX_IMAGE_VERSION"),
        "region": region.clone(),
        "allowEgress": true,
    });
    let aws = AwsClientConfig {
        account_id: env("AWS_TARGET_ACCOUNT_ID"),
        region,
        credentials: AwsCredentials::AccessKeys {
            access_key_id: env("AWS_TARGET_ACCESS_KEY_ID"),
            secret_access_key: env("AWS_TARGET_SECRET_ACCESS_KEY"),
            session_token: std::env::var("AWS_TARGET_SESSION_TOKEN")
                .ok()
                .filter(|token| !token.is_empty()),
        },
        service_overrides: None,
    };
    // Read directly because the binding reports `TERMINATING` as terminated, and the difference
    // between the two is what `terminate` is held to here.
    let microvms = LambdaMicrovmsClient::new(
        reqwest::Client::new(),
        AwsCredentialProvider::from_config_sync(aws.clone()),
    );
    let config = ClientConfig::Aws(Box::new(aws));
    let provider =
        BindingsProvider::new(config, HashMap::from([("box".to_string(), binding)])).unwrap();
    let sandbox = provider.load_sandbox("box").await.expect("loads");

    let instance = sandbox
        .create(CreateSandboxRequest::default())
        .await
        .unwrap_or_else(|error| panic!("create failed: {error}"));
    let id = instance.sandbox_id;
    eprintln!("created {id}");

    let marker = b"written before the pause".to_vec();
    let lifecycle = async {
        sandbox
            .write_files(
                &id,
                BTreeMap::from([("marker.txt".to_string(), marker.clone())]),
            )
            .await
            .map_err(|error| format!("write: {error}"))?;

        sandbox
            .pause(&id)
            .await
            .map_err(|error| format!("pause: {error}"))?;
        let deadline = Instant::now() + SUSPEND_TIMEOUT;
        loop {
            let state = sandbox
                .get(&id)
                .await
                .map_err(|error| format!("get while pausing: {error}"))?
                .map(|instance| instance.state);
            if state == Some(SandboxState::Paused) {
                break;
            }
            if Instant::now() >= deadline {
                return Err(format!("never reported paused; last state {state:?}"));
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        eprintln!("paused {id}");

        let resumed = Instant::now();
        sandbox
            .resume(&id)
            .await
            .map_err(|error| format!("resume: {error}"))?;
        eprintln!("resumed {id} in {:?}", resumed.elapsed());

        let read = sandbox
            .read_file(&id, "marker.txt")
            .await
            .map_err(|error| format!("first read after resume: {error}"))?;
        if read != marker {
            return Err(format!(
                "the file changed across the pause: {:?}",
                String::from_utf8_lossy(&read)
            ));
        }
        Ok::<(), String>(())
    }
    .await;

    // Terminated on every path, and judged after it: a failed lifecycle must not leave a
    // MicroVM billing, and a terminate that returned is a claim the record has to back up.
    let terminated = sandbox.terminate(&id).await;
    let after = microvms.get_microvm(&id).await;

    lifecycle.unwrap_or_else(|failure| panic!("{failure}"));
    terminated.unwrap_or_else(|error| panic!("terminate failed: {error}"));
    match after {
        Ok(record) => assert_eq!(
            record.state.as_deref(),
            Some("TERMINATED"),
            "terminate returned before AWS reported the MicroVM terminated"
        ),
        Err(error) => assert!(
            matches!(
                error.error,
                Some(alien_client_core::ErrorData::RemoteResourceNotFound { .. })
            ),
            "the record after terminate could not be read: {error}"
        ),
    }
}
