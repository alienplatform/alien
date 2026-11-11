//! Real-AWS proof that a deployment keeps installing while it waits for a deployer secret.
//!
//! A required deployer secret is read by one Worker (`reader`); a second Worker (`bystander`)
//! does not read it. Nothing writes the secret until the test has seen `bystander` running while
//! `reader` is held. Uses the prebuilt Lambda image (`ALIEN_TEST_AWS_LAMBDA_IMAGE`; override with
//! `ALIEN_E2E_SECRET_WAIT_IMAGE` when the target region needs its own copy).
//!
//!   cargo nextest run -p alien-test --test secret_wait --run-ignored all

use std::sync::Arc;
use std::time::{Duration, Instant};

use alien_aws_clients::ssm::{PutParameterRequest, SsmApi, SsmClient};
use alien_aws_clients::{AwsClientConfig, AwsCredentialProvider, AwsCredentials};
use alien_core::{
    PermissionProfile, Platform, ResourceLifecycle, StackInputDefinition,
    StackInputEnvironmentMapping, StackInputKind, StackInputProvider, Worker, WorkerCode,
};
use alien_test::{DeploymentModel, TestConfig, TestManager};

fn secret_input(id: &str, env_name: &str, targets: &[&str]) -> StackInputDefinition {
    StackInputDefinition {
        id: id.to_string(),
        kind: StackInputKind::Secret,
        when: None,
        provided_by: vec![StackInputProvider::Deployer],
        required: true,
        label: id.to_string(),
        description: "Proof secret".to_string(),
        placeholder: None,
        default: None,
        platforms: None,
        validation: None,
        generate: None,
        env: vec![StackInputEnvironmentMapping {
            name: env_name.to_string(),
            target_resources: Some(targets.iter().map(|t| t.to_string()).collect()),
            var_type: None,
        }],
    }
}

fn stack(image: &str, inputs: Vec<StackInputDefinition>) -> alien_core::Stack {
    // Workloads that share a permission profile must receive the same deployer secrets, so each
    // Worker gets its own profile.
    let worker = |id: &str| {
        Worker::new(id.to_string())
            .permissions(format!("{id}-profile"))
            .code(WorkerCode::Image {
                image: image.to_string(),
            })
            .build()
    };
    let mut stack = alien_core::Stack::new("secret-wait".to_string())
        .permission(
            "reader-profile",
            PermissionProfile::new().global(["worker/execute"]),
        )
        .permission(
            "bystander-profile",
            PermissionProfile::new().global(["worker/execute"]),
        )
        .add(worker("reader"), ResourceLifecycle::Live)
        .add(worker("bystander"), ResourceLifecycle::Live)
        .build();
    stack.inputs = inputs;
    stack
}

struct Snapshot {
    status: String,
    waiting: Vec<String>,
    worker_status: std::collections::BTreeMap<String, String>,
    secret_names: Vec<String>,
}

async fn snapshot(manager: &Arc<TestManager>, id: &str) -> anyhow::Result<Snapshot> {
    let resp = manager
        .client()
        .get_deployment()
        .id(id)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("get_deployment: {e}"))?;
    let meta = resp.runtime_metadata.clone().unwrap_or_default();
    let waiting = meta["resourcesWaitingForSecrets"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let secret_names = meta["deployerSecrets"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s["location"]["name"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let mut worker_status = std::collections::BTreeMap::new();
    if let Some(resources) = resp.stack_state.as_ref().and_then(|s| s["resources"].as_object()) {
        for (rid, r) in resources {
            worker_status.insert(rid.clone(), r["status"].as_str().unwrap_or("?").to_string());
        }
    }
    Ok(Snapshot {
        status: resp.status.clone(),
        waiting,
        worker_status,
        secret_names,
    })
}

async fn wait_for<F: Fn(&Snapshot) -> bool>(
    manager: &Arc<TestManager>,
    id: &str,
    what: &str,
    timeout: Duration,
    done: F,
) -> anyhow::Result<(Snapshot, Duration)> {
    let start = Instant::now();
    let mut last = String::new();
    loop {
        let s = snapshot(manager, id).await?;
        let line = format!("{} waiting={:?} resources={:?}", s.status, s.waiting, s.worker_status);
        if line != last {
            println!("[{:>5.1}s] {line}", start.elapsed().as_secs_f32());
            last = line.clone();
        }
        if done(&s) {
            return Ok((s, start.elapsed()));
        }
        if s.status.ends_with("failed") {
            anyhow::bail!("{what}: deployment failed: {line}");
        }
        if start.elapsed() > timeout {
            anyhow::bail!("timed out waiting for {what}: {line}");
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

async fn write_secret(config: &TestConfig, name: &str, value: &str) -> anyhow::Result<()> {
    let target = config.aws_target.as_ref().expect("target credentials");
    let ssm = SsmClient::new(
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
        .map_err(|e| anyhow::anyhow!("credentials: {e}"))?,
    );
    ssm.put_parameter(PutParameterRequest {
        name: name.to_string(),
        value: value.to_string(),
        parameter_type: "SecureString".to_string(),
        description: None,
        overwrite: Some(true),
        key_id: None,
        tier: None,
        tags: None,
        data_type: None,
    })
    .await
    .map_err(|e| anyhow::anyhow!("put_parameter {name}: {e}"))?;
    Ok(())
}

#[tokio::test]
#[ignore = "real AWS: creates and destroys Lambda functions in the target account"]
async fn aws_install_continues_while_waiting_for_a_secret() {
    alien_test::e2e::init_tracing();
    let config = TestConfig::from_env();
    let image = std::env::var("ALIEN_E2E_SECRET_WAIT_IMAGE")
        .ok()
        .or_else(|| config.aws_resources.lambda_image.clone())
        .expect("ALIEN_TEST_AWS_LAMBDA_IMAGE");
    let manager = Arc::new(
        TestManager::start_with_config(&config, &[Platform::Aws])
            .await
            .expect("manager"),
    );

    let stack = stack(&image, vec![secret_input("proofSecret", "PROOF_SECRET", &["reader"])]);
    let deployment = alien_test::e2e::create_release_and_deployment(
        &manager,
        Platform::Aws,
        DeploymentModel::Push,
        &stack,
        format!("e2e-secret-wait-{}", &uuid::Uuid::new_v4().to_string()[..8]),
        None,
    )
    .await
    .expect("deployment");
    println!("DEPLOYMENT_ID={}", deployment.id);

    let outcome: anyhow::Result<()> = async {
        let started = Instant::now();
        alien_test::setup::setup_target(
            &config,
            Platform::Aws,
            &deployment,
            &stack,
            &manager,
            manager.management_config(),
        )
        .await?;
        println!("setup_target done after {:?}", started.elapsed());

        // 1. While the secret is missing: only `reader` waits, `bystander` installs.
        let (held, took) = wait_for(&manager, &deployment.id, "bystander running while reader waits", Duration::from_secs(900), |s| {
            s.worker_status.get("bystander").map(String::as_str) == Some("running")
                && s.waiting == vec!["reader".to_string()]
        })
        .await?;
        println!("HELD_STATE after {took:?}: {:?} {:?}", held.waiting, held.worker_status);
        anyhow::ensure!(
            held.worker_status.get("reader").map(String::as_str) != Some("running"),
            "reader must not run before its secret exists"
        );
        let name = held.secret_names.first().cloned().expect("secret location name");

        // 2. Write the secret; the reader follows.
        let written = Instant::now();
        write_secret(&config, &name, "proof-value-not-a-real-secret").await?;
        let (_, to_running) = wait_for(&manager, &deployment.id, "running", Duration::from_secs(900), |s| {
            s.status == "running" && s.waiting.is_empty()
        })
        .await?;
        println!("RUNNING {:?} after the secret was written ({:?} since write)", to_running, written.elapsed());
        Ok(())
    }
    .await;

    // Always tear down by exact id.
    let destroy = deployment.destroy().await;
    println!("destroy requested: {:?}", destroy.is_ok());
    let cleanup =
        alien_test::setup::teardown_target(&config, Platform::Aws, &deployment.id, &manager).await;
    println!("teardown outcome: {:?}", cleanup.as_ref().map(|_| ()).map_err(|e| e.to_string()));
    outcome.expect("secret wait proof");
    cleanup.expect("teardown");
}

/// Update case and newer-release case on one deployment:
/// 1. install with `proofSecret` written up front and wait for `running`;
/// 2. release 2 adds a second required secret read only by `reader`: `bystander` is untouched and
///    `reader` keeps serving its old version while it waits;
/// 3. release 3 drops that requirement while the deployment still waits: the deployment takes the
///    newer release and reaches `running` without anyone writing the second secret.
#[tokio::test]
#[ignore = "real AWS: creates and destroys Lambda functions in the target account"]
async fn aws_update_waits_only_for_readers_and_takes_a_newer_release() {
    use alien_manager::auth::Subject;

    alien_test::e2e::init_tracing();
    let config = TestConfig::from_env();
    let image = std::env::var("ALIEN_E2E_SECRET_WAIT_IMAGE")
        .ok()
        .or_else(|| config.aws_resources.lambda_image.clone())
        .expect("ALIEN_TEST_AWS_LAMBDA_IMAGE");
    let manager = Arc::new(
        TestManager::start_with_config(&config, &[Platform::Aws])
            .await
            .expect("manager"),
    );
    let first = secret_input("proofSecret", "PROOF_SECRET", &["reader"]);
    let second = secret_input("secondSecret", "SECOND_SECRET", &["reader"]);
    let release1 = stack(&image, vec![first.clone()]);
    let release2 = stack(&image, vec![first.clone(), second]);
    let release3 = stack(&image, vec![first]);

    let deployment = alien_test::e2e::create_release_and_deployment(
        &manager,
        Platform::Aws,
        DeploymentModel::Push,
        &release1,
        format!("e2e-secret-update-{}", &uuid::Uuid::new_v4().to_string()[..8]),
        None,
    )
    .await
    .expect("deployment");
    println!("DEPLOYMENT_ID={}", deployment.id);

    let outcome: anyhow::Result<()> = async {
        alien_test::setup::setup_target(
            &config,
            Platform::Aws,
            &deployment,
            &release1,
            &manager,
            manager.management_config(),
        )
        .await?;
        let (held, _) = wait_for(&manager, &deployment.id, "first install held", Duration::from_secs(900), |s| {
            s.waiting == vec!["reader".to_string()]
                && s.worker_status.get("bystander").map(String::as_str) == Some("running")
        })
        .await?;
        let first_name = held.secret_names.first().cloned().expect("secret name");
        write_secret(&config, &first_name, "first-value").await?;
        wait_for(&manager, &deployment.id, "first release running", Duration::from_secs(900), |s| {
            s.status == "running" && s.waiting.is_empty()
        })
        .await?;
        println!("FIRST_RELEASE_RUNNING");

        // Release 2 adds a second required secret for `reader` only.
        let r2 = alien_test::e2e::create_release(&manager, Platform::Aws, &release2).await?;
        manager
            .deployment_store()
            .set_deployment_desired_release(&Subject::system(), &deployment.id, &r2)
            .await
            .map_err(|e| anyhow::anyhow!("point at release 2: {e}"))?;
        let started = Instant::now();
        let (waiting, _) = wait_for(&manager, &deployment.id, "update waits for the second secret", Duration::from_secs(600), |s| {
            s.waiting == vec!["reader".to_string()]
        })
        .await?;
        println!(
            "UPDATE_HELD after {:?}: {:?} {:?}",
            started.elapsed(),
            waiting.waiting,
            waiting.worker_status
        );
        anyhow::ensure!(
            waiting.worker_status.get("reader").map(String::as_str) == Some("running")
                && waiting.worker_status.get("bystander").map(String::as_str) == Some("running"),
            "both Workers must keep running during the update wait: {:?}",
            waiting.worker_status
        );

        // Release 3 arrives while the deployment still waits.
        let r3 = alien_test::e2e::create_release(&manager, Platform::Aws, &release3).await?;
        manager
            .deployment_store()
            .set_deployment_desired_release(&Subject::system(), &deployment.id, &r3)
            .await
            .map_err(|e| anyhow::anyhow!("point at release 3: {e}"))?;
        let newer = Instant::now();
        wait_for(&manager, &deployment.id, "newer release taken while waiting", Duration::from_secs(600), |s| {
            s.status == "running" && s.waiting.is_empty()
        })
        .await?;
        println!("NEWER_RELEASE_RUNNING after {:?}", newer.elapsed());
        let current = manager
            .client()
            .get_deployment()
            .id(&deployment.id)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("get_deployment: {e}"))?
            .current_release_id
            .clone();
        anyhow::ensure!(
            current.as_deref() == Some(r3.as_str()),
            "current release {current:?} should be release 3 {r3}"
        );
        Ok(())
    }
    .await;

    let destroy = deployment.destroy().await;
    println!("destroy requested: {:?}", destroy.is_ok());
    let cleanup =
        alien_test::setup::teardown_target(&config, Platform::Aws, &deployment.id, &manager).await;
    println!("teardown outcome: {:?}", cleanup.as_ref().map(|_| ()).map_err(|e| e.to_string()));
    outcome.expect("update proof");
    cleanup.expect("teardown");
}
