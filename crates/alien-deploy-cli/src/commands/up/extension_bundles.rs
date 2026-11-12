//! The bundles of the extensions a deployment's releases list, as direct setup loads them: each
//! core zip and this machine's controller, downloaded from the manager by digest, verified, and
//! cached under `~/.alien/extensions/<name>@sha256-<hex>/`. Nothing in a bundle runs before its
//! check passes.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use alien_core::{ClientConfig, DeploymentState};
use alien_error::{AlienError, ContextError};
use alien_infra::extension_bundle::{manager_object_url, BundleCache, BundleSource, Runtime};
use alien_infra::step_protocol::{
    CredentialSource, ForwardedCredentials, ProxyFactory, StdioTransport,
};
use alien_infra::PlatformServiceProvider;

use crate::error::{ErrorData, Result};

/// The cache under `~/.alien/extensions/`, beside the downloaded Operator in `~/.alien/bin/`.
pub(crate) fn cache() -> Result<BundleCache> {
    let home: PathBuf = dirs::home_dir().ok_or_else(|| {
        AlienError::new(ErrorData::ConfigurationError {
            message: "Could not determine home directory".to_string(),
        })
    })?;
    Ok(BundleCache::new(home.join(".alien").join("extensions")))
}

/// `error` as the CLI's own, with its message and whether another try may pass.
fn refused<E>(error: AlienError<E>) -> AlienError<ErrorData>
where
    E: alien_error::AlienErrorData
        + Clone
        + std::fmt::Debug
        + serde::Serialize
        + Send
        + Sync
        + 'static,
{
    // A download that may pass on another try stays retryable under the CLI's error.
    let message = error.message.clone();
    let retryable = error.retryable;
    let mut wrapped = error.context(ErrorData::ConfigurationError { message });
    wrapped.retryable = retryable;
    wrapped
}

/// The hooks engine of this process. The hooks runtime expects one per process, shared by every
/// module it loads.
fn runtime() -> Result<&'static Runtime> {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    if let Some(runtime) = RUNTIME.get() {
        return Ok(runtime);
    }
    let runtime = Runtime::new().map_err(refused)?;
    Ok(RUNTIME.get_or_init(|| runtime))
}

/// The service provider for direct setup of deployment `deployment_id` in `state`. When its
/// releases list no extensions it is `provider`, unchanged; otherwise one with the same local
/// bindings, each extension's hooks module, and each extension's controller as a process on this
/// machine. Each bundle's core zip and this machine's controller are downloaded from the manager
/// at `manager_url` with `token` when the cache lacks them. The controller gets the admin's
/// credentials from `client_config` with each step, on its stdin. Fails, naming the extension, on
/// an object that fails its check or a bundle with no controller for this machine.
pub(crate) async fn provider_for(
    provider: Option<Arc<dyn PlatformServiceProvider>>,
    manager_url: &str,
    token: &str,
    state: &DeploymentState,
    deployment_id: &str,
    client_config: &ClientConfig,
) -> Result<Option<Arc<dyn PlatformServiceProvider>>> {
    if alien_infra::extension_bundle::release_extensions(state).is_empty() {
        return Ok(provider);
    }
    provider_in(
        provider,
        &cache()?,
        manager_url,
        token,
        state,
        deployment_id,
        client_config,
    )
    .await
}

/// The hooks module of each extension `stack` lists, from the manager at `manager_url`, for a
/// setup template the setup CLI renders itself. A stack without extensions downloads nothing.
pub(crate) async fn extensions_for(
    manager_url: &str,
    token: &str,
    stack: &alien_core::Stack,
) -> Result<Arc<alien_infra::LoadedExtensions>> {
    if stack.extensions.is_empty() {
        return Ok(Arc::default());
    }
    extensions_in(&cache()?, manager_url, token, stack).await
}

/// [`extensions_for`] with the bundles cached in `cache`.
pub(crate) async fn extensions_in(
    cache: &BundleCache,
    manager_url: &str,
    token: &str,
    stack: &alien_core::Stack,
) -> Result<Arc<alien_infra::LoadedExtensions>> {
    let client = reqwest::Client::new();
    let mut loaded = alien_infra::LoadedExtensions::default();
    for extension in &stack.extensions {
        let url = manager_object_url(manager_url, &extension.digest);
        let bundle = cache
            .core(
                extension,
                BundleSource {
                    client: &client,
                    url: &url,
                    bearer: Some(token),
                },
            )
            .await
            .map_err(refused)?;
        loaded = loaded.with_hooks(
            extension.name.clone(),
            bundle.load_hooks(runtime()?).map_err(refused)?,
        );
    }
    Ok(Arc::new(loaded))
}

/// [`provider_for`] with the bundles cached in `cache`.
pub(crate) async fn provider_in(
    provider: Option<Arc<dyn PlatformServiceProvider>>,
    cache: &BundleCache,
    manager_url: &str,
    token: &str,
    state: &DeploymentState,
    deployment_id: &str,
    client_config: &ClientConfig,
) -> Result<Option<Arc<dyn PlatformServiceProvider>>> {
    let extensions = alien_infra::extension_bundle::release_extensions(state);
    if extensions.is_empty() {
        return Ok(provider);
    }
    // The manager may redirect to a presigned URL on another host; the client drops the token
    // on such a redirect.
    let client = reqwest::Client::new();
    let object_url = |digest: &str| manager_object_url(manager_url, digest);
    let credentials = forwarded(client_config);
    let mut bundles = std::collections::HashMap::new();
    let mut controllers = std::collections::HashMap::new();
    for extension in extensions {
        let core_url = object_url(&extension.digest);
        let bundle = cache
            .core(
                extension,
                BundleSource {
                    client: &client,
                    url: &core_url,
                    bearer: Some(token),
                },
            )
            .await
            .map_err(refused)?;
        let pinned = bundle
            .host_controller()
            .map_err(refused)?
            .map(|(_, binary)| binary.sha256.to_ascii_lowercase());
        let controller_url = pinned
            .as_ref()
            .map(|sha256| object_url(&format!("sha256:{sha256}")));
        let binary = cache
            .controller(
                extension,
                &bundle,
                controller_url.as_deref().map(|url| BundleSource {
                    client: &client,
                    url,
                    bearer: Some(token),
                }),
            )
            .await
            .map_err(refused)?;
        if let (Some(sha256), Some(binary)) = (&pinned, binary) {
            let transport = StdioTransport::verified(
                &extension.name,
                &binary,
                sha256,
                &cache.dir(extension).map_err(refused)?,
                Vec::new(),
            )
            .await
            .map_err(refused)?;
            controllers.insert(
                extension.digest.clone(),
                ProxyFactory::new(deployment_id, Arc::new(transport), credentials.clone()),
            );
        }
        bundles.insert(extension.digest.clone(), Arc::new(bundle));
    }
    let base = match provider
        .as_ref()
        .and_then(|provider| provider.get_local_bindings_provider())
    {
        Some(local) => alien_infra::DefaultPlatformServiceProvider::with_local_bindings(local),
        None => alien_infra::DefaultPlatformServiceProvider::default(),
    };
    let provider = alien_infra::extension_bundle::provider_for(
        base,
        state,
        runtime()?,
        &bundles,
        &controllers,
    )
    .map_err(refused)?;
    Ok(Some(Arc::new(provider)))
}

/// The admin's credentials in `client_config`, which a controller on this machine gets with each
/// step, or `None` on a platform with no cloud credentials to forward.
fn forwarded(client_config: &ClientConfig) -> Option<Arc<dyn CredentialSource>> {
    matches!(
        client_config,
        ClientConfig::Aws(_) | ClientConfig::Gcp(_) | ClientConfig::Azure(_)
    )
    .then(|| {
        Arc::new(ForwardedCredentials::new(client_config.clone())) as Arc<dyn CredentialSource>
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::BinaryTarget;
    use alien_infra::extension_bundle::{host_target, testing::TestBundle};
    use httpmock::{Method::GET, MockServer};

    /// The CLI's error keeps whether another try may pass, as a download that broke off may.
    #[test]
    fn the_cli_error_keeps_whether_a_retry_may_pass() {
        for retryable in [true, false] {
            let mut inner = AlienError::new(ErrorData::ConfigurationError {
                message: "inner".to_string(),
            });
            inner.retryable = retryable;
            assert_eq!(refused(inner).retryable, retryable);
        }
    }

    fn host() -> BinaryTarget {
        host_target().expect("tests run on a machine with a controller target")
    }

    fn other_target() -> BinaryTarget {
        if host() == BinaryTarget::WindowsX64 {
            BinaryTarget::LinuxX64
        } else {
            BinaryTarget::WindowsX64
        }
    }

    fn setup_of(bundle: &TestBundle) -> DeploymentState {
        serde_json::from_value(serde_json::json!({
            "platform": "aws", "status": "pending",
            "protocolVersion": alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            "targetRelease": {"releaseId": "rel_1", "stack": {"id": "app", "resources": {}, "extensions": [bundle.manifest]}}
        }))
        .unwrap()
    }

    fn object_path(digest: &str) -> String {
        format!("/v1/extensions/bundles/{digest}")
    }

    fn binary_path(bytes: &[u8]) -> String {
        object_path(&format!(
            "sha256:{:x}",
            <sha2::Sha256 as sha2::Digest>::digest(bytes)
        ))
    }

    #[tokio::test]
    async fn downloads_the_core_zip_and_only_this_machines_controller() {
        let bundle = TestBundle::new(
            "acme",
            "1.0.0",
            &[
                (host(), &b"host controller"[..]),
                (other_target(), &b"other controller"[..]),
            ],
        );
        let server = MockServer::start_async().await;
        let core = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path(object_path(&bundle.manifest.digest))
                    .header("authorization", "Bearer dep-token");
                then.status(200).body(&bundle.zip);
            })
            .await;
        let own = server
            .mock_async(|when, then| {
                when.method(GET).path(binary_path(b"host controller"));
                then.status(200).body("host controller");
            })
            .await;
        let other = server
            .mock_async(|when, then| {
                when.method(GET).path(binary_path(b"other controller"));
                then.status(200).body("other controller");
            })
            .await;
        let root = tempfile::tempdir().unwrap();
        let cache = BundleCache::new(root.path());

        for _ in 0..2 {
            let provider = provider_in(
                None,
                &cache,
                &server.base_url(),
                "dep-token",
                &setup_of(&bundle),
                "dep_demo",
                &ClientConfig::Test,
            )
            .await
            .unwrap()
            .unwrap();
            assert!(provider.extensions().is_some());
        }

        core.assert_hits_async(1).await;
        own.assert_hits_async(1).await;
        other.assert_hits_async(0).await;
        let dir = cache.dir(&bundle.manifest).unwrap();
        assert!(dir.join("core.zip").exists());
        assert_eq!(
            std::fs::read(dir.join(format!("controller-{}", host()))).unwrap(),
            b"host controller"
        );
    }

    #[tokio::test]
    async fn refuses_a_tampered_object_and_keeps_nothing_of_it() {
        let bundle = TestBundle::new("acme", "1.0.0", &[(host(), &b"host controller"[..])]);
        let mut tampered_zip = bundle.zip.clone();
        *tampered_zip.last_mut().unwrap() ^= 1;
        for (core_body, controller_body, object) in [
            (
                tampered_zip,
                b"host controller".to_vec(),
                "core zip".to_string(),
            ),
            (
                bundle.zip.clone(),
                b"tampered".to_vec(),
                format!("controller for {}", host()),
            ),
        ] {
            let server = MockServer::start_async().await;
            server
                .mock_async(|when, then| {
                    when.method(GET).path(object_path(&bundle.manifest.digest));
                    then.status(200).body(&core_body);
                })
                .await;
            server
                .mock_async(|when, then| {
                    when.method(GET).path(binary_path(b"host controller"));
                    then.status(200).body(&controller_body);
                })
                .await;
            let root = tempfile::tempdir().unwrap();
            let cache = BundleCache::new(root.path());

            let error = provider_in(
                None,
                &cache,
                &server.base_url(),
                "dep-token",
                &setup_of(&bundle),
                "dep_demo",
                &ClientConfig::Test,
            )
            .await
            .err()
            .expect("a tampered object is refused");

            assert!(
                error.message.contains("Extension 'acme'") && error.message.contains(&object),
                "{}",
                error.message
            );
            let dir = cache.dir(&bundle.manifest).unwrap();
            assert!(!dir.join(format!("controller-{}", host())).exists());
        }
    }

    #[tokio::test]
    async fn a_bundle_without_this_machines_controller_fails_naming_the_extension_and_target() {
        let bundle = TestBundle::new(
            "acme",
            "1.0.0",
            &[(other_target(), &b"other controller"[..])],
        );
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(object_path(&bundle.manifest.digest));
                then.status(200).body(&bundle.zip);
            })
            .await;
        let root = tempfile::tempdir().unwrap();

        let error = provider_in(
            None,
            &BundleCache::new(root.path()),
            &server.base_url(),
            "dep-token",
            &setup_of(&bundle),
            "dep_demo",
            &ClientConfig::Test,
        )
        .await
        .err()
        .expect("no controller for this machine");

        assert!(
            error.message.contains("Extension 'acme'")
                && error
                    .message
                    .contains(&format!("no controller for {}", host())),
            "{}",
            error.message
        );
    }

    const ADMIN_SECRET: &str = "admin-secret-never-leaves";
    const ADMIN_SESSION: &str = "admin-session-never-leaves";

    /// The controller process the direct setup test starts: this test binary, run as this test,
    /// answering steps with the step protocol's test controller. It writes replies to descriptor
    /// 3, since the test harness prints to stdout, and records in `MOCK_CLOUD_DIR` the access key
    /// each step carried and whether its environment holds the admin's secret.
    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "the controller process the direct setup test starts"]
    async fn controller_process() {
        use alien_infra::step_protocol::protocol::{Credentials, StepRequest};
        use std::io::Write as _;
        use std::os::fd::FromRawFd as _;
        use tokio::io::AsyncBufReadExt as _;
        let cloud = PathBuf::from(std::env::var("MOCK_CLOUD_DIR").unwrap());
        let host = alien_infra::step_protocol::test_controller::ControllerHost::new(&cloud, None);
        let in_env = std::env::vars().any(|(_, value)| value.contains(ADMIN_SECRET));
        std::fs::write(cloud.join("env-holds-secret"), in_env.to_string()).unwrap();
        // SAFETY: the script that starts this process opens descriptor 3 on the parent's pipe.
        let mut replies = unsafe { std::fs::File::from_raw_fd(3) };
        let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
        while let Some(line) = lines.next_line().await.unwrap() {
            let request: StepRequest = serde_json::from_str(&line).unwrap();
            let key = match &request.credentials {
                Some(Credentials::Aws {
                    access_key_id,
                    secret_access_key,
                    session_token,
                    ..
                }) if secret_access_key == ADMIN_SECRET
                    && session_token.as_deref() == Some(ADMIN_SESSION) =>
                {
                    access_key_id.clone()
                }
                other => format!("{other:?}"),
            };
            let mut log = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(cloud.join("keys"))
                .unwrap();
            writeln!(log, "{key}").unwrap();
            let reply = host.handle(request).await;
            writeln!(replies, "{}", serde_json::to_string(&reply).unwrap()).unwrap();
            replies.flush().unwrap();
        }
    }

    /// Every file under `dir` whose bytes hold `needle`.
    #[cfg(unix)]
    fn files_holding(dir: &std::path::Path, needle: &str) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(files_holding(&path, needle));
            } else if std::fs::read(&path)
                .unwrap_or_default()
                .windows(needle.len())
                .any(|window| window == needle.as_bytes())
            {
                found.push(path);
            }
        }
        found
    }

    /// Direct setup creates a Frozen custom resource, and setup teardown deletes it, with the
    /// admin's credentials, which reach the controller process only on its stdin: not in its
    /// environment, not in anything sent to the manager, and not in any file.
    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn direct_setup_forwards_the_admins_credentials_only_to_the_controllers_stdin() {
        use alien_core::{ClientConfig, DeploymentStatus};
        use alien_deployment::loop_contract::LoopOperation;
        use alien_deployment::runner::{run_step_loop, DelayStrategy, RunnerPolicy};
        use alien_infra::step_protocol::test_controller::{MockCloud, DNS_ZONE};

        let cloud = tempfile::tempdir().unwrap();
        let script = format!(
            "#!/bin/sh\nMOCK_CLOUD_DIR='{}' exec '{}' --ignored --exact commands::up::extension_bundles::tests::controller_process --nocapture --test-threads=1 3>&1 1>&2\n",
            cloud.path().display(),
            std::env::current_exe().unwrap().display()
        );
        let bundle = TestBundle::new("acme", "1.0.0", &[(host(), script.as_bytes())]);
        let mut manifest = serde_json::to_value(&bundle.manifest).unwrap();
        manifest["resourceTypes"] = serde_json::json!({ DNS_ZONE: {
            "lifecycles": ["frozen"], "holdsData": false,
            "setup": { "cloudformation": [], "terraform": [], "helm": [], "importers": [] },
            "controllerPlatforms": ["aws"], "permissionSets": [], "link": { "permissionSets": [] },
            "schemas": { "config": {}, "outputs": {} }
        }});
        let stack = serde_json::json!({
            "id": "app",
            "resources": { "zone": {
                "config": { "type": DNS_ZONE, "id": "zone", "domain": "setup.acme.dev" },
                "lifecycle": "frozen", "dependencies": []
            }},
            "permissions": { "profiles": {}, "management": "auto" },
            "extensions": [manifest]
        });
        let mut state: DeploymentState = serde_json::from_value(serde_json::json!({
            "platform": "aws", "status": "initial-setup",
            "protocolVersion": alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            "targetRelease": { "releaseId": "rel_1", "stack": stack },
            "stackState": { "platform": "aws", "resources": {}, "resourcePrefix": "setup" },
            "runtimeMetadata": {
                "initialSetupAuthority": "directSetup",
                "preparedStack": stack,
                "pendingPreparedReleaseId": "rel_1"
            }
        }))
        .unwrap();
        let mut config: alien_core::DeploymentConfig = serde_json::from_value(serde_json::json!({
            "stackSettings": {},
            "environmentVariables": { "variables": [], "hash": "", "createdAt": "" }
        }))
        .unwrap();
        let admin = ClientConfig::Aws(Box::new(alien_core::AwsClientConfig {
            account_id: "210987654321".to_string(),
            region: "us-east-2".to_string(),
            credentials: alien_core::AwsCredentials::AccessKeys {
                access_key_id: "AKIAADMIN".to_string(),
                secret_access_key: ADMIN_SECRET.to_string(),
                session_token: Some(ADMIN_SESSION.to_string()),
            },
            service_overrides: None,
        }));

        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(object_path(&bundle.manifest.digest));
                then.status(200).body(&bundle.zip);
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(binary_path(script.as_bytes()));
                then.status(200).body(&script);
            })
            .await;
        let leaks = server
            .mock_async(|when, then| {
                when.body_contains(ADMIN_SECRET);
                then.status(500);
            })
            .await;
        let reconcile = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::POST)
                    .path("/v1/sync/reconcile");
                then.status(200)
                    .json_body(serde_json::json!({ "success": true, "current": null }));
            })
            .await;
        let home = tempfile::tempdir().unwrap();
        let cache = BundleCache::new(home.path());

        // The admin's credentials sit in this process's environment too, as the setup CLI loads
        // them; nothing in it reaches the controller but what the transport gives it.
        std::env::set_var("ALIEN_TEST_ADMIN_SECRET", ADMIN_SECRET);
        let provider = provider_in(
            None,
            &cache,
            &server.base_url(),
            "dep-token",
            &state,
            "dep_demo",
            &admin,
        )
        .await
        .unwrap();
        let transport = alien_deployment::manager_api_transport::ManagerApiTransport::new(
            alien_manager_api::Client::new(&server.base_url()),
            "session-1".to_string(),
        );
        let result = run_step_loop(
            &mut state,
            &mut config,
            &admin,
            "dep_demo",
            &RunnerPolicy {
                max_steps: 50,
                operation: LoopOperation::InitialSetup,
                delay_strategy: DelayStrategy::Inline,
            },
            &transport,
            provider.clone(),
            None,
        )
        .await;

        result.unwrap_or_else(|e| panic!("direct setup failed: {e:#?}"));
        assert_eq!(
            state.status,
            DeploymentStatus::Provisioning,
            "{:#?}",
            state.error
        );
        let zone = &state.stack_state.as_ref().unwrap().resources["zone"];
        assert_eq!(zone.status, alien_core::ResourceStatus::Running);
        assert_eq!(MockCloud::at(cloud.path()).calls("create").len(), 1);
        let keys = std::fs::read_to_string(cloud.path().join("keys")).unwrap();
        assert!(
            !keys.is_empty() && keys.lines().all(|key| key == "AKIAADMIN"),
            "every step carries the admin's credentials: {keys}"
        );
        assert_eq!(
            std::fs::read_to_string(cloud.path().join("env-holds-secret")).unwrap(),
            "false"
        );

        // Teardown deletes the zone through the same controller, with the admin's credentials.
        state.status = DeploymentStatus::TeardownRequired;
        state.current_release = state.target_release.clone();
        alien_deployment::setup_teardown::run_setup_teardown_after_handoff(
            &mut state,
            &mut config,
            &admin,
            "dep_demo",
            &RunnerPolicy {
                max_steps: 50,
                operation: LoopOperation::Delete,
                delay_strategy: DelayStrategy::Inline,
            },
            &transport,
            provider,
        )
        .await
        .unwrap_or_else(|e| panic!("setup teardown failed: {e:#?}"));
        let zones = MockCloud::at(cloud.path()).state().zones;
        assert!(
            zones.len() == 1 && zones.values().all(|zone| zone.deleted),
            "{zones:?}; {:#?}",
            state.error
        );
        let keys = std::fs::read_to_string(cloud.path().join("keys")).unwrap();
        assert!(
            keys.lines().count() > 1 && keys.lines().all(|key| key == "AKIAADMIN"),
            "every teardown step carries the admin's credentials too: {keys}"
        );

        assert!(reconcile.hits_async().await > 0);
        leaks.assert_hits_async(0).await;
        for secret in [ADMIN_SECRET, ADMIN_SESSION] {
            assert_eq!(files_holding(home.path(), secret), Vec::<PathBuf>::new());
            assert_eq!(files_holding(cloud.path(), secret), Vec::<PathBuf>::new());
            assert!(!serde_json::to_string(&state).unwrap().contains(secret));
        }
    }

    #[tokio::test]
    async fn a_setup_whose_releases_list_no_extensions_downloads_nothing() {
        let state: DeploymentState = serde_json::from_value(serde_json::json!({
            "platform": "aws", "status": "pending",
            "protocolVersion": alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            "targetRelease": {"releaseId": "rel_1", "stack": {"id": "app", "resources": {}}}
        }))
        .unwrap();

        // An unreachable manager: nothing may be requested.
        let provider = provider_for(
            None,
            "http://127.0.0.1:9",
            "dep-token",
            &state,
            "dep_demo",
            &ClientConfig::Test,
        )
        .await
        .unwrap();

        assert!(provider.is_none());

        // Teardown passes the runtime's provider, which comes back as it was, whatever
        // credentials the admin has.
        let runtime: Arc<dyn PlatformServiceProvider> =
            Arc::new(alien_infra::DefaultPlatformServiceProvider::default());
        let admin = ClientConfig::Aws(Box::new(alien_core::AwsClientConfig {
            account_id: "210987654321".to_string(),
            region: "us-east-2".to_string(),
            credentials: alien_core::AwsCredentials::AccessKeys {
                access_key_id: "AKIAADMIN".to_string(),
                secret_access_key: ADMIN_SECRET.to_string(),
                session_token: None,
            },
            service_overrides: None,
        }));
        let kept = provider_for(
            Some(runtime.clone()),
            "http://127.0.0.1:9",
            "dep-token",
            &state,
            "dep_demo",
            &admin,
        )
        .await
        .unwrap()
        .unwrap();
        assert!(Arc::ptr_eq(&kept, &runtime));
    }
}
