//! A custom resource deployed by the Operator, its controller a process started from the
//! verified bundle in the Operator's cache.
//!
//! The controller process is this test binary, run as `controller_process`, which answers steps
//! with the step protocol's test controller. The binary the bundle pins is a script that starts
//! it, so the cache verifies and runs it as it does any controller.

use super::*;
use crate::config::OperatorConfig;
use crate::controller_pods::{Cluster, ControllerPod};
use crate::db::OperatorDb;
use crate::OperatorState;
use alien_aws_clients::sts::{StsApi, StsClient};
use alien_core::{
    AwsClientConfig, AwsCredentials, AwsServiceOverrides, DeploymentConfig, DeploymentStatus,
    EnvironmentVariablesSnapshot, InitialSetupAuthority, Platform, ReleaseInfo, ResourceStatus,
    RuntimeMetadata, StackSettings, StackState,
};
use alien_infra::extension_bundle::{host_target, testing::TestBundle};
use alien_infra::step_protocol::protocol::{Credentials, StepRequest};
use alien_infra::step_protocol::test_controller::{ControllerHost, MockCloud, DNS_ZONE};
use axum::{extract::State, http::HeaderMap, routing::post, Router};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write as _;
use tokio::io::AsyncBufReadExt as _;
use tokio_util::sync::CancellationToken;

const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const IDENTITY_ROLE: &str = "arn:aws:iam::210987654321:role/test-acme-ext";
/// The token Kubernetes projects into a pod for its service account.
const POD_TOKEN: &str = "the-pods-own-token";

/// The controller process the other tests start. It writes its replies to descriptor 3, since
/// the test harness prints to stdout. With `CALLER_IDENTITY_URL` set, each step that carries AWS
/// credentials first makes a signed call there with them, as a controller's handler would.
#[tokio::test]
#[ignore = "the controller process the other tests start"]
async fn controller_process() {
    use std::os::fd::FromRawFd as _;
    let host = ControllerHost::new(std::env::var("MOCK_CLOUD_DIR").unwrap(), None);
    let caller_identity = std::env::var("CALLER_IDENTITY_URL").ok();
    // SAFETY: the script that starts this process opens descriptor 3 on the parent's pipe.
    let mut replies = unsafe { std::fs::File::from_raw_fd(3) };
    let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await.unwrap() {
        let request: StepRequest = serde_json::from_str(&line).unwrap();
        if let (
            Some(url),
            Some(Credentials::Aws {
                account_id,
                region,
                access_key_id,
                secret_access_key,
                session_token,
                ..
            }),
        ) = (&caller_identity, &request.credentials)
        {
            let config = AwsClientConfig {
                account_id: account_id.clone(),
                region: region.clone(),
                credentials: AwsCredentials::AccessKeys {
                    access_key_id: access_key_id.clone(),
                    secret_access_key: secret_access_key.clone(),
                    session_token: session_token.clone(),
                },
                service_overrides: Some(AwsServiceOverrides {
                    endpoints: [("sts".to_string(), url.clone())].into(),
                }),
            };
            StsClient::new(reqwest::Client::new(), config)
                .get_caller_identity()
                .await
                .unwrap();
        }
        let reply = host.handle(request).await;
        writeln!(replies, "{}", serde_json::to_string(&reply).unwrap()).unwrap();
        replies.flush().unwrap();
    }
}

/// A bundle whose controller for this machine starts `controller_process` with `env`.
fn bundle(env: &[(&str, String)]) -> TestBundle {
    let exe = std::env::current_exe().unwrap();
    let env: String = env
        .iter()
        .map(|(name, value)| format!("{name}='{value}' "))
        .collect();
    let script = format!(
        "#!/bin/sh\n{env}exec '{}' --ignored --exact extension_bundles::tests::controller_process --nocapture --test-threads=1 3>&1 1>&2\n",
        exe.display()
    );
    let target = host_target().expect("tests run on a machine with a controller target");
    TestBundle::new("acme", "1.0.0", &[(target, script.as_bytes())])
}

/// Writes `bundle` into the cache under `data_dir`, as the sync loop leaves a verified download.
fn cache_bundle(data_dir: &std::path::Path, bundle: &TestBundle) {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = cache(data_dir.to_str().unwrap())
        .dir(&bundle.manifest)
        .unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("core.zip"), &bundle.zip).unwrap();
    for (target, bytes) in &bundle.controllers {
        let path = dir.join(format!("controller-{target}"));
        std::fs::write(&path, bytes).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// A stack with the zone at `domain`, whose type the bundle's extension defines with a
/// controller on `platform`, and `resources` beside it.
fn stack(bundle: &TestBundle, platform: &str, domain: &str, resources: Value) -> alien_core::Stack {
    let mut manifest = serde_json::to_value(&bundle.manifest).unwrap();
    manifest["resourceTypes"] = json!({
        DNS_ZONE: {
            "lifecycles": ["live"],
            "holdsData": false,
            "setup": { "cloudformation": [], "terraform": [], "helm": [], "importers": [] },
            "controllerPlatforms": [platform],
            "permissionSets": [],
            "link": { "permissionSets": [] },
            "schemas": { "config": {}, "outputs": {} }
        }
    });
    let mut all = json!({
        "zone": {
            "config": { "type": DNS_ZONE, "id": "zone", "domain": domain },
            "lifecycle": "live",
            "dependencies": []
        }
    });
    all.as_object_mut()
        .unwrap()
        .extend(resources.as_object().unwrap().clone());
    serde_json::from_value(json!({
        "id": "acme",
        "resources": all,
        "permissions": { "profiles": {}, "management": "auto" },
        "extensions": [manifest]
    }))
    .unwrap()
}

fn release(id: &str, stack: alien_core::Stack) -> ReleaseInfo {
    ReleaseInfo {
        release_id: Some(id.to_string()),
        version: Some("1.0.0".to_string()),
        description: None,
        stack,
    }
}

/// Where initial setup starts for `stack`, already prepared.
fn initial_setup(
    platform: Platform,
    stack: alien_core::Stack,
    stack_state: StackState,
    authority: InitialSetupAuthority,
) -> DeploymentState {
    DeploymentState {
        status: DeploymentStatus::InitialSetup,
        platform,
        current_release: None,
        target_release: Some(release("rel_1", stack.clone())),
        stack_state: Some(stack_state),
        error: None,
        environment_info: None,
        runtime_metadata: Some(RuntimeMetadata {
            initial_setup_authority: authority,
            prepared_stack: Some(stack),
            pending_prepared_release_id: Some("rel_1".to_string()),
            ..RuntimeMetadata::default()
        }),
        retry_requested: false,
        protocol_version: alien_core::DEPLOYMENT_PROTOCOL_VERSION,
    }
}

fn config() -> DeploymentConfig {
    DeploymentConfig::builder()
        .stack_settings(StackSettings::default())
        .environment_variables(EnvironmentVariablesSnapshot {
            variables: vec![],
            hash: "hash".to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
        })
        .external_bindings(Default::default())
        .allow_frozen_changes(false)
        .build()
}

/// The zone's outputs in `state`.
fn zone_outputs(state: &DeploymentState) -> Value {
    let zone = &state.stack_state.as_ref().unwrap().resources["zone"];
    serde_json::to_value(zone.outputs.as_ref().unwrap()).unwrap()
}

/// The pid of each controller process the Operator holds.
fn process_ids(state: &OperatorState) -> Vec<u32> {
    let Controllers::Processes(processes) = state.extension_controllers.as_ref() else {
        panic!("this Operator runs its controllers as processes");
    };
    processes
        .by_binary
        .try_lock()
        .unwrap()
        .values()
        .filter_map(|transport| transport.child_pid())
        .collect()
}

/// Runs the Operator's deployment loop until the deployment reaches one of `statuses`.
async fn run_until(state: &OperatorState, statuses: &[DeploymentStatus]) -> DeploymentState {
    // The secrets digest key the Operator installs at startup.
    alien_deployment::set_ephemeral_secrets_digest_key();
    for _ in 0..40 {
        crate::loops::deployment::run_deployment_continuously(state)
            .await
            .unwrap_or_else(|e| panic!("the deployment loop failed: {e:#?}"));
        let current = state.db.get_deployment_state().await.unwrap().unwrap();
        if statuses.contains(&current.status) {
            return current;
        }
    }
    let current = state.db.get_deployment_state().await.unwrap().unwrap();
    panic!(
        "never reached {statuses:?}; last {:?}: {:#?}",
        current.status, current.error
    );
}

/// Stages `target` for the deployment loop, as the sync loop does when it accepts a release.
async fn stage(state: &OperatorState, status: DeploymentStatus, target: ReleaseInfo) {
    let mut current = state.db.get_deployment_state().await.unwrap().unwrap();
    current.status = status;
    current.target_release = Some(target);
    state.db.set_deployment_state(&current).await.unwrap();
    state.db.set_deployment_config(&config()).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_operator_creates_updates_and_deletes_a_custom_resource_through_its_controller_process()
{
    let vault = tempfile::tempdir().unwrap();
    std::env::set_var("TEST_VAULT_DATA_DIR", vault.path());
    let data_dir = tempfile::tempdir().unwrap();
    let cloud = tempfile::tempdir().unwrap();
    let bundle = bundle(&[("MOCK_CLOUD_DIR", cloud.path().display().to_string())]);
    cache_bundle(data_dir.path(), &bundle);
    let db = OperatorDb::new(data_dir.path().to_str().unwrap(), KEY)
        .await
        .unwrap();
    db.set_deployment_id("dep_pull").await.unwrap();
    let first = stack(&bundle, "test", "a.acme.dev", json!({}));
    db.set_deployment_state(&initial_setup(
        Platform::Test,
        first,
        StackState::new(Platform::Test),
        InitialSetupAuthority::DirectSetup,
    ))
    .await
    .unwrap();
    db.set_deployment_config(&config()).await.unwrap();
    let state = OperatorState {
        config: OperatorConfig::builder()
            .platform(Platform::Test)
            .data_dir(data_dir.path().to_str().unwrap())
            .encryption_key(KEY)
            .build(),
        db: Arc::new(db),
        service_provider: None,
        extension_controllers: Default::default(),
        operations_sync_handler: None,
        pod_log_collection: false,
        cancel: CancellationToken::new(),
    };

    let created = run_until(
        &state,
        &[
            DeploymentStatus::Running,
            DeploymentStatus::ProvisioningFailed,
        ],
    )
    .await;
    assert_eq!(
        created.status,
        DeploymentStatus::Running,
        "{:#?}",
        created.error
    );
    assert_eq!(zone_outputs(&created)["domain"], "a.acme.dev");
    let pids = process_ids(&state);
    assert_eq!(pids.len(), 1, "one process serves the extension's steps");

    stage(
        &state,
        DeploymentStatus::UpdatePending,
        release("rel_2", stack(&bundle, "test", "b.acme.dev", json!({}))),
    )
    .await;
    let updated = run_until(
        &state,
        &[DeploymentStatus::Running, DeploymentStatus::UpdateFailed],
    )
    .await;
    assert_eq!(
        updated.status,
        DeploymentStatus::Running,
        "{:#?}",
        updated.error
    );
    assert_eq!(zone_outputs(&updated)["domain"], "b.acme.dev");
    assert_eq!(
        process_ids(&state),
        pids,
        "the process outlives a pass of the deployment loop"
    );

    let installed = updated.current_release.clone().unwrap();
    stage(&state, DeploymentStatus::DeletePending, installed).await;
    let deleted = run_until(
        &state,
        &[DeploymentStatus::Deleted, DeploymentStatus::DeleteFailed],
    )
    .await;
    assert_eq!(
        deleted.status,
        DeploymentStatus::Deleted,
        "{:#?}",
        deleted.error
    );

    let cloud = MockCloud::at(cloud.path());
    assert_eq!(cloud.calls("create").len(), 1, "{:?}", cloud.state().calls);
    assert_eq!(cloud.calls("update").len(), 1, "{:?}", cloud.state().calls);
    let zones = cloud.state().zones;
    assert_eq!(zones.len(), 1);
    assert!(zones.values().all(|zone| zone.deleted));
}

/// An Operator can run on a machine its release's bundle has no controller for, as when it moves
/// to a node of another architecture. An update to a release whose bundle has one still runs: the
/// release it replaces needs its controller only to delete what the update removes. The release
/// the deployment runs still refuses to load without one.
#[tokio::test(flavor = "multi_thread")]
async fn an_update_runs_when_the_release_it_replaces_has_no_controller_for_this_machine() {
    let vault = tempfile::tempdir().unwrap();
    std::env::set_var("TEST_VAULT_DATA_DIR", vault.path());
    let data_dir = tempfile::tempdir().unwrap();
    let cloud = tempfile::tempdir().unwrap();
    let bundle = bundle(&[("MOCK_CLOUD_DIR", cloud.path().display().to_string())]);
    cache_bundle(data_dir.path(), &bundle);
    let elsewhere = [
        alien_core::BinaryTarget::LinuxX64,
        alien_core::BinaryTarget::LinuxArm64,
    ]
    .into_iter()
    .find(|target| Some(*target) != host_target())
    .unwrap();
    let accepted_elsewhere = TestBundle::new("acme", "0.9.0", &[(elsewhere, b"controller")]);
    cache_bundle(data_dir.path(), &accepted_elsewhere);
    let db = OperatorDb::new(data_dir.path().to_str().unwrap(), KEY)
        .await
        .unwrap();
    db.set_deployment_id("dep_pull").await.unwrap();
    db.set_deployment_state(&initial_setup(
        Platform::Test,
        stack(&bundle, "test", "a.acme.dev", json!({})),
        StackState::new(Platform::Test),
        InitialSetupAuthority::DirectSetup,
    ))
    .await
    .unwrap();
    db.set_deployment_config(&config()).await.unwrap();
    let state = OperatorState {
        config: OperatorConfig::builder()
            .platform(Platform::Test)
            .data_dir(data_dir.path().to_str().unwrap())
            .encryption_key(KEY)
            .build(),
        db: Arc::new(db),
        service_provider: None,
        extension_controllers: Default::default(),
        operations_sync_handler: None,
        pod_log_collection: false,
        cancel: CancellationToken::new(),
    };
    let created = run_until(
        &state,
        &[
            DeploymentStatus::Running,
            DeploymentStatus::ProvisioningFailed,
        ],
    )
    .await;
    assert_eq!(
        created.status,
        DeploymentStatus::Running,
        "{:#?}",
        created.error
    );
    let mut running = created;
    running.current_release = Some(release(
        "rel_1",
        stack(&accepted_elsewhere, "test", "a.acme.dev", json!({})),
    ));
    state.db.set_deployment_state(&running).await.unwrap();

    stage(
        &state,
        DeploymentStatus::UpdatePending,
        release("rel_2", stack(&bundle, "test", "b.acme.dev", json!({}))),
    )
    .await;
    let updated = run_until(
        &state,
        &[DeploymentStatus::Running, DeploymentStatus::UpdateFailed],
    )
    .await;

    assert_eq!(
        updated.status,
        DeploymentStatus::Running,
        "{:#?}",
        updated.error
    );
    assert_eq!(zone_outputs(&updated)["domain"], "b.acme.dev");
    let error = provider_for(
        None,
        &cache(data_dir.path().to_str().unwrap()),
        &running,
        "dep_pull",
        &alien_core::ClientConfig::Test,
        &state.extension_controllers,
    )
    .await
    .err()
    .expect("the release it runs needs this machine's controller");
    assert!(
        error.message.contains("has no controller for"),
        "{}",
        error.message
    );
}

/// What the mock STS saw: each call's action and the access key that signed it.
type Calls = Arc<std::sync::Mutex<Vec<(String, String, String)>>>;

const ASSUME_ROLE_RESPONSE: &str = r#"<AssumeRoleResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <AssumeRoleResult>
    <AssumedRoleUser>
      <Arn>arn:aws:sts::210987654321:assumed-role/test-acme-ext/alien-extension-acme</Arn>
      <AssumedRoleId>AROA:alien-extension-acme</AssumedRoleId>
    </AssumedRoleUser>
    <Credentials>
      <AccessKeyId>ASIAEXTENSION</AccessKeyId>
      <SecretAccessKey>extension-secret</SecretAccessKey>
      <SessionToken>extension-session</SessionToken>
      <Expiration>2099-01-01T00:00:00Z</Expiration>
    </Credentials>
  </AssumeRoleResult>
  <ResponseMetadata><RequestId>request-1</RequestId></ResponseMetadata>
</AssumeRoleResponse>"#;

const CALLER_IDENTITY_RESPONSE: &str = r#"<GetCallerIdentityResponse xmlns="https://sts.amazonaws.com/doc/2011-06-15/">
  <GetCallerIdentityResult>
    <Arn>arn:aws:sts::210987654321:assumed-role/test-acme-ext/alien-extension-acme</Arn>
    <UserId>AROA:alien-extension-acme</UserId>
    <Account>210987654321</Account>
  </GetCallerIdentityResult>
  <ResponseMetadata><RequestId>request-2</RequestId></ResponseMetadata>
</GetCallerIdentityResponse>"#;

async fn sts(State(calls): State<Calls>, headers: HeaderMap, body: String) -> String {
    let field = |name: &str| {
        body.split('&')
            .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
            .map(|value| urlencoding::decode(value).unwrap().into_owned())
            .unwrap_or_default()
    };
    let signer = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split("Credential=").nth(1))
        .and_then(|rest| rest.split('/').next())
        .unwrap_or_default()
        .to_string();
    let action = field("Action");
    calls
        .lock()
        .unwrap()
        .push((action.clone(), signer, field("RoleArn")));
    match action.as_str() {
        "AssumeRole" => ASSUME_ROLE_RESPONSE.to_string(),
        // The pod's own token, exchanged for the identity its service account is bound to.
        "AssumeRoleWithWebIdentity" if field("WebIdentityToken") == POD_TOKEN => {
            ASSUME_ROLE_RESPONSE.replace("AssumeRole", "AssumeRoleWithWebIdentity")
        }
        "AssumeRoleWithWebIdentity" => panic!("an unknown web identity token: {body}"),
        _ => CALLER_IDENTITY_RESPONSE.to_string(),
    }
}

/// A mock STS on a free port.
async fn mock_sts() -> (String, Calls) {
    let calls = Calls::default();
    let app = Router::new()
        .route("/", post(sts))
        .with_state(calls.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, calls)
}

/// The Operator's own client config: its pod's identity, with STS at `sts`.
fn operator_identity(sts: &str) -> ClientConfig {
    ClientConfig::Aws(Box::new(AwsClientConfig {
        account_id: "210987654321".to_string(),
        region: "us-east-2".to_string(),
        credentials: AwsCredentials::AccessKeys {
            access_key_id: "AKIAOPERATOR".to_string(),
            secret_access_key: "operator-secret".to_string(),
            session_token: None,
        },
        service_overrides: Some(AwsServiceOverrides {
            endpoints: [("sts".to_string(), sts.to_string())].into(),
        }),
    }))
}

/// A pull deployment on AWS whose identity for the acme extension exists from setup, with the
/// zone still to create.
fn aws_deployment(bundle: &TestBundle, identity: bool) -> DeploymentState {
    let mut resources = json!({});
    let mut stack_state = StackState::new(Platform::Aws);
    if identity {
        let account = alien_core::ServiceAccount::new("acme-ext".to_string())
            .extension("acme".to_string())
            .build();
        resources = json!({
            "acme-ext": {
                "config": serde_json::to_value(alien_core::Resource::new(account.clone())).unwrap(),
                "lifecycle": "frozen",
                "dependencies": []
            }
        });
        stack_state.resources.insert(
            "acme-ext".to_string(),
            alien_core::StackResourceState::builder()
                .resource_type(alien_core::ServiceAccount::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Running)
                .config(alien_core::Resource::new(account))
                .outputs(alien_core::ResourceOutputs::new(
                    alien_core::ServiceAccountOutputs {
                        identity: IDENTITY_ROLE.to_string(),
                        resource_id: "test-acme-ext".to_string(),
                    },
                ))
                .lifecycle(alien_core::ResourceLifecycle::Frozen)
                .dependencies(vec![])
                .build(),
        );
    }
    initial_setup(
        Platform::Aws,
        stack(bundle, "aws", "a.acme.dev", resources),
        stack_state,
        InitialSetupAuthority::ImportedHandoff,
    )
}

/// The cluster of the tests below: each pod is a controller's host served in this process,
/// started with the arguments and the environment the Operator gives the real pod.
#[derive(Debug, Default)]
struct LocalCluster {
    /// The service accounts the chart installed.
    accounts: Vec<String>,
    /// What the cluster adds to the environment of a pod whose service account is bound to a
    /// cloud identity, as the EKS webhook does.
    bound_env: HashMap<String, String>,
    running: tokio::sync::Mutex<HashMap<String, RunningPod>>,
}

#[derive(Debug)]
struct RunningPod {
    pod: ControllerPod,
    url: String,
    host: tokio::task::JoinHandle<()>,
    _dir: tempfile::TempDir,
}

impl LocalCluster {
    async fn pods(&self) -> Vec<ControllerPod> {
        let mut pods: Vec<ControllerPod> = self
            .running
            .lock()
            .await
            .values()
            .map(|running| running.pod.clone())
            .collect();
        pods.sort_by_key(ControllerPod::name);
        pods
    }

    async fn url(&self, name: &str) -> String {
        self.running.lock().await[name].url.clone()
    }
}

#[async_trait::async_trait]
impl Cluster for LocalCluster {
    async fn has_service_account(&self, name: &str) -> Result<bool> {
        Ok(self.accounts.iter().any(|account| account == name))
    }

    async fn run(&self, pods: &[ControllerPod]) -> Result<Vec<String>> {
        let mut running = self.running.lock().await;
        running.retain(|name, pod| {
            let wanted = pods.iter().any(|pod| &pod.name() == name);
            if !wanted {
                pod.host.abort();
            }
            wanted
        });
        let mut urls = Vec::new();
        for pod in pods {
            if let Some(same) = running
                .get(&pod.name())
                .filter(|running| &running.pod == pod)
            {
                urls.push(same.url.clone());
                continue;
            }
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let listen = listener.local_addr().unwrap().to_string();
            let url = format!("http://{listen}");
            let dir = tempfile::tempdir().unwrap();
            let mut args = pod.host_args(&listen, &url);
            args.extend(["--dir".to_string(), dir.path().display().to_string()]);
            let mut env: HashMap<String, String> = pod.env.clone().into_iter().collect();
            if pod.identity.is_some() {
                env.extend(self.bound_env.clone());
            }
            let args = <crate::controller_host::HostArgs as clap::Parser>::parse_from(args);
            let host = tokio::spawn(async move {
                crate::controller_host::serve(listener, args, env)
                    .await
                    .unwrap()
            });
            if let Some(replaced) = running.insert(
                pod.name(),
                RunningPod {
                    pod: pod.clone(),
                    url: url.clone(),
                    host,
                    _dir: dir,
                },
            ) {
                replaced.host.abort();
            }
            urls.push(url);
        }
        Ok(urls)
    }
}

/// An Operator on Kubernetes whose pods run in `cluster`, with a new step-token key.
fn pods_in(cluster: &Arc<LocalCluster>) -> Controllers {
    use base64::Engine as _;
    let key = base64::engine::general_purpose::STANDARD
        .encode(ed25519_compact::KeyPair::generate().as_ref());
    Controllers::Pods(OperatorPods::of(
        ControllerPods::new(cluster.clone(), "test".to_string()),
        key,
    ))
}

/// The Operator's client config on EKS: the cluster, and its own identity in the base cloud.
fn eks_operator(sts: &str) -> ClientConfig {
    ClientConfig::KubernetesCloud {
        kubernetes: Box::new(alien_core::KubernetesClientConfig::InCluster {
            namespace: Some("acme".to_string()),
            additional_headers: None,
        }),
        cloud: Box::new(operator_identity(sts)),
    }
}

/// A pull deployment on EKS with the acme extension's zone still to create.
fn eks_deployment(bundle: &TestBundle, identity: bool) -> (DeploymentState, DeploymentConfig) {
    let mut deployment = aws_deployment(bundle, identity);
    deployment.platform = Platform::Kubernetes;
    deployment.stack_state.as_mut().unwrap().platform = Platform::Kubernetes;
    let mut config = config();
    config.base_platform = Some(Platform::Aws);
    (deployment, config)
}

/// Steps `state` on EKS with `provider` until it is Running or has failed.
async fn step_on_eks(
    mut state: DeploymentState,
    config: &DeploymentConfig,
    operator: &ClientConfig,
    provider: &Arc<dyn PlatformServiceProvider>,
) -> DeploymentState {
    alien_deployment::set_ephemeral_secrets_digest_key();
    for _ in 0..40 {
        if matches!(
            state.status,
            DeploymentStatus::Running | DeploymentStatus::ProvisioningFailed
        ) {
            break;
        }
        state = alien_deployment::step(
            state,
            config.clone(),
            operator.clone(),
            Some(provider.clone()),
        )
        .await
        .unwrap_or_else(|e| panic!("step failed: {e:#?}"))
        .state;
    }
    state
}

/// On EKS a controller runs in a pod of its own, whose service account is the one bound to the
/// extension's identity. Its cloud calls are signed with credentials the pod got for that
/// identity from its own token. The Operator's identity mints nothing and signs nothing.
#[tokio::test(flavor = "multi_thread")]
async fn on_eks_the_controller_runs_in_its_own_pod_as_the_extension_s_identity() {
    let data_dir = tempfile::tempdir().unwrap();
    let cloud = tempfile::tempdir().unwrap();
    let (sts_url, calls) = mock_sts().await;
    let token = data_dir.path().join("pod-token");
    std::fs::write(&token, POD_TOKEN).unwrap();
    let bundle = bundle(&[
        ("MOCK_CLOUD_DIR", cloud.path().display().to_string()),
        ("CALLER_IDENTITY_URL", sts_url.clone()),
    ]);
    cache_bundle(data_dir.path(), &bundle);
    let cluster = Arc::new(LocalCluster {
        accounts: vec!["test-acme-ext-sa".to_string()],
        bound_env: HashMap::from([
            ("AWS_ROLE_ARN".to_string(), IDENTITY_ROLE.to_string()),
            (
                "AWS_WEB_IDENTITY_TOKEN_FILE".to_string(),
                token.display().to_string(),
            ),
            (
                "AWS_SERVICE_OVERRIDES_ENDPOINTS".to_string(),
                json!({ "sts": sts_url }).to_string(),
            ),
        ]),
        ..Default::default()
    });
    let operator = eks_operator(&sts_url);
    let (deployment, eks_config) = eks_deployment(&bundle, true);
    let provider = provider_for(
        None,
        &cache(data_dir.path().to_str().unwrap()),
        &deployment,
        "dep_pull",
        &operator,
        &pods_in(&cluster),
    )
    .await
    .unwrap()
    .unwrap();

    let state = step_on_eks(deployment, &eks_config, &operator, &provider).await;

    assert_eq!(state.status, DeploymentStatus::Running, "{:#?}", state);
    let pods = cluster.pods().await;
    assert_eq!(pods.len(), 1, "{pods:?}");
    assert_eq!(
        pods[0].identity,
        Some(("test-acme-ext-sa".to_string(), Platform::Aws))
    );
    assert_eq!(
        pods[0].env,
        [
            ("AWS_ACCOUNT_ID", "210987654321"),
            ("AWS_REGION", "us-east-2")
        ]
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .into()
    );
    let calls = calls.lock().unwrap().clone();
    assert!(
        calls.iter().all(|(_, signer, _)| signer != "AKIAOPERATOR"),
        "the Operator's own identity signed a call: {calls:?}"
    );
    assert!(
        !calls.iter().any(|(action, _, _)| action == "AssumeRole"),
        "nothing mints the identity's credentials from another identity: {calls:?}"
    );
    let exchanged: Vec<&str> = calls
        .iter()
        .filter(|(action, _, _)| action == "AssumeRoleWithWebIdentity")
        .map(|(_, _, role)| role.as_str())
        .collect();
    assert!(!exchanged.is_empty(), "{calls:?}");
    assert!(
        exchanged.iter().all(|role| *role == IDENTITY_ROLE),
        "{calls:?}"
    );
    let controller: Vec<&str> = calls
        .iter()
        .filter(|(action, _, _)| action == "GetCallerIdentity")
        .map(|(_, signer, _)| signer.as_str())
        .collect();
    assert!(!controller.is_empty(), "{calls:?}");
    assert!(
        controller.iter().all(|signer| *signer == "ASIAEXTENSION"),
        "every call the controller made is signed as the extension's identity: {calls:?}"
    );
    assert_eq!(MockCloud::at(cloud.path()).calls("create").len(), 1);
}

/// A pod whose service account the chart did not install runs as no identity. Its controller
/// gets no credentials: the step is refused, and nothing reaches the cloud as anyone.
#[tokio::test(flavor = "multi_thread")]
async fn a_controller_in_a_pod_with_no_identity_gets_no_credentials() {
    let data_dir = tempfile::tempdir().unwrap();
    let cloud = tempfile::tempdir().unwrap();
    let (sts_url, calls) = mock_sts().await;
    let bundle = bundle(&[
        ("MOCK_CLOUD_DIR", cloud.path().display().to_string()),
        ("CALLER_IDENTITY_URL", sts_url.clone()),
    ]);
    cache_bundle(data_dir.path(), &bundle);
    let cluster = Arc::new(LocalCluster::default());
    let operator = eks_operator(&sts_url);
    let (deployment, eks_config) = eks_deployment(&bundle, false);
    let provider = provider_for(
        None,
        &cache(data_dir.path().to_str().unwrap()),
        &deployment,
        "dep_pull",
        &operator,
        &pods_in(&cluster),
    )
    .await
    .unwrap()
    .unwrap();

    let failed = step_on_eks(deployment, &eks_config, &operator, &provider).await;

    assert_eq!(failed.status, DeploymentStatus::ProvisioningFailed);
    let pods = cluster.pods().await;
    assert_eq!(pods[0].identity, None);
    assert!(pods[0].env.is_empty(), "{:?}", pods[0].env);
    let zone = &failed.stack_state.as_ref().unwrap().resources["zone"];
    let error = format!("{:?}", zone.error);
    assert!(
        error.contains("the controller's pod has no identity of its own: AWS_ROLE_ARN is not set"),
        "{error}"
    );
    assert!(
        calls.lock().unwrap().is_empty(),
        "{:?}",
        calls.lock().unwrap()
    );
    assert!(MockCloud::at(cloud.path()).state().calls.is_empty());
}

/// A pod's host takes a step only from the Operator that created the pod. The same step signed
/// with another key, as another Operator or anything else in the cluster would sign it, is
/// refused before the controller sees it.
#[tokio::test(flavor = "multi_thread")]
async fn a_pod_refuses_a_step_this_operator_did_not_sign() {
    use alien_infra::step_protocol::{HttpTransport, TokenSigner, Transport as _};

    let data_dir = tempfile::tempdir().unwrap();
    let cloud = tempfile::tempdir().unwrap();
    let bundle = bundle(&[("MOCK_CLOUD_DIR", cloud.path().display().to_string())]);
    cache_bundle(data_dir.path(), &bundle);
    let cluster = Arc::new(LocalCluster::default());
    let deployment = initial_setup(
        Platform::Kubernetes,
        stack(&bundle, "local", "a.acme.dev", json!({})),
        StackState::new(Platform::Kubernetes),
        InitialSetupAuthority::DirectSetup,
    );
    provider_for(
        None,
        &cache(data_dir.path().to_str().unwrap()),
        &deployment,
        "dep_pull",
        &ClientConfig::Test,
        &pods_in(&cluster),
    )
    .await
    .unwrap();
    let pod = cluster.pods().await.remove(0);
    let url = cluster.url(&pod.name()).await;
    // The pod holds its controller, so only the token stands between a step and the controller.
    let loaded = reqwest::Client::new()
        .put(format!("{url}/v1/controller"))
        .body(bundle.controllers.values().next().unwrap().clone())
        .send()
        .await
        .unwrap();
    assert_eq!(loaded.status(), 204);
    let request: StepRequest = serde_json::from_value(json!({
        "protocol": 1, "stepId": "s1", "deploymentId": "dep_pull", "resourceId": "zone",
        "resourceType": DNS_ZONE, "platform": "local", "resourcePrefix": "p",
        "initialSetupAuthority": "directSetup", "lifecycle": "live",
        "resource": { "type": DNS_ZONE, "id": "zone", "domain": "a.acme.dev" },
        "previousResource": null, "dependencies": {}, "deploymentConfig": config(),
        "setupRun": false, "setupRunId": null, "state": null, "transitions": [],
        "credentials": null, "deadline": (chrono::Utc::now() + chrono::Duration::seconds(30)).to_rfc3339()
    }))
    .unwrap();
    let signed_by = |issuer: String, key: ed25519_compact::KeyPair| {
        HttpTransport::to_own_workload(
            url.clone(),
            TokenSigner {
                issuer,
                project: String::new(),
                secret_key: key.sk,
            },
        )
    };

    let another_key = signed_by(pod.issuer(), ed25519_compact::KeyPair::generate())
        .step(&request, std::time::Duration::from_secs(30))
        .await
        .unwrap_err();
    assert_eq!(
        another_key.code, "EXTENSION_AUTH_REJECTED",
        "{another_key:?}"
    );
    assert!(
        another_key.message.contains("token signature is not valid"),
        "{another_key:?}"
    );
    let unsigned = reqwest::Client::new()
        .post(format!("{url}/v1/step"))
        .json(&request)
        .send()
        .await
        .unwrap();
    assert_eq!(unsigned.status(), 401);
    assert!(
        MockCloud::at(cloud.path()).state().calls.is_empty(),
        "the controller never saw either step"
    );
}

/// A pod runs only the binary with the sha256 it was created with. Other bytes are refused
/// whoever sends them, no controller starts, and a step is answered as not ready.
#[tokio::test(flavor = "multi_thread")]
async fn a_pod_never_runs_a_binary_with_another_digest() {
    let data_dir = tempfile::tempdir().unwrap();
    let marker = data_dir.path().join("ran");
    let cluster = Arc::new(LocalCluster::default());
    let pod = ControllerPod {
        extension: "acme".to_string(),
        sha256: format!(
            "{:x}",
            <sha2::Sha256 as sha2::Digest>::digest(b"the pinned binary")
        ),
        deployment_id: "dep_pull".to_string(),
        identity: None,
        env: Default::default(),
        trusted_key: {
            use base64::Engine as _;
            base64::engine::general_purpose::STANDARD
                .encode(ed25519_compact::KeyPair::generate().pk.as_ref())
        },
    };
    let url = cluster
        .run(std::slice::from_ref(&pod))
        .await
        .unwrap()
        .remove(0);
    let http = reqwest::Client::new();
    let controller = format!("{url}/v1/controller");

    let swapped = format!("#!/bin/sh\ntouch '{}'\n", marker.display());
    let refused = http.put(&controller).body(swapped).send().await.unwrap();

    assert_eq!(refused.status(), 400);
    let reason = refused.text().await.unwrap();
    assert!(reason.contains("not the pinned"), "{reason}");
    assert_eq!(http.get(&controller).send().await.unwrap().status(), 404);
    let step = http
        .post(format!("{url}/v1/step"))
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_ne!(step.status(), 200);
    assert!(!marker.exists(), "the swapped binary ran");
}

/// One pod per controller binary the deployment's releases pin: a release that pins another
/// binary replaces the pod, and a deployment whose releases list no extension keeps none.
#[tokio::test(flavor = "multi_thread")]
async fn a_pod_follows_the_binaries_the_releases_pin() {
    let data_dir = tempfile::tempdir().unwrap();
    let v1 = bundle(&[("MOCK_CLOUD_DIR", "/v1".to_string())]);
    let v2 = bundle(&[("MOCK_CLOUD_DIR", "/v2".to_string())]);
    cache_bundle(data_dir.path(), &v1);
    cache_bundle(data_dir.path(), &v2);
    let cluster = Arc::new(LocalCluster::default());
    let controllers = pods_in(&cluster);
    let cache = cache(data_dir.path().to_str().unwrap());
    let running = |bundle: Option<&TestBundle>| {
        let mut deployment = initial_setup(
            Platform::Kubernetes,
            match bundle {
                Some(bundle) => stack(bundle, "local", "a.acme.dev", json!({})),
                None => alien_core::Stack::new("acme".to_string()).build(),
            },
            StackState::new(Platform::Kubernetes),
            InitialSetupAuthority::DirectSetup,
        );
        deployment.current_release = deployment.target_release.clone();
        deployment
    };
    let sha256 = |bundle: &TestBundle| {
        format!(
            "{:x}",
            <sha2::Sha256 as sha2::Digest>::digest(bundle.controllers.values().next().unwrap())
        )
    };
    let pinned = |pods: Vec<ControllerPod>| -> Vec<String> {
        pods.into_iter().map(|pod| pod.sha256).collect()
    };

    for (release, expected) in [
        (Some(&v1), vec![sha256(&v1)]),
        (Some(&v2), vec![sha256(&v2)]),
        (None, vec![]),
    ] {
        provider_for(
            None,
            &cache,
            &running(release),
            "dep_pull",
            &ClientConfig::Test,
            &controllers,
        )
        .await
        .unwrap();
        assert_eq!(pinned(cluster.pods().await), expected);
    }
}

/// An Operator that runs as a process on a cloud holds that cloud identity's credentials, so it
/// runs no controller beside itself: a release that lists one is refused, naming the extension.
#[tokio::test]
async fn an_operator_that_is_a_process_on_a_cloud_runs_no_controller() {
    let data_dir = tempfile::tempdir().unwrap();
    let cloud = tempfile::tempdir().unwrap();
    let bundle = bundle(&[("MOCK_CLOUD_DIR", cloud.path().display().to_string())]);
    cache_bundle(data_dir.path(), &bundle);
    let db = Arc::new(
        OperatorDb::new(data_dir.path().to_str().unwrap(), KEY)
            .await
            .unwrap(),
    );
    let on = |platform| {
        Controllers::for_operator(
            &OperatorConfig::builder()
                .platform(platform)
                .encryption_key(KEY)
                .build(),
            db.clone(),
        )
    };
    assert!(matches!(on(Platform::Local), Controllers::Processes(_)));
    assert!(matches!(on(Platform::Kubernetes), Controllers::Pods(_)));

    for platform in [Platform::Aws, Platform::Gcp, Platform::Azure] {
        let error = provider_for(
            None,
            &cache(data_dir.path().to_str().unwrap()),
            &aws_deployment(&bundle, true),
            "dep_pull",
            &operator_identity("http://127.0.0.1:9"),
            &on(platform),
        )
        .await
        .err()
        .unwrap_or_else(|| panic!("an Operator on {platform} ran the controller"));
        assert!(
            error
                .message
                .contains("Extension 'acme': its controller can't run beside this Operator"),
            "{error}"
        );
    }
    assert!(MockCloud::at(cloud.path()).state().calls.is_empty());
}

/// A controller process whose binary leaves the deployment's releases is killed, so an upgrade
/// leaves no process of the old release running.
#[tokio::test]
async fn a_process_whose_binary_leaves_the_releases_is_killed() {
    let cloud = tempfile::tempdir().unwrap();
    let data_dir = tempfile::tempdir().unwrap();
    let bundle = bundle(&[("MOCK_CLOUD_DIR", cloud.path().display().to_string())]);
    cache_bundle(data_dir.path(), &bundle);
    let binary = cache(data_dir.path().to_str().unwrap())
        .dir(&bundle.manifest)
        .unwrap()
        .join(format!("controller-{}", host_target().unwrap()));
    let processes = ControllerProcesses::default();
    let sha256 = bundle
        .controllers
        .values()
        .next()
        .map(|bytes| format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(bytes)));
    let transport = processes
        .keep(&[CachedBinary {
            extension: &bundle.manifest,
            folder: binary.parent().unwrap().to_path_buf(),
            path: binary,
            sha256: sha256.unwrap(),
        }])
        .await
        .unwrap()
        .remove(0);
    let request: StepRequest = serde_json::from_value(json!({
        "protocol": 1, "stepId": "s1", "deploymentId": "dep_pull", "resourceId": "zone",
        "resourceType": DNS_ZONE, "platform": "test", "resourcePrefix": "p",
        "initialSetupAuthority": "directSetup", "lifecycle": "live",
        "resource": { "type": DNS_ZONE, "id": "zone", "domain": "a.acme.dev" },
        "previousResource": null, "dependencies": {}, "deploymentConfig": config(),
        "setupRun": false, "setupRunId": null, "state": null, "transitions": [],
        "credentials": null, "deadline": (chrono::Utc::now() + chrono::Duration::seconds(30)).to_rfc3339()
    }))
    .unwrap();
    alien_infra::step_protocol::Transport::step(
        transport.as_ref(),
        &request,
        std::time::Duration::from_secs(30),
    )
    .await
    .unwrap();
    let pid = transport.child_pid().expect("the process runs");
    drop(transport);

    processes.keep(&[]).await.unwrap();

    for _ in 0..50 {
        let status = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        let status = String::from_utf8_lossy(&status.stdout);
        if status.trim().is_empty() || status.starts_with('Z') {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("process {pid} still runs");
}

/// The early return is the parity: a deployment whose releases list no extensions keeps the
/// Operator's provider and starts no process.
#[tokio::test]
async fn a_deployment_without_extensions_starts_no_controller_process() {
    let data_dir = tempfile::tempdir().unwrap();
    let deployment: DeploymentState = serde_json::from_value(json!({
        "platform": "aws", "status": "pending",
        "protocolVersion": alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
        "targetRelease": {"releaseId": "rel_1", "stack": {"id": "app", "resources": {}}}
    }))
    .unwrap();
    let operators: Arc<dyn PlatformServiceProvider> =
        Arc::new(alien_infra::DefaultPlatformServiceProvider::default());
    let controllers = Controllers::default();

    let provider = provider_for(
        Some(operators.clone()),
        &cache(data_dir.path().to_str().unwrap()),
        &deployment,
        "dep_pull",
        &operator_identity("http://127.0.0.1:9"),
        &controllers,
    )
    .await
    .unwrap()
    .unwrap();

    assert!(Arc::ptr_eq(&provider, &operators));
    let Controllers::Processes(processes) = &controllers else {
        panic!("the default runs controllers as processes");
    };
    assert!(processes.by_binary.try_lock().unwrap().is_empty());
    assert!(!data_dir.path().join(SYNCED_EXTENSIONS_DIR).exists());
}

/// A private copy an Operator left when it ended without removing it is removed at startup; the
/// copy a running controller process of this Operator uses is not.
#[tokio::test]
async fn startup_removes_the_private_copies_a_previous_operator_left() {
    let data_dir = tempfile::tempdir().unwrap();
    let cloud = tempfile::tempdir().unwrap();
    let bundle = bundle(&[("MOCK_CLOUD_DIR", cloud.path().display().to_string())]);
    cache_bundle(data_dir.path(), &bundle);
    let folder = cache(data_dir.path().to_str().unwrap())
        .dir(&bundle.manifest)
        .unwrap();
    let stale = folder.join(".run-x");
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::write(
        stale.join(format!("controller-{}", host_target().unwrap())),
        b"old",
    )
    .unwrap();
    let processes = ControllerProcesses::default();
    let binary = folder.join(format!("controller-{}", host_target().unwrap()));
    let sha256 = format!(
        "{:x}",
        <sha2::Sha256 as sha2::Digest>::digest(bundle.controllers.values().next().unwrap())
    );
    let live = processes
        .keep(&[CachedBinary {
            extension: &bundle.manifest,
            folder: folder.clone(),
            path: binary,
            sha256,
        }])
        .await
        .unwrap();
    let copies = || -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(&folder)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name.starts_with(".run-"))
            .collect();
        names.sort();
        names
    };
    assert_eq!(copies().len(), 2);

    remove_stale_private_copies(data_dir.path().to_str().unwrap()).await;

    let left = copies();
    assert_eq!(left.len(), 1, "{left:?}");
    assert_ne!(left[0], ".run-x");
    drop(live);
    drop(processes);
    assert!(copies().is_empty(), "the live copy goes with its transport");
}
