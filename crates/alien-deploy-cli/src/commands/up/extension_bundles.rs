//! The bundles of the extensions a deployment's releases list, as direct setup loads them: each
//! core zip, downloaded from the manager by digest, verified, and cached under
//! `~/.alien/extensions/<name>@sha256-<hex>/`. Nothing in a bundle runs before its check passes.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use alien_core::DeploymentState;
use alien_error::{AlienError, ContextError};
use alien_infra::extension_bundle::{manager_object_url, BundleCache, BundleSource, Runtime};
use alien_infra::step_protocol::wasm::WasmHost;
use alien_infra::step_protocol::ProxyFactory;
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

/// What direct setup does with a deployment.
pub(crate) enum Operation {
    /// Initial setup, or a setup update.
    Setup,
    Teardown,
}

/// The service provider for `operation` on deployment `deployment_id` in `state`. When its
/// releases list no extensions it is `provider`, unchanged; otherwise one with the same local
/// bindings, each extension's hooks module, and each extension's controller module, which runs
/// in this process. Each bundle's core zip is downloaded from the manager at `manager_url` with
/// `token` when the cache lacks it. A controller module's steps carry no cloud credentials, so
/// its cloud calls are refused: the admin's credentials reach far more than the extension's
/// identity does, and the admin can't act as that identity. For the same reason no client
/// carries the extensions' interceptors, and setup is refused when an extension ships one, as
/// it would not run. Teardown goes on without them, so a deployment can always be deleted. An
/// extension whose bundle has no controller module gets no controller. Fails, naming the
/// extension, on a bundle that fails its check.
pub(crate) async fn provider_for(
    provider: Option<Arc<dyn PlatformServiceProvider>>,
    manager_url: &str,
    token: &str,
    state: &DeploymentState,
    deployment_id: &str,
    operation: Operation,
) -> Result<Option<Arc<dyn PlatformServiceProvider>>> {
    if alien_infra::extension_bundle::release_extensions(state).is_empty() {
        return Ok(provider);
    }
    provider_in(
        provider,
        &cache()?,
        &WasmHost::shared().map_err(refused)?,
        manager_url,
        token,
        state,
        deployment_id,
        operation,
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

/// [`provider_for`] with the bundles cached in `cache` and the controller modules stepped on
/// `host`.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn provider_in(
    provider: Option<Arc<dyn PlatformServiceProvider>>,
    cache: &BundleCache,
    host: &Arc<WasmHost>,
    manager_url: &str,
    token: &str,
    state: &DeploymentState,
    deployment_id: &str,
    operation: Operation,
) -> Result<Option<Arc<dyn PlatformServiceProvider>>> {
    let extensions = alien_infra::extension_bundle::release_extensions(state);
    if extensions.is_empty() {
        return Ok(provider);
    }
    // The manager may redirect to a presigned URL on another host; the client drops the token
    // on such a redirect.
    let client = reqwest::Client::new();
    let mut bundles = std::collections::HashMap::new();
    let mut controllers = std::collections::HashMap::new();
    let mut intercepting = Vec::new();
    for extension in extensions {
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
        let hooks = bundle.load_hooks(runtime()?).map_err(refused)?;
        if alien_infra::DeploymentInterceptors::declared_in(&hooks) {
            intercepting.push(extension.name.clone());
        }
        // The setup CLI steps one deployment, so its steps share no project's limit with
        // another deployment's.
        if let Some(module) = bundle
            .wasm_transport(runtime()?, host, deployment_id)
            .map_err(refused)?
        {
            controllers.insert(
                extension.digest.clone(),
                ProxyFactory::new(deployment_id, Arc::new(module), None),
            );
        }
        bundles.insert(extension.digest.clone(), Arc::new(bundle));
    }
    if !intercepting.is_empty() {
        let names = intercepting
            .iter()
            .map(|name| format!("'{name}'"))
            .collect::<Vec<_>>()
            .join(", ");
        match operation {
            Operation::Setup => {
                let (extensions, change) = match intercepting.len() {
                    1 => ("Extension", "changes"),
                    _ => ("Extensions", "change"),
                };
                return Err(AlienError::new(ErrorData::ConfigurationError {
                    message: format!(
                        "{extensions} {names} {change} Alien's cloud calls with interceptors, which the setup CLI doesn't run: it signs those calls with your own credentials, which reach beyond this deployment. Set this deployment up from its setup template, or let the Alien manager or the Operator set it up."
                    ),
                }));
            }
            Operation::Teardown => crate::output::info(&format!(
                "Tearing down without the extensions' interceptors ({names}): the setup CLI doesn't run them."
            )),
        }
    }
    let base = alien_infra::extension_bundle::base_provider(provider.as_ref());
    let provider = alien_infra::extension_bundle::provider_with_controllers(
        base,
        state,
        runtime()?,
        &bundles,
        false,
        |extension, _| Ok(controllers.get(&extension.digest).cloned()),
    )
    .map_err(refused)?;
    Ok(Some(Arc::new(provider)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alien_core::ClientConfig;
    use alien_infra::extension_bundle::testing::{controller_guest, TestBundle};
    use httpmock::{
        Method::{GET, POST},
        MockServer,
    };

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

    fn module_host() -> Arc<WasmHost> {
        Arc::new(WasmHost::new().unwrap())
    }

    /// Setup asks the manager for a bundle's core zip alone, once, with the deployment token. A
    /// controller binary the bundle lists for this machine is never asked for.
    #[tokio::test]
    async fn downloads_only_the_core_zip_and_never_a_controller_binary() {
        let target = alien_infra::extension_bundle::host_target()
            .expect("tests run on a machine with a controller target");
        let bundle = TestBundle::new("acme", "1.0.0", &[(target, &b"host controller"[..])]);
        let server = MockServer::start_async().await;
        let core = server
            .mock_async(|when, then| {
                when.method(GET)
                    .path(object_path(&bundle.manifest.digest))
                    .header("authorization", "Bearer dep-token");
                then.status(200).body(&bundle.zip);
            })
            .await;
        let anything_else = server
            .mock_async(|_, then| {
                then.status(200).body("host controller");
            })
            .await;
        let root = tempfile::tempdir().unwrap();
        let cache = BundleCache::new(root.path());

        for _ in 0..2 {
            provider_in(
                None,
                &cache,
                &module_host(),
                &server.base_url(),
                "dep-token",
                &setup_of(&bundle),
                "dep_demo",
                Operation::Setup,
            )
            .await
            .unwrap()
            .unwrap();
        }

        core.assert_hits_async(1).await;
        anything_else.assert_hits_async(0).await;
        let cached: Vec<_> = std::fs::read_dir(cache.dir(&bundle.manifest).unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(cached, ["core.zip"]);
    }

    /// The error direct setup ends with when the manager serves `core_zip` as the core zip of
    /// `bundle`'s extension.
    async fn refusal(
        bundle: &TestBundle,
        core_zip: &[u8],
        cache: &BundleCache,
    ) -> AlienError<ErrorData> {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(object_path(&bundle.manifest.digest));
                then.status(200).body(core_zip);
            })
            .await;
        provider_in(
            None,
            cache,
            &module_host(),
            &server.base_url(),
            "dep-token",
            &setup_of(bundle),
            "dep_demo",
            Operation::Setup,
        )
        .await
        .err()
        .expect("the bundle is refused")
    }

    #[tokio::test]
    async fn refuses_a_tampered_core_zip_and_keeps_nothing_of_it() {
        let bundle = TestBundle::with_controller_module("acme", "1.0.0", controller_guest(), &[]);
        let mut tampered = bundle.zip.clone();
        *tampered.last_mut().unwrap() ^= 1;
        let root = tempfile::tempdir().unwrap();
        let cache = BundleCache::new(root.path());

        let error = refusal(&bundle, &tampered, &cache).await;

        assert!(
            error.message.contains("Extension 'acme'") && error.message.contains("core zip"),
            "{}",
            error.message
        );
        assert!(!cache
            .dir(&bundle.manifest)
            .unwrap()
            .join("core.zip")
            .exists());
    }

    #[tokio::test]
    async fn refuses_a_controller_module_the_runtime_cannot_load() {
        let bundle = TestBundle::with_controller_module("acme", "1.0.0", b"not a module", &[]);
        let root = tempfile::tempdir().unwrap();

        let error = refusal(&bundle, &bundle.zip, &BundleCache::new(root.path())).await;

        assert!(
            error.message.contains("Extension 'acme'") && error.message.contains("controller.wasm"),
            "{}",
            error.message
        );
    }

    /// A deployment whose one extension ships a controller module for `acme/probe` on AWS and an
    /// AWS interceptor that matches every call. Its hooks module answers every call with an
    /// empty output, so asking the interceptor anything fails.
    fn intercepting_deployment() -> (TestBundle, DeploymentState) {
        let bundle = TestBundle::with_module_and_hooks(
            "acme",
            "1.0.0",
            &[],
            controller_guest(),
            &[],
            r#"[{"name":"all","kind":"awsInterceptor"}]"#,
        );
        let mut manifest = serde_json::to_value(&bundle.manifest).unwrap();
        manifest["resourceTypes"] = serde_json::json!({ "acme/probe": {
            "lifecycles": ["frozen"], "holdsData": false,
            "setup": { "cloudformation": [], "terraform": [], "helm": [], "importers": [] },
            "controllerPlatforms": ["aws"], "permissionSets": [], "link": { "permissionSets": [] },
            "schemas": { "config": {}, "outputs": {} }
        }});
        let state = serde_json::from_value(serde_json::json!({
            "platform": "aws", "status": "teardown-required",
            "protocolVersion": alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            "currentRelease": {"releaseId": "rel_1", "stack": {"id": "app", "resources": {}, "extensions": [manifest]}}
        }))
        .unwrap();
        (bundle, state)
    }

    /// Setup is refused before any step when an extension ships an interceptor: the setup CLI
    /// would sign the calls it changes with the admin's credentials. Only the core zip the check
    /// reads is downloaded.
    #[tokio::test]
    async fn setup_refuses_an_extension_with_interceptors_and_names_it() {
        let (bundle, state) = intercepting_deployment();
        let server = MockServer::start_async().await;
        let core = server
            .mock_async(|when, then| {
                when.method(GET).path(object_path(&bundle.manifest.digest));
                then.status(200).body(&bundle.zip);
            })
            .await;
        let anything_else = server
            .mock_async(|_, then| {
                then.status(200);
            })
            .await;
        let root = tempfile::tempdir().unwrap();

        let error = provider_in(
            None,
            &BundleCache::new(root.path()),
            &module_host(),
            &server.base_url(),
            "dep-token",
            &state,
            "dep_demo",
            Operation::Setup,
        )
        .await
        .err()
        .expect("setup is refused");

        assert!(
            error
                .message
                .contains("Extension 'acme' changes Alien's cloud calls with interceptors")
                && error.message.contains("setup template"),
            "{}",
            error.message
        );
        assert!(!error.retryable);
        core.assert_hits_async(1).await;
        anything_else.assert_hits_async(0).await;
    }

    /// Teardown is never refused, so the deployment stays deletable: it runs with the
    /// extension's controller, and no client carries its interceptors, so the one that matches
    /// every call is never asked about a call.
    #[tokio::test]
    async fn teardown_runs_the_controller_without_the_extensions_interceptors() {
        use alien_client_core::interceptor::Cloud;

        let (bundle, state) = intercepting_deployment();
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method(GET).path(object_path(&bundle.manifest.digest));
                then.status(200).body(&bundle.zip);
            })
            .await;
        let root = tempfile::tempdir().unwrap();

        let provider = provider_in(
            None,
            &BundleCache::new(root.path()),
            &module_host(),
            &server.base_url(),
            "dep-token",
            &state,
            "dep_demo",
            Operation::Teardown,
        )
        .await
        .unwrap()
        .unwrap();

        let stack = &state.current_release.as_ref().unwrap().stack;
        let registry = alien_infra::ResourceRegistry::with_built_ins_and_extensions(
            stack,
            &provider.extensions().unwrap(),
        );
        registry
            .get_controller("acme/probe".into(), alien_core::Platform::Aws)
            .unwrap_or_else(|e| panic!("the controller is loaded: {e:#?}"));
        let mut call = reqwest::Request::new(
            reqwest::Method::POST,
            "https://ec2.us-east-1.amazonaws.com/".parse().unwrap(),
        );
        let hook = provider
            .interceptors(Cloud::Aws, None)
            .intercept(Cloud::Aws, "ec2", &mut call)
            .unwrap_or_else(|e| panic!("no interceptor is asked: {e:#?}"));
        assert!(hook.is_none());
    }

    const ADMIN_SECRET: &str = "admin-secret-never-leaves";
    const ADMIN_SESSION: &str = "admin-session-never-leaves";

    /// Direct setup steps a Frozen custom resource in its bundle's controller module, and setup
    /// teardown deletes it there. The module tries to call EC2 in each step: the call is refused
    /// before it is signed, so nothing reaches the cloud with the admin's credentials, and nothing
    /// kept in the state or sent to the manager holds them.
    #[tokio::test(flavor = "multi_thread")]
    async fn direct_setup_steps_a_custom_resource_in_its_module_without_cloud_credentials() {
        use alien_core::DeploymentStatus;
        use alien_deployment::loop_contract::LoopOperation;
        use alien_deployment::runner::{run_step_loop, DelayStrategy, RunnerPolicy};

        // The CLI's entry point sets the key; this test starts below it.
        alien_deployment::set_secrets_digest_key_if_unset([9; 32]);
        let bundle = TestBundle::with_controller_module("acme", "1.0.0", controller_guest(), &[]);
        let mut manifest = serde_json::to_value(&bundle.manifest).unwrap();
        manifest["resourceTypes"] = serde_json::json!({ "acme/probe": {
            "lifecycles": ["frozen"], "holdsData": false,
            "setup": { "cloudformation": [], "terraform": [], "helm": [], "importers": [] },
            "controllerPlatforms": ["aws"], "permissionSets": [], "link": { "permissionSets": [] },
            "schemas": { "config": {}, "outputs": {} }
        }});
        // The module's `cloud` probe sends its `arg` as a cloud call in each step.
        let stack = serde_json::json!({
            "id": "app",
            "resources": { "zone": {
                "config": {
                    "type": "acme/probe", "id": "zone", "probe": "cloud",
                    "arg": {
                        "cloud": "aws", "service": "ec2", "method": "POST", "path": "/",
                        "body": "Action=DescribeRegions&Version=2016-11-15"
                    }
                },
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

        // The EC2 of the admin's region, on this machine: no call may reach it.
        let ec2 = MockServer::start_async().await;
        let cloud_calls = ec2
            .mock_async(|_, then| {
                then.status(200).body("<DescribeRegionsResponse/>");
            })
            .await;
        let host = Arc::new(
            WasmHost::new()
                .unwrap()
                .routing("https://ec2.us-east-2.amazonaws.com", &ec2.base_url()),
        );

        let manager = MockServer::start_async().await;
        let leaks = manager
            .mock_async(|when, then| {
                when.body_contains(ADMIN_SECRET);
                then.status(500);
            })
            .await;
        manager
            .mock_async(|when, then| {
                when.method(GET).path(object_path(&bundle.manifest.digest));
                then.status(200).body(&bundle.zip);
            })
            .await;
        let reconcile = manager
            .mock_async(|when, then| {
                when.method(POST).path("/v1/sync/reconcile");
                then.status(200)
                    .json_body(serde_json::json!({ "success": true, "current": null }));
            })
            .await;
        let home = tempfile::tempdir().unwrap();

        let provider = provider_in(
            None,
            &BundleCache::new(home.path()),
            &host,
            &manager.base_url(),
            "dep-token",
            &state,
            "dep_demo",
            Operation::Setup,
        )
        .await
        .unwrap();
        let transport = alien_deployment::manager_api_transport::ManagerApiTransport::new(
            alien_manager_api::Client::new(&manager.base_url()),
            "session-1".to_string(),
        );
        run_step_loop(
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
        .await
        .unwrap_or_else(|e| panic!("direct setup failed: {e:#?}"));

        assert_eq!(
            state.status,
            DeploymentStatus::Provisioning,
            "{:#?}",
            state.error
        );
        let zone = &state.stack_state.as_ref().unwrap().resources["zone"];
        assert_eq!(zone.status, alien_core::ResourceStatus::Running);
        let progress = zone.internal_state.as_ref().unwrap()["remote"]["progress"].to_string();
        assert!(
            progress.contains("ERR") && progress.contains("no cloud credentials"),
            "{progress}"
        );

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

        assert_eq!(
            state.status,
            DeploymentStatus::Deleted,
            "{:#?}",
            state.error
        );
        cloud_calls.assert_hits_async(0).await;
        assert!(reconcile.hits_async().await > 0);
        leaks.assert_hits_async(0).await;
        for secret in [ADMIN_SECRET, ADMIN_SESSION] {
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
            Operation::Setup,
        )
        .await
        .unwrap();

        assert!(provider.is_none());

        // Teardown passes the runtime's provider, which comes back as it was.
        let runtime: Arc<dyn PlatformServiceProvider> =
            Arc::new(alien_infra::DefaultPlatformServiceProvider::default());
        let kept = provider_for(
            Some(runtime.clone()),
            "http://127.0.0.1:9",
            "dep-token",
            &state,
            "dep_demo",
            Operation::Teardown,
        )
        .await
        .unwrap()
        .unwrap();
        assert!(Arc::ptr_eq(&kept, &runtime));
    }
}
