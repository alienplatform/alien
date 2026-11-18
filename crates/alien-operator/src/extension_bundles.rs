//! The bundles of the extensions this deployment's releases list, verified and cached by digest
//! in `<data dir>/synced-extensions/`.
//!
//! The sync loop downloads a release's bundles before it accepts the release. A bundle that
//! fails its check is refused: the release is never accepted, and the deployment stays on the
//! release it runs, with that release's bundles. The deployment loop then reads them from the
//! cache only.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use alien_core::{ClientConfig, DeploymentState, Stack};
use alien_error::{AlienError, ContextError};
use alien_infra::extension_bundle::{manager_object_url, BundleCache, BundleSource, Runtime};
use alien_infra::step_protocol::wasm::WasmHost;
use alien_infra::step_protocol::{CredentialSource, ExtensionIdentityCredentials, ProxyFactory};
use alien_infra::PlatformServiceProvider;

use crate::error::{ErrorData, Result};

/// The folder under the Operator's data directory that holds extension bundles.
pub const SYNCED_EXTENSIONS_DIR: &str = "synced-extensions";

/// The cache under `data_dir`.
pub fn cache(data_dir: &str) -> BundleCache {
    BundleCache::new(Path::new(data_dir).join(SYNCED_EXTENSIONS_DIR))
}

/// Downloads the core zip of each extension `stacks` list that the cache lacks, from the manager
/// at `manager_url`, by the digest the stack pins. `manager` is the client authenticated to the
/// manager; a manager that redirects to a presigned URL gets the token dropped on the way.
pub async fn download(
    cache: &BundleCache,
    manager: &reqwest::Client,
    manager_url: &str,
    stacks: &[&Stack],
) -> Result<()> {
    for extension in stacks.iter().flat_map(|stack| &stack.extensions) {
        let core_url = manager_object_url(manager_url, &extension.digest);
        cache
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

/// The service provider for a deployment in `state`. When its releases list no extensions it
/// is `provider`, the Operator's own; otherwise one with the same local bindings and, from
/// `cache`, each extension's hooks module and its controller module, which runs in this process.
/// A module's cloud calls are signed with credentials minted for each step from `client_config`,
/// the Operator's own, for the extension's identity. An extension whose bundle has no controller
/// module gets no controller, so stepping one of its resources fails naming it. Fails, naming
/// the extension, when one is not cached.
pub async fn provider_for(
    provider: Option<Arc<dyn PlatformServiceProvider>>,
    cache: &BundleCache,
    state: &DeploymentState,
    deployment_id: &str,
    client_config: &ClientConfig,
) -> Result<Option<Arc<dyn PlatformServiceProvider>>> {
    if alien_infra::extension_bundle::release_extensions(state).is_empty() {
        return Ok(provider);
    }
    let host = WasmHost::shared().map_err(into_deployment)?;
    provider_on(&host, provider, cache, state, deployment_id, client_config)
        .await
        .map(Some)
}

/// [`provider_for`] a deployment whose releases list extensions, with their controller modules
/// stepped on `host`.
async fn provider_on(
    host: &Arc<WasmHost>,
    provider: Option<Arc<dyn PlatformServiceProvider>>,
    cache: &BundleCache,
    state: &DeploymentState,
    deployment_id: &str,
    client_config: &ClientConfig,
) -> Result<Arc<dyn PlatformServiceProvider>> {
    let mut bundles = HashMap::new();
    let mut controllers = HashMap::new();
    for extension in alien_infra::extension_bundle::release_extensions(state) {
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
        // The Operator steps one deployment, so its steps share no project's limit with another
        // deployment's.
        if let Some(module) = bundle
            .wasm_transport(runtime()?, host, deployment_id)
            .map_err(into_deployment)?
        {
            let credentials = ExtensionIdentityCredentials::new(extension, client_config)
                .map(|source| Arc::new(source) as Arc<dyn CredentialSource>);
            controllers.insert(
                extension.digest.clone(),
                ProxyFactory::new(deployment_id, Arc::new(module), credentials),
            );
        }
        bundles.insert(extension.digest.clone(), Arc::new(bundle));
    }
    let base = alien_infra::extension_bundle::base_provider(provider.as_ref());
    let provider = alien_infra::extension_bundle::provider_for(
        base,
        state,
        runtime()?,
        &bundles,
        &controllers,
    )
    .map_err(into_deployment)?;
    Ok(Arc::new(provider))
}

#[cfg(test)]
mod tests;
