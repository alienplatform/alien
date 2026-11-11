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
    stack_at(image, inputs, "1")
}

fn stack_at(image: &str, inputs: Vec<StackInputDefinition>, release: &str) -> alien_core::Stack {
    // Workloads that share a permission profile must receive the same deployer secrets, so each
    // Worker gets its own profile.
    let worker = |id: &str| {
        Worker::new(id.to_string())
            .permissions(format!("{id}-profile"))
            .environment(std::collections::HashMap::from([(
                "PROOF_RELEASE".to_string(),
                release.to_string(),
            )]))
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
    release_marker: std::collections::BTreeMap<String, String>,
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
    let mut release_marker = std::collections::BTreeMap::new();
    if let Some(resources) = resp.stack_state.as_ref().and_then(|s| s["resources"].as_object()) {
        for (rid, r) in resources {
            worker_status.insert(rid.clone(), r["status"].as_str().unwrap_or("?").to_string());
            if let Some(marker) = r["config"]["environment"]["PROOF_RELEASE"].as_str() {
                release_marker.insert(rid.clone(), marker.to_string());
            }
        }
    }
    Ok(Snapshot {
        status: resp.status.clone(),
        waiting,
        worker_status,
        secret_names,
        release_marker,
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
        let line = format!("{} waiting={:?} resources={:?} marker={:?}", s.status, s.waiting, s.worker_status, s.release_marker);
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

async fn ssm_client(config: &TestConfig) -> SsmClient {
    let target = config.aws_target.as_ref().expect("target credentials");
    let credentials = AwsCredentialProvider::from_config(AwsClientConfig {
        account_id: target.account_id.clone().unwrap_or_default(),
        region: target.region.clone(),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: target.access_key_id.clone(),
            secret_access_key: target.secret_access_key.clone(),
            session_token: target.session_token.clone(),
        },
        service_overrides: None,
    })
    .await;
    SsmClient::new(reqwest::Client::new(), credentials.expect("credentials"))
}

/// The deployer secrets live in the customer's own store and outlive the deployment, so a stale
/// value from an earlier run would make the next run skip the wait. Delete the names this test
/// uses before it starts and after it ends.
async fn delete_proof_secrets(config: &TestConfig) {
    let prefix = alien_test::config::e2e_resource_prefix().expect("slot");
    let ssm = ssm_client(config).await;
    for key in ["proof-secret", "second-secret"] {
        let name = format!("{prefix}-deployer-secrets-{key}");
        let result = ssm.delete_parameter(&name).await;
        println!("delete {name}: {:?}", result.map_err(|e| e.to_string()));
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
    delete_proof_secrets(&config).await;
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
    delete_proof_secrets(&config).await;
    outcome.expect("secret wait proof");
    cleanup.expect("teardown");
}

/// Update case and newer-release case on one deployment:
/// 1. install with the secret written and wait for `running`;
/// 2. the secret goes missing, then release 2 changes a plain variable on both Workers: `bystander`
///    takes release 2, `reader` keeps serving release 1 while it waits;
/// 3. release 3 arrives while the deployment still waits: `bystander` takes it too;
/// 4. the secret is written again and `reader` follows to release 3.
///
/// (Adding a new required secret to an existing permission profile is a setup change, so a push
/// update cannot do that; a missing secret at update time is the case that reaches the wait.)
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
    delete_proof_secrets(&config).await;
    let manager = Arc::new(
        TestManager::start_with_config(&config, &[Platform::Aws])
            .await
            .expect("manager"),
    );
    let input = secret_input("proofSecret", "PROOF_SECRET", &["reader"]);
    let release1 = stack_at(&image, vec![input.clone()], "1");
    let release2 = stack_at(&image, vec![input.clone()], "2");
    let release3 = stack_at(&image, vec![input], "3");

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
        let name = held.secret_names.first().cloned().expect("secret name");
        println!("SECRET_NAME={name}");
        write_secret(&config, &name, "first-value").await?;
        wait_for(&manager, &deployment.id, "first release running", Duration::from_secs(900), |s| {
            s.status == "running" && s.waiting.is_empty()
        })
        .await?;
        println!("FIRST_RELEASE_RUNNING");

        // The secret goes missing; release 2 changes a plain variable on both Workers.
        delete_proof_secrets(&config).await;
        let r2 = alien_test::e2e::create_release(&manager, Platform::Aws, &release2).await?;
        manager
            .deployment_store()
            .set_deployment_desired_release(&Subject::system(), &deployment.id, &r2)
            .await
            .map_err(|e| anyhow::anyhow!("point at release 2: {e}"))?;
        let started = Instant::now();
        let (mid, _) = wait_for(&manager, &deployment.id, "bystander takes release 2 while reader waits", Duration::from_secs(600), |s| {
            s.waiting == vec!["reader".to_string()]
                && s.release_marker.get("bystander").map(String::as_str) == Some("2")
        })
        .await?;
        println!("UPDATE_HELD after {:?}: {:?} {:?}", started.elapsed(), mid.waiting, mid.release_marker);
        anyhow::ensure!(
            mid.release_marker.get("reader").map(String::as_str) == Some("1")
                && mid.worker_status.get("reader").map(String::as_str) == Some("running"),
            "reader keeps serving release 1 while its secret is missing: {:?}",
            mid.release_marker
        );

        // Release 3 arrives while the deployment still waits.
        let r3 = alien_test::e2e::create_release(&manager, Platform::Aws, &release3).await?;
        manager
            .deployment_store()
            .set_deployment_desired_release(&Subject::system(), &deployment.id, &r3)
            .await
            .map_err(|e| anyhow::anyhow!("point at release 3: {e}"))?;
        let newer = Instant::now();
        wait_for(&manager, &deployment.id, "newer release reaches bystander while waiting", Duration::from_secs(600), |s| {
            s.waiting == vec!["reader".to_string()]
                && s.release_marker.get("bystander").map(String::as_str) == Some("3")
        })
        .await?;
        println!("NEWER_RELEASE_TAKEN_WHILE_WAITING after {:?}", newer.elapsed());

        // Write the secret again: reader follows to release 3.
        write_secret(&config, &name, "second-value").await?;
        wait_for(&manager, &deployment.id, "reader on release 3", Duration::from_secs(600), |s| {
            s.status == "running"
                && s.waiting.is_empty()
                && s.release_marker.values().all(|m| m == "3")
        })
        .await?;
        let current = manager
            .client()
            .get_deployment()
            .id(&deployment.id)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("get_deployment: {e}"))?
            .current_release_id
            .clone();
        println!("CURRENT_RELEASE={current:?} expected={r3}");
        anyhow::ensure!(current.as_deref() == Some(r3.as_str()), "current release should be release 3");
        Ok(())
    }
    .await;

    let destroy = deployment.destroy().await;
    println!("destroy requested: {:?}", destroy.is_ok());
    let cleanup =
        alien_test::setup::teardown_target(&config, Platform::Aws, &deployment.id, &manager).await;
    println!("teardown outcome: {:?}", cleanup.as_ref().map(|_| ()).map_err(|e| e.to_string()));
    delete_proof_secrets(&config).await;
    outcome.expect("update proof");
    cleanup.expect("teardown");
}
