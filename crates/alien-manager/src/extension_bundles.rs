//! The extension bundles a manager loads for the deployments it runs: each core zip fetched from
//! the [`ExtensionBundleStore`] once per project and digest, checked against the digest the stack
//! pins, and kept for the life of the process. A digest names its bytes, so a kept bundle never
//! goes stale.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, PoisonError};

use tokio::sync::OnceCell;

use alien_core::{DeploymentState, ExtensionManifest, Stack};
use alien_error::{AlienError, Context, GenericError, IntoAlienError};
use alien_infra::extension_bundle::{sha256_hex, ExtensionBundle};
use alien_infra::step_protocol::{ProxyFactory, StdioTransport};

use crate::auth::Subject;
use crate::traits::{BundleObject, ExtensionBundleStore};

/// Finds an extension's controller binary on the machine the manager runs on, from the
/// extension's name and the sha256 its bundle pins for this machine's controller: its path,
/// `None` when the machine has none, or why it could not look.
pub type FindLocalController =
    Arc<dyn Fn(&str, &str) -> Result<Option<PathBuf>, AlienError> + Send + Sync>;

/// How a manager on the developer's machine runs each extension's controller as a process, for
/// `alien dev`.
pub struct LocalControllers {
    find: FindLocalController,
    /// All the environment a controller process gets, beside the system and network variables
    /// every controller process gets.
    env: Vec<(String, String)>,
    /// Where each process's private copy of its binary goes: outside the folders the binaries
    /// are built into, which a rebuild empties.
    copies: PathBuf,
    /// One process per controller binary, by its sha256, kept for the manager's lifetime.
    processes: tokio::sync::Mutex<HashMap<String, Arc<StdioTransport>>>,
}

impl LocalControllers {
    /// Controllers `find` locates, each started with `env` from a private copy under `copies`.
    pub fn new(
        find: FindLocalController,
        env: Vec<(String, String)>,
        copies: impl Into<PathBuf>,
    ) -> Self {
        Self {
            find,
            env,
            copies: copies.into(),
            processes: tokio::sync::Mutex::default(),
        }
    }

    /// The process for `bundle`'s controller on this machine, started from a binary with the
    /// sha256 the bundle pins, or `None` when the extension has no controller.
    async fn process(
        &self,
        extension: &ExtensionManifest,
        bundle: &ExtensionBundle,
    ) -> Result<Option<Arc<StdioTransport>>, AlienError> {
        let Some((target, binary)) = bundle.host_controller().map_err(AlienError::into_generic)?
        else {
            return Ok(None);
        };
        let sha256 = binary.sha256.to_ascii_lowercase();
        let mut processes = self.processes.lock().await;
        if let Some(transport) = processes.get(&sha256) {
            return Ok(Some(transport.clone()));
        }
        let object = format!("controller for {target}");
        let path = (self.find)(&extension.name, &sha256)
            .context(GenericError {
                message: format!(
                    "Extension '{}': its {object} with sha256 {sha256} could not be looked for",
                    extension.name
                ),
            })?
            .ok_or_else(|| {
                AlienError::new(GenericError {
                    message: format!(
                        "Extension '{}': its {object} with sha256 {sha256} is not on this machine; run `alien extensions build` in its workspace",
                        extension.name
                    ),
                })
            })?;
        let transport = Arc::new(
            StdioTransport::verified(
                &extension.name,
                &path,
                &sha256,
                &self.copies,
                self.env.clone(),
            )
            .await
            .map_err(AlienError::into_generic)?,
        );
        processes.insert(sha256, transport.clone());
        Ok(Some(transport))
    }
}

/// The bundles of the extensions a manager's deployments list.
pub struct ExtensionBundles {
    store: Arc<dyn ExtensionBundleStore>,
    /// Set only where the manager runs on the developer's machine, for `alien dev`.
    local_controllers: Option<LocalControllers>,
    /// The client for a store that answers with a URL, or why it could not be built.
    http: Result<reqwest::Client, String>,
    /// Started on first use, so a manager whose stacks list no extensions runs no hooks engine.
    runtime: OnceLock<alien_infra::extension_bundle::Runtime>,
    /// By project and digest, least recently used first. Each cell is filled once, so concurrent
    /// loads of one bundle fetch it once, and a slow fetch holds up only the loads of that
    /// bundle.
    opened: std::sync::Mutex<VecDeque<(BundleKey, Arc<OnceCell<Opened>>)>>,
}

type BundleKey = (String, String);

/// An opened bundle, with the size of the core zip it came from.
struct Opened {
    bundle: Arc<ExtensionBundle>,
    bytes: usize,
}

/// How many opened bundles a manager keeps, and how many bytes of core zips they may come
/// from. Past either, the one used longest ago is fetched again when a deployment needs it.
const MAX_OPENED_BUNDLES: usize = 256;
const MAX_OPENED_BYTES: usize = 512 << 20;

/// Drops the bundles used longest ago until at most `max_bundles` are left, opened from at
/// most `max_bytes`; the one used last always stays.
fn evict(
    opened: &mut VecDeque<(BundleKey, Arc<OnceCell<Opened>>)>,
    max_bundles: usize,
    max_bytes: usize,
) {
    let bytes = |opened: &VecDeque<(BundleKey, Arc<OnceCell<Opened>>)>| -> usize {
        opened
            .iter()
            .filter_map(|(_, cell)| cell.get())
            .map(|opened| opened.bytes)
            .sum()
    };
    while opened.len() > 1 && (opened.len() > max_bundles || bytes(opened) > max_bytes) {
        opened.pop_front();
    }
}

/// How long fetching one core zip may take.
const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

impl ExtensionBundles {
    pub fn new(store: Arc<dyn ExtensionBundleStore>) -> Self {
        Self {
            store,
            local_controllers: None,
            http: reqwest::Client::builder()
                .timeout(FETCH_TIMEOUT)
                .build()
                .map_err(|error| error.to_string()),
            runtime: OnceLock::new(),
            opened: std::sync::Mutex::default(),
        }
    }

    /// Runs each extension's controller as a process on this machine, from a binary with the
    /// sha256 its verified bundle pins.
    pub fn with_local_controllers(mut self, local: LocalControllers) -> Self {
        self.local_controllers = Some(local);
        self
    }

    /// The store the bundles come from.
    pub fn store(&self) -> &Arc<dyn ExtensionBundleStore> {
        &self.store
    }

    /// `extension`'s bundle, as a release of `project_id` pins it, fetched as `caller` on the
    /// first call for its project and digest.
    pub async fn open(
        &self,
        caller: &Subject,
        project_id: &str,
        extension: &ExtensionManifest,
    ) -> Result<Arc<ExtensionBundle>, AlienError> {
        let key = (project_id.to_string(), extension.digest.clone());
        let cell = {
            // No await while the map is locked.
            let mut opened = self.opened.lock().unwrap_or_else(PoisonError::into_inner);
            let index = opened.iter().position(|(opened_key, _)| *opened_key == key);
            let used = index
                .and_then(|index| opened.remove(index))
                .unwrap_or_else(|| (key, Arc::new(OnceCell::new())));
            let cell = used.1.clone();
            opened.push_back(used);
            evict(&mut opened, MAX_OPENED_BUNDLES, MAX_OPENED_BYTES);
            cell
        };
        let opened = cell
            .get_or_try_init(|| async {
                let zip = self.fetch(caller, project_id, extension).await?;
                let bundle =
                    ExtensionBundle::open(extension, &zip).map_err(AlienError::into_generic)?;
                Ok::<_, AlienError>(Opened {
                    bundle: Arc::new(bundle),
                    bytes: zip.len(),
                })
            })
            .await?;
        let bundle = opened.bundle.clone();
        evict(
            &mut self.opened.lock().unwrap_or_else(PoisonError::into_inner),
            MAX_OPENED_BUNDLES,
            MAX_OPENED_BYTES,
        );
        Ok(bundle)
    }

    async fn fetch(
        &self,
        caller: &Subject,
        project_id: &str,
        extension: &ExtensionManifest,
    ) -> Result<Vec<u8>, AlienError> {
        let unavailable = |message: String| GenericError {
            message: format!(
                "Extension '{}': its bundle {} is unavailable: {message}",
                extension.name, extension.digest
            ),
        };
        let sha256 = sha256_hex(&extension.digest).ok_or_else(|| {
            AlienError::new(unavailable("the digest is not sha256:<hex>".to_string()))
        })?;
        match self.store.get(caller, project_id, sha256).await? {
            Some(BundleObject::Bytes(bytes)) => Ok(bytes),
            Some(BundleObject::Url(url)) => {
                // The URL may be presigned: its query is a credential, so no error carries it.
                let http = self.http.as_ref().map_err(|error| {
                    AlienError::new(unavailable(format!(
                        "the HTTP client could not be built: {error}"
                    )))
                })?;
                let response = http.get(&url).send().await.map_err(|error| {
                    AlienError::new(unavailable(error.without_url().to_string()))
                })?;
                if !response.status().is_success() {
                    return Err(AlienError::new(unavailable(format!(
                        "the download returned HTTP {}",
                        response.status()
                    ))));
                }
                // A core zip is at most one bundle object, so a longer download is refused as
                // it arrives rather than held whole.
                let limit = alien_infra::extension_bundle::MAX_OBJECT_BYTES as usize;
                let mut response = response;
                let mut zip = Vec::new();
                while let Some(chunk) = response
                    .chunk()
                    .await
                    .into_alien_error()
                    .context(unavailable("the download was cut short".to_string()))?
                {
                    if zip.len() + chunk.len() > limit {
                        return Err(AlienError::new(unavailable(format!(
                            "the download is larger than the {limit} bytes a bundle object may take"
                        ))));
                    }
                    zip.extend_from_slice(&chunk);
                }
                Ok(zip)
            }
            None => {
                let mut error = AlienError::new(unavailable(
                    "this manager has no such bundle; release it again".to_string(),
                ));
                error.http_status_code = Some(404);
                Err(error)
            }
        }
    }

    /// `provider` for deployment `deployment_id` of `project_id` in `state`, with the hooks
    /// module of each extension its current and target releases list, and each extension's
    /// controller: with local controllers, its process on this machine; otherwise the controller
    /// `controller` gives for its bundle.
    pub async fn provider_for(
        &self,
        provider: alien_infra::DefaultPlatformServiceProvider,
        project_id: &str,
        deployment_id: &str,
        state: &DeploymentState,
        controller: impl Fn(&ExtensionManifest, &ExtensionBundle) -> Option<ProxyFactory>,
    ) -> Result<alien_infra::DefaultPlatformServiceProvider, AlienError> {
        let extensions = alien_infra::extension_bundle::release_extensions(state);
        if extensions.is_empty() {
            return Ok(provider);
        }
        let mut bundles = HashMap::new();
        let mut controllers = HashMap::new();
        for extension in extensions {
            let bundle = self.open(&Subject::system(), project_id, extension).await?;
            if let Some(local) = &self.local_controllers {
                if let Some(transport) = local.process(extension, &bundle).await? {
                    // `alien dev` makes no cloud call, so a step carries no credentials.
                    controllers.insert(
                        extension.digest.clone(),
                        ProxyFactory::new(deployment_id, transport, None),
                    );
                }
            }
            bundles.insert(extension.digest.clone(), bundle);
        }
        let runtime = self.runtime()?;
        alien_infra::extension_bundle::provider_with_controllers(
            provider,
            state,
            runtime,
            &bundles,
            |extension, bundle| {
                Ok(match &self.local_controllers {
                    // `alien dev` runs controllers on this machine only: a type with none stops
                    // naming the extension, rather than asking for a server it can't sign for.
                    Some(_) => controllers.get(&extension.digest).cloned(),
                    None => controller(extension, bundle),
                })
            },
        )
        .map_err(AlienError::into_generic)
    }

    /// The hooks module of each extension `stack`, a stack of a release of `project_id`, lists,
    /// fetched as `caller`, who read the release.
    pub async fn loaded_extensions(
        &self,
        caller: &Subject,
        project_id: &str,
        stack: &Stack,
    ) -> Result<Arc<alien_infra::LoadedExtensions>, AlienError> {
        let mut loaded = alien_infra::LoadedExtensions::default();
        for extension in &stack.extensions {
            let module = self
                .open(caller, project_id, extension)
                .await?
                .load_hooks(self.runtime()?)
                .map_err(AlienError::into_generic)?;
            loaded = loaded.with_hooks(extension.name.clone(), module);
        }
        Ok(Arc::new(loaded))
    }

    fn runtime(&self) -> Result<&alien_infra::extension_bundle::Runtime, AlienError> {
        if let Some(runtime) = self.runtime.get() {
            return Ok(runtime);
        }
        let runtime =
            alien_infra::extension_bundle::Runtime::new().map_err(AlienError::into_generic)?;
        Ok(self.runtime.get_or_init(|| runtime))
    }
}

#[cfg(test)]
mod eviction_tests {
    use super::*;
    use alien_infra::extension_bundle::testing::TestBundle;

    fn opened(bytes: usize) -> Arc<OnceCell<Opened>> {
        let bundle = TestBundle::new("acme", "1.0.0", &[]);
        let cell = OnceCell::new();
        let _ = cell.set(Opened {
            bundle: Arc::new(ExtensionBundle::open(&bundle.manifest, &bundle.zip).unwrap()),
            bytes,
        });
        Arc::new(cell)
    }

    fn keys(opened: &VecDeque<(BundleKey, Arc<OnceCell<Opened>>)>) -> Vec<&str> {
        opened
            .iter()
            .map(|((_, digest), _)| digest.as_str())
            .collect()
    }

    /// The bundles used longest ago leave first, by count and by bytes, and the one just used
    /// stays even when it alone is over the bytes.
    #[test]
    fn the_least_recently_used_bundles_leave_by_count_and_bytes() {
        let mut queue: VecDeque<_> = ["a", "b", "c"]
            .into_iter()
            .map(|digest| (("p".to_string(), digest.to_string()), opened(10)))
            .collect();
        evict(&mut queue, 2, 100);
        assert_eq!(keys(&queue), ["b", "c"]);

        let mut queue: VecDeque<_> = ["a", "b", "c"]
            .into_iter()
            .map(|digest| (("p".to_string(), digest.to_string()), opened(10)))
            .collect();
        evict(&mut queue, 10, 20);
        assert_eq!(keys(&queue), ["b", "c"]);

        let mut queue: VecDeque<_> = [(("p".to_string(), "big".to_string()), opened(1000))].into();
        evict(&mut queue, 10, 20);
        assert_eq!(keys(&queue), ["big"]);
    }
}
