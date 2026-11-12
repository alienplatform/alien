//! The bundles of the extensions this deployment's releases list, verified and cached by digest
//! in `<data dir>/synced-extensions/`.
//!
//! The sync loop downloads a release's bundles before it accepts the release. A bundle that
//! fails its check is refused: the release is never accepted, and the deployment stays on the
//! release it runs, with that release's bundles. The deployment loop then reads them from the
//! cache only.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use alien_core::{ClientConfig, DeploymentState, ExtensionManifest, Platform, Stack};
use alien_error::{AlienError, ContextError};
use alien_infra::extension_bundle::{manager_object_url, BundleCache, BundleSource, Runtime};
use alien_infra::step_protocol::{ProxyFactory, StdioTransport, Transport};
use alien_infra::PlatformServiceProvider;

use crate::controller_pods::{ControllerPods, VerifiedController};
use crate::error::{ErrorData, Result};

/// The folder under the Operator's data directory that holds extension bundles.
pub const SYNCED_EXTENSIONS_DIR: &str = "synced-extensions";

/// The cache under `data_dir`.
pub fn cache(data_dir: &str) -> BundleCache {
    BundleCache::new(Path::new(data_dir).join(SYNCED_EXTENSIONS_DIR))
}

/// Removes the private copies of controller binaries an Operator left in the cache under
/// `data_dir` when it ended without removing them, as a crash does. Each is as large as its
/// binary, on a volume the Operator's own state shares. Only the Operator that holds the
/// instance lock on `data_dir` may call it, so no other process runs a copy there.
pub async fn remove_stale_private_copies(data_dir: &str) {
    alien_infra::step_protocol::remove_stale_private_copies(
        &Path::new(data_dir).join(SYNCED_EXTENSIONS_DIR),
    )
    .await;
}

/// Downloads the bundle of each extension `stacks` list that the cache lacks, from the manager
/// at `manager_url`: the core zip by the digest the stack pins, and this machine's controller by
/// the sha256 the verified zip's manifest pins. `manager` is the client authenticated to the
/// manager; a manager that redirects to a presigned URL gets the token dropped on the way.
pub async fn download(
    cache: &BundleCache,
    manager: &reqwest::Client,
    manager_url: &str,
    stacks: &[&Stack],
) -> Result<()> {
    for extension in stacks.iter().flat_map(|stack| &stack.extensions) {
        let core_url = manager_object_url(manager_url, &extension.digest);
        let bundle = cache
            .core(
                extension,
                BundleSource {
                    client: manager,
                    url: &core_url,
                    bearer: None,
                },
            )
            .await
            .map_err(into_sync)?;
        let controller_url = bundle
            .host_controller()
            .map_err(into_sync)?
            .map(|(_, binary)| {
                manager_object_url(
                    manager_url,
                    &format!("sha256:{}", binary.sha256.to_ascii_lowercase()),
                )
            });
        cache
            .controller(
                extension,
                &bundle,
                controller_url.as_deref().map(|url| BundleSource {
                    client: manager,
                    url,
                    bearer: None,
                }),
            )
            .await
            .map_err(into_sync)?;
    }
    Ok(())
}

fn into_sync<E>(error: AlienError<E>) -> AlienError<ErrorData>
where
    E: alien_error::AlienErrorData
        + Clone
        + std::fmt::Debug
        + serde::Serialize
        + Send
        + Sync
        + 'static,
{
    let message = error.message.clone();
    if error.code == "EXTENSION_BUNDLE_REFUSED" {
        return error.context(ErrorData::ExtensionBundleRefused { message });
    }
    // A download cut short may finish on the next sync; any other failure keeps its own flag.
    let retryable = error.retryable;
    let mut wrapped = error.context(ErrorData::SyncFailed { message });
    wrapped.retryable = retryable;
    wrapped
}

/// The hooks engine of this process. The hooks runtime expects one per process, shared by every
/// module it loads; it starts the first time a release lists an extension.
fn runtime() -> Result<&'static Runtime> {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    if let Some(runtime) = RUNTIME.get() {
        return Ok(runtime);
    }
    let runtime = Runtime::new().map_err(into_deployment)?;
    Ok(RUNTIME.get_or_init(|| runtime))
}

fn into_deployment<E>(error: AlienError<E>) -> AlienError<ErrorData>
where
    E: alien_error::AlienErrorData
        + Clone
        + std::fmt::Debug
        + serde::Serialize
        + Send
        + Sync
        + 'static,
{
    let message = error.message.clone();
    error.context(ErrorData::DeploymentFailed { message })
}

/// Where the Operator runs the controllers of the extensions its deployment's releases list. A
/// controller is code Alien did not write, so it runs only where it can find no credentials
/// stronger than its own.
#[derive(Debug)]
pub enum Controllers {
    /// Processes beside the Operator, on the local platform: the Operator holds no cloud
    /// identity there and setup creates none for an extension.
    Processes(ControllerProcesses),
    /// A pod per controller on Kubernetes, apart from the Operator's.
    Pods(OperatorPods),
    /// Nowhere: an Operator that runs as a process on this platform holds a cloud identity, and
    /// a controller beside it could use that identity.
    Refused(Platform),
}

impl Default for Controllers {
    fn default() -> Self {
        Self::Processes(ControllerProcesses::default())
    }
}

impl Controllers {
    /// Where the Operator configured by `config`, whose state is in `db`, runs controllers.
    pub fn for_operator(
        config: &crate::config::OperatorConfig,
        db: Arc<crate::db::OperatorDb>,
    ) -> Self {
        match config.platform {
            Platform::Kubernetes => Self::Pods(OperatorPods {
                namespace: config.namespace.clone(),
                label_domain: config.label_domain.clone(),
                resource_prefix: config.resource_prefix.clone(),
                key: StepKey::Stored(db),
                pods: Default::default(),
            }),
            Platform::Local | Platform::Test => Self::default(),
            other => Self::Refused(other),
        }
    }

    /// A running controller for each of `binaries` and no other, and the transport that steps
    /// each.
    async fn keep(
        &self,
        binaries: &[CachedBinary<'_>],
        deployment_id: &str,
        client_config: &ClientConfig,
    ) -> Result<Vec<Arc<dyn Transport>>> {
        match self {
            Self::Processes(processes) => Ok(processes
                .keep(binaries)
                .await?
                .into_iter()
                .map(|transport| transport as Arc<dyn Transport>)
                .collect()),
            Self::Pods(operator) => {
                let Some(pods) = operator.pods(binaries.first())? else {
                    return Ok(Vec::new());
                };
                let verified: Vec<VerifiedController<'_>> = binaries
                    .iter()
                    .map(|binary| VerifiedController {
                        extension: binary.extension,
                        path: binary.path.clone(),
                        sha256: binary.sha256.clone(),
                    })
                    .collect();
                pods.keep(&verified, deployment_id, client_config, || operator.key())
                    .await
            }
            Self::Refused(platform) => match binaries.first() {
                None => Ok(Vec::new()),
                Some(binary) => Err(AlienError::new(ErrorData::DeploymentFailed {
                    message: format!(
                        "Extension '{}': its controller can't run beside this Operator, which runs as a process with the credentials of its {platform} identity; run the deployment on Kubernetes, where the controller gets a pod of its own",
                        binary.extension.name
                    ),
                })),
            },
        }
    }
}

/// The controller pods of an Operator on Kubernetes, made the first time they are needed from
/// what the chart told the Operator about its install.
pub struct OperatorPods {
    namespace: Option<String>,
    label_domain: Option<String>,
    resource_prefix: Option<String>,
    key: StepKey,
    pods: std::sync::OnceLock<ControllerPods>,
}

/// Where the Operator's step-token key pair is.
enum StepKey {
    /// In the Operator's database, made the first time a pod is wanted.
    Stored(Arc<crate::db::OperatorDb>),
    /// This one, as base64.
    Given(String),
}

impl std::fmt::Debug for OperatorPods {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OperatorPods")
            .field("namespace", &self.namespace)
            .field("resource_prefix", &self.resource_prefix)
            .finish_non_exhaustive()
    }
}

impl OperatorPods {
    /// `pods`, stepped with tokens signed by `key`, an Ed25519 key pair as base64.
    pub fn of(pods: ControllerPods, key: String) -> Self {
        Self {
            namespace: None,
            label_domain: None,
            resource_prefix: None,
            key: StepKey::Given(key),
            pods: std::sync::OnceLock::from(pods),
        }
    }

    /// The pods. `None` when the chart named no namespace or prefix and no controller is
    /// wanted: such an Operator never made a pod. With `wanted`, a controller to run, the
    /// missing setting fails naming its extension.
    fn pods(&self, wanted: Option<&CachedBinary<'_>>) -> Result<Option<&ControllerPods>> {
        if let Some(pods) = self.pods.get() {
            return Ok(Some(pods));
        }
        let (Some(namespace), Some(prefix)) = (&self.namespace, &self.resource_prefix) else {
            return match wanted {
                None => Ok(None),
                Some(binary) => Err(AlienError::new(ErrorData::DeploymentFailed {
                    message: format!(
                        "Extension '{}': its controller runs in a pod of its own, which needs the namespace and the resource prefix the chart gives the Operator; one is not set",
                        binary.extension.name
                    ),
                })),
            };
        };
        Ok(Some(self.pods.get_or_init(|| {
            let scope = alien_infra::kubernetes_deployment_scope_label_for(
                self.label_domain.as_deref(),
                prefix,
            );
            ControllerPods::new(
                Arc::new(crate::controller_pods::KubernetesCluster::new(
                    alien_core::KubernetesClientConfig::InCluster {
                        namespace: Some(namespace.clone()),
                        additional_headers: None,
                    },
                    namespace.clone(),
                    scope,
                    prefix.clone(),
                    std::env::var("HOSTNAME").ok(),
                )),
                prefix.clone(),
            )
        })))
    }

    /// The Operator's step-token key pair, base64.
    async fn key(&self) -> Result<String> {
        match &self.key {
            StepKey::Stored(db) => db.step_signing_key().await,
            StepKey::Given(key) => Ok(key.clone()),
        }
    }
}

/// The controller processes of the extensions the Operator's deployment runs, one per verified
/// binary. Each runs a private copy of the bytes its binary was checked against, starts on its
/// first step, is killed and started again when it exits or misses a step's deadline, and is
/// killed once its binary leaves the deployment's releases. The Operator runs one deployment, so
/// these are that deployment's.
#[derive(Debug, Default)]
pub struct ControllerProcesses {
    by_binary: tokio::sync::Mutex<HashMap<PathBuf, Arc<StdioTransport>>>,
}

/// A controller binary in the cache, with what its bundle pins it to.
struct CachedBinary<'a> {
    extension: &'a ExtensionManifest,
    path: PathBuf,
    sha256: String,
    /// The bundle's folder in the cache, which holds the binary's private copies too.
    folder: PathBuf,
}

impl ControllerProcesses {
    /// The process for each of `binaries`, a new one checked against its sha256 for a binary
    /// that has none. Every other process is dropped, which kills it once the pass that last
    /// used it ends.
    async fn keep(&self, binaries: &[CachedBinary<'_>]) -> Result<Vec<Arc<StdioTransport>>> {
        let mut by_binary = self.by_binary.lock().await;
        by_binary.retain(|path, _| binaries.iter().any(|binary| &binary.path == path));
        let mut kept = Vec::new();
        for binary in binaries {
            let transport = match by_binary.get(&binary.path) {
                Some(transport) => transport.clone(),
                None => {
                    let transport = Arc::new(
                        StdioTransport::verified(
                            &binary.extension.name,
                            &binary.path,
                            &binary.sha256,
                            &binary.folder,
                            Vec::new(),
                        )
                        .await
                        .map_err(into_deployment)?,
                    );
                    by_binary.insert(binary.path.clone(), transport.clone());
                    transport
                }
            };
            kept.push(transport);
        }
        Ok(kept)
    }
}

/// The service provider for a deployment in `state`. When its releases list no extensions it
/// is `provider`, the Operator's own; otherwise one with the same local bindings, each
/// extension's hooks module from `cache`, and each extension's controller binary from `cache`
/// running in `controllers`. A step carries no credentials: the Operator holds none of an
/// extension's identity, and a controller in a pod gets its own from the pod. Fails, naming the
/// extension, when one is not cached.
pub async fn provider_for(
    provider: Option<Arc<dyn PlatformServiceProvider>>,
    cache: &BundleCache,
    state: &DeploymentState,
    deployment_id: &str,
    client_config: &ClientConfig,
    controllers: &Controllers,
) -> Result<Option<Arc<dyn PlatformServiceProvider>>> {
    let extensions = alien_infra::extension_bundle::release_extensions(state);
    if extensions.is_empty() {
        controllers.keep(&[], deployment_id, client_config).await?;
        return Ok(provider);
    }
    let runs = state
        .target_release
        .as_ref()
        .or(state.current_release.as_ref());
    let mut bundles = HashMap::new();
    let mut binaries = Vec::new();
    for extension in &extensions {
        let bundle = cache
            .cached(extension)
            .await
            .map_err(into_deployment)?
            .ok_or_else(|| {
                AlienError::new(ErrorData::DeploymentFailed {
                    message: format!(
                        "Extension '{}': its bundle {} is not downloaded yet",
                        extension.name, extension.digest
                    ),
                })
            })?;
        // The sync loop downloaded and verified the binary; the cache checks it again here.
        let pinned = match bundle.host_controller() {
            Ok(pinned) => pinned.map(|(_, binary)| binary.sha256.clone()),
            // The release an update replaces may have been accepted on a machine of another
            // architecture. Its controller only deletes what the update removes, so the update
            // runs without it, and such a delete fails naming the extension.
            Err(_)
                if runs.is_some_and(|release| {
                    !release.stack.extensions.iter().any(|listed| {
                        listed.digest == extension.digest && listed.name == extension.name
                    })
                }) =>
            {
                bundles.insert(extension.digest.clone(), Arc::new(bundle));
                continue;
            }
            Err(error) => return Err(into_deployment(error)),
        };
        let cached = cache
            .controller(extension, &bundle, None)
            .await
            .map_err(into_deployment)?;
        if let (Some(sha256), Some(path)) = (pinned, cached) {
            binaries.push(CachedBinary {
                extension,
                path,
                sha256,
                folder: cache.dir(extension).map_err(into_deployment)?,
            });
        }
        bundles.insert(extension.digest.clone(), Arc::new(bundle));
    }
    let transports = controllers
        .keep(&binaries, deployment_id, client_config)
        .await?;
    let controllers = binaries
        .iter()
        .zip(transports)
        .map(|(binary, transport)| {
            (
                binary.extension.digest.clone(),
                ProxyFactory::new(deployment_id, transport, None),
            )
        })
        .collect();
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
    .map_err(into_deployment)?;
    Ok(Some(Arc::new(provider)))
}

#[cfg(all(test, unix))]
mod tests;
