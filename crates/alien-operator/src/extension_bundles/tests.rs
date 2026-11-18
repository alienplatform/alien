//! A custom resource deployed by the Operator, stepped in the controller module of the verified
//! bundle in the Operator's cache: in the Operator's process, its cloud calls signed with
//! credentials minted for the extension's identity.

use super::*;
use alien_core::{
    AwsClientConfig, AwsCredentials, AwsServiceOverrides, DeploymentConfig, DeploymentStatus,
    EnvironmentVariablesSnapshot, InitialSetupAuthority, Platform, ReleaseInfo, ResourceStatus,
    RuntimeMetadata, StackSettings, StackState,
};
use alien_infra::extension_bundle::host_target;
use alien_infra::extension_bundle::testing::{controller_guest, TestBundle};
use axum::{extract::State, http::HeaderMap, routing::post, Router};
use serde_json::{json, Value};

const IDENTITY_ROLE: &str = "arn:aws:iam::210987654321:role/test-acme-ext";

/// The custom type of the test's controller module: each step runs the resource's `probe` with
/// its `arg` and reports the result as the resource's progress.
const PROBE: &str = "acme/probe";

/// The EC2 endpoint of the Operator's region, where the module's calls go.
const AWS_EC2: &str = "https://ec2.us-east-2.amazonaws.com";

/// The acme extension's bundle, whose controller is the test's controller module.
fn bundle() -> TestBundle {
    TestBundle::with_controller_module("acme", "1.0.0", controller_guest(), &[])
}

/// Writes `bundle`'s core zip into the cache under `data_dir`, as the sync loop leaves a
/// verified download.
fn cache_bundle(data_dir: &std::path::Path, bundle: &TestBundle) {
    let dir = cache(data_dir.to_str().unwrap())
        .dir(&bundle.manifest)
        .unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("core.zip"), &bundle.zip).unwrap();
}

/// A probe that returns the step it was sent, with `arg`.
fn echo(arg: &str) -> Value {
    json!({ "probe": "echo", "arg": arg })
}

/// A probe that calls EC2 once in each step.
fn describe_regions() -> Value {
    json!({
        "probe": "cloud",
        "arg": {
            "cloud": "aws", "service": "ec2", "method": "POST", "path": "/",
            "body": "Action=DescribeRegions&Version=2016-11-15"
        }
    })
}

/// A stack with the zone, a resource of the type the bundle's extension defines with a
/// controller on `platform`, that runs `probe`, and `resources` beside it.
fn stack(bundle: &TestBundle, platform: &str, probe: Value, resources: Value) -> alien_core::Stack {
    let mut manifest = serde_json::to_value(&bundle.manifest).unwrap();
    manifest["resourceTypes"] = json!({
        PROBE: {
            "lifecycles": ["live"],
            "holdsData": false,
            "setup": { "cloudformation": [], "terraform": [], "helm": [], "importers": [] },
            "controllerPlatforms": [platform],
            "permissionSets": [],
            "link": { "permissionSets": [] },
            "schemas": { "config": {}, "outputs": {} }
        }
    });
    let mut zone = json!({ "type": PROBE, "id": "zone" });
    zone.as_object_mut()
        .unwrap()
        .extend(probe.as_object().unwrap().clone());
    let mut all = json!({
        "zone": { "config": zone, "lifecycle": "live", "dependencies": [] }
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
        signed: None,
        signer_lists: Vec::new(),
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

/// What the module reported as the zone's progress in its last step.
fn zone_progress(state: &DeploymentState) -> String {
    let zone = &state.stack_state.as_ref().unwrap().resources["zone"];
    zone.internal_state.as_ref().expect("the zone was stepped")["remote"]["progress"]
        .as_str()
        .expect("the module reported its progress")
        .to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_extension_without_a_controller_module_has_no_controller() {
    alien_deployment::set_ephemeral_secrets_digest_key();
    let data_dir = tempfile::tempdir().unwrap();
    let target = host_target().expect("tests run on a machine with a controller target");
    let bundle = TestBundle::new("acme", "1.0.0", &[(target, b"controller")]);
    cache_bundle(data_dir.path(), &bundle);
    let mut state = initial_setup(
        Platform::Test,
        stack(&bundle, "test", echo("a"), json!({})),
        StackState::new(Platform::Test),
        InitialSetupAuthority::DirectSetup,
    );
    let provider = provider_for(
        None,
        &cache(data_dir.path().to_str().unwrap()),
        &state,
        "dep_pull",
        &ClientConfig::Test,
    )
    .await
    .unwrap()
    .expect("the release lists an extension");

    let mut refusal = None;
    for _ in 0..40 {
        match alien_deployment::step(state, config(), ClientConfig::Test, Some(provider.clone()))
            .await
        {
            Ok(stepped) => state = stepped.state,
            Err(error) => {
                refusal = Some(format!("{error:?}"));
                break;
            }
        }
    }

    let refusal = refusal.expect("a deployment with no controller for its custom type stepped on");
    assert!(
        refusal.contains(
            "The controller of extension 'acme' for acme/probe on test can't run: this process has not loaded the extension's controller"
        ),
        "{refusal}"
    );
}

/// What the mock AWS saw: each call's action, the access key that signed it, and the role it
/// names.
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

const DESCRIBE_REGIONS_RESPONSE: &str = r#"<DescribeRegionsResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
  <requestId>request-2</requestId>
  <regionInfo>
    <item><regionName>us-east-2</regionName><regionEndpoint>ec2.us-east-2.amazonaws.com</regionEndpoint></item>
  </regionInfo>
</DescribeRegionsResponse>"#;

/// STS for the Operator's own minting, and EC2 for the module's calls.
async fn aws(State(calls): State<Calls>, headers: HeaderMap, body: String) -> String {
    let field = |name: &str| {
        body.split('&')
            .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
            .map(|value| urlencoding::decode(value).unwrap().into_owned())
            .unwrap_or_default()
    };
    let signer = headers["authorization"]
        .to_str()
        .unwrap()
        .split("Credential=")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .unwrap()
        .to_string();
    let action = field("Action");
    calls
        .lock()
        .unwrap()
        .push((action.clone(), signer, field("RoleArn")));
    match action.as_str() {
        "AssumeRole" => ASSUME_ROLE_RESPONSE.to_string(),
        _ => DESCRIBE_REGIONS_RESPONSE.to_string(),
    }
}

/// A mock of STS and EC2 on a free port.
async fn mock_aws() -> (String, Calls) {
    let calls = Calls::default();
    let app = Router::new()
        .route("/", post(aws))
        .with_state(calls.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, calls)
}

/// The signer of each `action` call in `calls`, with the role the call names.
fn calls_of(calls: &Calls, action: &str) -> Vec<(String, String)> {
    calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(called, _, _)| called == action)
        .map(|(_, signer, role)| (signer.clone(), role.clone()))
        .collect()
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

/// A host for controller modules whose calls to EC2 go to the mock at `ec2`.
fn module_host(ec2: &str) -> Arc<WasmHost> {
    Arc::new(WasmHost::new().unwrap().routing(AWS_EC2, ec2))
}

/// Whether a deployment has the identity setup creates for the acme extension.
#[derive(Clone, Copy, PartialEq)]
enum Identity {
    /// In the stack and set up, with outputs.
    SetUp,
    /// In the stack and the state, with no outputs yet.
    NotSetUp,
    /// Not in the stack at all, as for an extension whose types hold no permission sets.
    None,
}

/// A pull deployment on AWS with the zone still to create, whose module calls EC2 in each step.
fn aws_deployment(bundle: &TestBundle, identity: Identity) -> DeploymentState {
    let mut resources = json!({});
    let mut stack_state = StackState::with_resource_prefix(Platform::Aws, "test".to_string());
    if identity != Identity::None {
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
    }
    if identity != Identity::None {
        let account = alien_core::ServiceAccount::new("acme-ext".to_string())
            .extension("acme".to_string())
            .build();
        stack_state.resources.insert(
            "acme-ext".to_string(),
            alien_core::StackResourceState::builder()
                .resource_type(alien_core::ServiceAccount::RESOURCE_TYPE.to_string())
                .status(ResourceStatus::Running)
                .config(alien_core::Resource::new(account))
                .maybe_outputs((identity == Identity::SetUp).then(|| {
                    alien_core::ResourceOutputs::new(alien_core::ServiceAccountOutputs {
                        identity: IDENTITY_ROLE.to_string(),
                        resource_id: "test-acme-ext".to_string(),
                    })
                }))
                .lifecycle(alien_core::ResourceLifecycle::Frozen)
                .dependencies(vec![])
                .build(),
        );
    }
    initial_setup(
        Platform::Aws,
        stack(bundle, "aws", describe_regions(), resources),
        stack_state,
        InitialSetupAuthority::ImportedHandoff,
    )
}

/// Steps `state` with the Operator's provider until it reaches one of `statuses`.
async fn step_until(
    mut state: DeploymentState,
    config: DeploymentConfig,
    client_config: &ClientConfig,
    provider: &Arc<dyn PlatformServiceProvider>,
    statuses: &[DeploymentStatus],
) -> DeploymentState {
    alien_deployment::set_ephemeral_secrets_digest_key();
    for _ in 0..40 {
        if statuses.contains(&state.status) {
            return state;
        }
        state = alien_deployment::step(
            state,
            config.clone(),
            client_config.clone(),
            Some(provider.clone()),
        )
        .await
        .unwrap_or_else(|e| panic!("step failed: {e:#?}"))
        .state;
    }
    panic!(
        "never reached {statuses:?}; last {:?}: {:#?}",
        state.status, state.error
    );
}

/// The module runs in the Operator's process, but its cloud calls are signed with credentials
/// minted for the extension's identity, never with the Operator's own.
#[tokio::test(flavor = "multi_thread")]
async fn the_module_calls_the_cloud_with_credentials_minted_for_the_extension_identity() {
    let data_dir = tempfile::tempdir().unwrap();
    let (aws_url, calls) = mock_aws().await;
    let bundle = bundle();
    cache_bundle(data_dir.path(), &bundle);
    let operator = operator_identity(&aws_url);
    let deployment = aws_deployment(&bundle, Identity::SetUp);
    let provider = provider_on(
        &module_host(&aws_url),
        None,
        &cache(data_dir.path().to_str().unwrap()),
        &deployment,
        "dep_pull",
        &operator,
    )
    .await
    .unwrap();

    let running = step_until(
        deployment,
        config(),
        &operator,
        &provider,
        &[
            DeploymentStatus::Running,
            DeploymentStatus::ProvisioningFailed,
        ],
    )
    .await;

    assert_eq!(
        running.status,
        DeploymentStatus::Running,
        "{:#?}",
        running.error
    );
    let minted = calls_of(&calls, "AssumeRole");
    assert!(!minted.is_empty(), "{calls:?}");
    assert!(
        minted
            .iter()
            .all(|(signer, role)| signer == "AKIAOPERATOR" && role == IDENTITY_ROLE),
        "the Operator mints from its own identity, for the extension's: {calls:?}"
    );
    let module = calls_of(&calls, "DescribeRegions");
    assert!(!module.is_empty(), "{calls:?}");
    assert!(
        module.iter().all(|(signer, _)| signer == "ASIAEXTENSION"),
        "every call the module made is signed as the extension's identity: {calls:?}"
    );
    let progress = zone_progress(&running);
    assert!(progress.contains("DescribeRegionsResponse"), "{progress}");
}

/// With its identity in the stack but not set up yet, the module gets no credentials: the step
/// fails naming the extension, nothing is minted, and the module's call never reaches the cloud.
#[tokio::test(flavor = "multi_thread")]
async fn a_module_whose_identity_is_missing_gets_no_credentials() {
    let data_dir = tempfile::tempdir().unwrap();
    let (aws_url, calls) = mock_aws().await;
    let bundle = bundle();
    cache_bundle(data_dir.path(), &bundle);
    let operator = operator_identity(&aws_url);
    let deployment = aws_deployment(&bundle, Identity::NotSetUp);
    let provider = provider_on(
        &module_host(&aws_url),
        None,
        &cache(data_dir.path().to_str().unwrap()),
        &deployment,
        "dep_pull",
        &operator,
    )
    .await
    .unwrap();

    let failed = step_until(
        deployment,
        config(),
        &operator,
        &provider,
        &[
            DeploymentStatus::Running,
            DeploymentStatus::ProvisioningFailed,
        ],
    )
    .await;

    assert_eq!(failed.status, DeploymentStatus::ProvisioningFailed);
    let zone = &failed.stack_state.as_ref().unwrap().resources["zone"];
    let error = format!("{:?}", zone.error);
    assert!(
        error.contains("Extension 'acme': its identity 'acme-ext' has no outputs"),
        "{error}"
    );
    assert!(
        calls.lock().unwrap().is_empty(),
        "{:?}",
        calls.lock().unwrap()
    );
}

/// An extension the deployment has no identity for still runs its module, with no credentials:
/// its cloud call is refused, and the Operator mints nothing and lends it nothing of its own.
#[tokio::test(flavor = "multi_thread")]
async fn a_module_without_an_identity_steps_with_no_credentials() {
    let data_dir = tempfile::tempdir().unwrap();
    let (aws_url, calls) = mock_aws().await;
    let bundle = bundle();
    cache_bundle(data_dir.path(), &bundle);
    let operator = operator_identity(&aws_url);
    let deployment = aws_deployment(&bundle, Identity::None);
    let provider = provider_on(
        &module_host(&aws_url),
        None,
        &cache(data_dir.path().to_str().unwrap()),
        &deployment,
        "dep_pull",
        &operator,
    )
    .await
    .unwrap();

    let running = step_until(
        deployment,
        config(),
        &operator,
        &provider,
        &[
            DeploymentStatus::Running,
            DeploymentStatus::ProvisioningFailed,
        ],
    )
    .await;

    assert_eq!(
        running.status,
        DeploymentStatus::Running,
        "{:#?}",
        running.error
    );
    let progress = zone_progress(&running);
    assert!(progress.contains("refused: "), "{progress}");
    assert!(
        calls.lock().unwrap().is_empty(),
        "{:?}",
        calls.lock().unwrap()
    );
}

/// On Kubernetes on a cloud the Operator's client config is the cluster plus the cloud under
/// it. The credentials for the extension's identity are minted from the cloud's part, and the
/// module's cloud calls are signed with them.
#[tokio::test(flavor = "multi_thread")]
async fn on_kubernetes_on_a_cloud_the_modules_credentials_are_minted_from_the_cloud() {
    let data_dir = tempfile::tempdir().unwrap();
    let (aws_url, calls) = mock_aws().await;
    let bundle = bundle();
    cache_bundle(data_dir.path(), &bundle);
    let operator = ClientConfig::KubernetesCloud {
        kubernetes: Box::new(alien_core::KubernetesClientConfig::InCluster {
            namespace: Some("acme".to_string()),
            additional_headers: None,
        }),
        cloud: Box::new(operator_identity(&aws_url)),
    };
    let mut deployment = aws_deployment(&bundle, Identity::SetUp);
    deployment.platform = Platform::Kubernetes;
    deployment.stack_state.as_mut().unwrap().platform = Platform::Kubernetes;
    let mut on_aws = config();
    on_aws.base_platform = Some(Platform::Aws);
    let provider = provider_on(
        &module_host(&aws_url),
        None,
        &cache(data_dir.path().to_str().unwrap()),
        &deployment,
        "dep_pull",
        &operator,
    )
    .await
    .unwrap();

    let running = step_until(
        deployment,
        on_aws,
        &operator,
        &provider,
        &[
            DeploymentStatus::Running,
            DeploymentStatus::ProvisioningFailed,
        ],
    )
    .await;

    assert_eq!(
        running.status,
        DeploymentStatus::Running,
        "{:#?}",
        running.error
    );
    let minted = calls_of(&calls, "AssumeRole");
    assert!(!minted.is_empty(), "{calls:?}");
    assert!(
        minted
            .iter()
            .all(|(signer, role)| signer == "AKIAOPERATOR" && role == IDENTITY_ROLE),
        "{calls:?}"
    );
    let module = calls_of(&calls, "DescribeRegions");
    assert!(!module.is_empty(), "{calls:?}");
    assert!(
        module.iter().all(|(signer, _)| signer == "ASIAEXTENSION"),
        "{calls:?}"
    );
}
