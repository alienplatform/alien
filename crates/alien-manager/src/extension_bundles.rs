//! The extension bundles a manager loads for the deployments it runs: each core zip fetched from
//! the [`ExtensionBundleStore`] once per project and digest, checked against the digest the stack
//! pins, and kept for the life of the process. A digest names its bytes, so a kept bundle never
//! goes stale.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, PoisonError};

use tokio::sync::OnceCell;

use alien_core::{DeploymentState, ExtensionManifest, Stack};
use alien_error::{AlienError, Context, GenericError, IntoAlienError};
use alien_infra::extension_bundle::{sha256_hex, ExtensionBundle};
use alien_infra::step_protocol::wasm::{ModuleSlot, WasmHost};
use alien_infra::step_protocol::{CredentialSource, ProxyFactory, StdioTransport};

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
    /// What a test's `module_host` gives, in place of the process's host.
    #[cfg(test)]
    module_host: Option<Result<Arc<WasmHost>, AlienError>>,
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

/// How long connecting to where a core zip downloads from, TLS included, may take. An address
/// that never answers fails within this, not at the end of [`FETCH_TIMEOUT`].
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// The object of a bundle the manager fetches, as the errors about it name it.
const CORE_OBJECT: &str = "core zip";

/// `extension`'s core zip could not be fetched now; another try may fetch it.
fn unavailable(extension: &ExtensionManifest, message: String) -> alien_infra::ErrorData {
    alien_infra::ErrorData::ExtensionBundleUnavailable {
        extension: extension.name.clone(),
        object: CORE_OBJECT.to_string(),
        message,
    }
}

/// `extension`'s core zip is not one the manager may use.
fn refused(extension: &ExtensionManifest, reason: String) -> alien_infra::ErrorData {
    alien_infra::ErrorData::ExtensionBundleRefused {
        extension: extension.name.clone(),
        object: CORE_OBJECT.to_string(),
        reason,
    }
}

impl ExtensionBundles {
    pub fn new(store: Arc<dyn ExtensionBundleStore>) -> Self {
        Self {
            store,
            local_controllers: None,
            http: reqwest::Client::builder()
                .connect_timeout(CONNECT_TIMEOUT)
                .build()
                .map_err(|error| error.to_string()),
            runtime: OnceLock::new(),
            #[cfg(test)]
            module_host: None,
            opened: std::sync::Mutex::default(),
        }
    }

    /// Steps controller modules on `host`, whose cloud calls a test routes to its own servers.
    #[cfg(test)]
    pub(crate) fn with_module_host(mut self, host: Arc<WasmHost>) -> Self {
        self.module_host = Some(Ok(host));
        self
    }

    /// Fetches with `http` instead of the client [`Self::new`] builds.
    #[cfg(test)]
    pub(crate) fn with_http(mut self, http: reqwest::Client) -> Self {
        self.http = Ok(http);
        self
    }

    /// Fails each use of the host for controller modules.
    #[cfg(test)]
    pub(crate) fn without_module_host(mut self) -> Self {
        self.module_host = Some(Err(AlienError::new(GenericError {
            message: "this test gives no host for controller modules".to_string(),
        })));
        self
    }

    fn module_host(&self) -> Result<Arc<WasmHost>, AlienError> {
        #[cfg(test)]
        if let Some(host) = &self.module_host {
            return host.clone();
        }
        WasmHost::shared().map_err(AlienError::into_generic)
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
        // A failed fetch leaves the cell empty, and the next caller queued on it fetches again.
        // So a caller waits at most one fetch's time for its turn, and a fetch, which runs in the
        // caller whose turn it is, has its full time whenever that turn comes.
        let fetching = AtomicBool::new(false);
        let init = cell.get_or_try_init(|| async {
            fetching.store(true, Ordering::Relaxed);
            let zip =
                tokio::time::timeout(FETCH_TIMEOUT, self.fetch(caller, project_id, extension))
                    .await
                    .map_err(|_| {
                        AlienError::new(unavailable(
                            extension,
                            format!(
                                "it was not fetched within {} seconds",
                                FETCH_TIMEOUT.as_secs()
                            ),
                        ))
                        .into_generic()
                    })??;
            let bundle =
                ExtensionBundle::open(extension, &zip).map_err(AlienError::into_generic)?;
            Ok::<_, AlienError>(Opened {
                bundle: Arc::new(bundle),
                bytes: zip.len(),
            })
        });
        tokio::pin!(init);
        let opened = match tokio::time::timeout(FETCH_TIMEOUT, &mut init).await {
            Ok(opened) => opened?,
            Err(_) if fetching.load(Ordering::Relaxed) => init.await?,
            Err(_) => {
                return Err(AlienError::new(unavailable(
                    extension,
                    format!(
                        "another fetch of it did not finish within {} seconds",
                        FETCH_TIMEOUT.as_secs()
                    ),
                ))
                .into_generic())
            }
        };
        let bundle = opened.bundle.clone();
        evict(
            &mut self.opened.lock().unwrap_or_else(PoisonError::into_inner),
            MAX_OPENED_BUNDLES,
            MAX_OPENED_BYTES,
        );
        // The cache is keyed by digest, and two stacks may describe one digest differently.
        bundle
            .check_stack(extension)
            .map_err(AlienError::into_generic)?;
        Ok(bundle)
    }

    async fn fetch(
        &self,
        caller: &Subject,
        project_id: &str,
        extension: &ExtensionManifest,
    ) -> Result<Vec<u8>, AlienError> {
        let sha256 = sha256_hex(&extension.digest).ok_or_else(|| {
            AlienError::new(refused(
                extension,
                format!(
                    "the stack pins digest '{}', which is not sha256:<hex>",
                    extension.digest
                ),
            ))
            .into_generic()
        })?;
        match self.store.get(caller, project_id, sha256).await? {
            Some(BundleObject::Bytes(bytes)) => Ok(bytes),
            Some(BundleObject::Url(url)) => {
                // The URL may be presigned: its query is a credential, so no error carries it.
                let http = self.http.as_ref().map_err(|error| {
                    AlienError::new(unavailable(
                        extension,
                        format!("the HTTP client could not be built: {error}"),
                    ))
                    .into_generic()
                })?;
                let response = http
                    .get(&url)
                    .send()
                    .await
                    .map_err(reqwest::Error::without_url)
                    .into_alien_error()
                    .context(unavailable(
                        extension,
                        "the download could not be made".to_string(),
                    ))
                    .map_err(AlienError::into_generic)?;
                // Another fetch asks the store again, which answers with a fresh URL.
                if !response.status().is_success() {
                    return Err(AlienError::new(unavailable(
                        extension,
                        format!("the download returned HTTP {}", response.status()),
                    ))
                    .into_generic());
                }
                // A core zip is at most one bundle object, so a longer download is refused as
                // it arrives rather than held whole.
                let limit = alien_infra::extension_bundle::MAX_OBJECT_BYTES as usize;
                let mut response = response;
                let mut zip = Vec::new();
                while let Some(chunk) = response
                    .chunk()
                    .await
                    .map_err(reqwest::Error::without_url)
                    .into_alien_error()
                    .context(unavailable(
                        extension,
                        "the download was cut short".to_string(),
                    ))
                    .map_err(AlienError::into_generic)?
                {
                    if zip.len() + chunk.len() > limit {
                        return Err(AlienError::new(refused(
                            extension,
                            format!("it is larger than the {limit} bytes a bundle object may take"),
                        ))
                        .into_generic());
                    }
                    zip.extend_from_slice(&chunk);
                }
                Ok(zip)
            }
            None => Err(AlienError::new(crate::error::ErrorData::ExtensionBundleMissing {
                message: format!(
                    "Extension '{}': its bundle {} is unavailable: this manager has no such bundle; release it again",
                    extension.name, extension.digest
                ),
            })
            .into_generic()),
        }
    }

    /// `provider` for deployment `deployment_id` of `project_id` in `state`, with the hooks
    /// module of each extension its current and target releases list, and each extension's
    /// controller: with local controllers, its process on this machine; otherwise its bundle's
    /// controller module in this process, whose cloud calls are signed with what `credentials`
    /// gives for the extension, and refused when it gives none. An extension with neither
    /// controller gets none, so stepping one of its resources fails naming it.
    pub async fn provider_for(
        &self,
        provider: alien_infra::DefaultPlatformServiceProvider,
        project_id: &str,
        deployment_id: &str,
        state: &DeploymentState,
        credentials: impl Fn(&ExtensionManifest) -> Option<Arc<dyn CredentialSource>>,
    ) -> Result<alien_infra::DefaultPlatformServiceProvider, AlienError> {
        let extensions = alien_infra::extension_bundle::release_extensions(state);
        if extensions.is_empty() {
            return Ok(provider);
        }
        let runtime = self.runtime()?;
        let mut bundles = HashMap::new();
        let mut controllers = HashMap::new();
        for extension in extensions {
            let bundle = self.open(&Subject::system(), project_id, extension).await?;
            // Compiling a module and running its manifest is work the extension's author sized,
            // so it runs off the async workers that step other deployments. The runtime keeps
            // what it compiled, so the loads below find it.
            let module = {
                let (runtime, bundle, project_id) =
                    (runtime.clone(), bundle.clone(), project_id.to_string());
                let host = self.compile_slot(&project_id, extension).await?;
                alien_infra::run_hooks(move || {
                    bundle.load_hooks(&runtime)?;
                    match host {
                        Some((host, _slot)) => bundle.wasm_transport(&runtime, &host, &project_id),
                        None => Ok(None),
                    }
                })
                .await
                .map_err(AlienError::into_generic)?
                .map_err(AlienError::into_generic)?
            };
            let controller = match &self.local_controllers {
                // `alien dev` makes no cloud call, so a step carries no credentials.
                Some(local) => local
                    .process(extension, &bundle)
                    .await?
                    .map(|process| ProxyFactory::new(deployment_id, process, None)),
                None => module.map(|module| {
                    ProxyFactory::new(deployment_id, Arc::new(module), credentials(extension))
                }),
            };
            if let Some(controller) = controller {
                controllers.insert(extension.digest.clone(), controller);
            }
            bundles.insert(extension.digest.clone(), bundle);
        }
        alien_infra::extension_bundle::provider_for(
            provider,
            state,
            runtime,
            &bundles,
            &controllers,
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
            let bundle = self.open(caller, project_id, extension).await?;
            let runtime = self.runtime()?.clone();
            let slot = self.compile_slot(project_id, extension).await?;
            // Off the async workers, as in `provider_for`.
            let module = alien_infra::run_hooks(move || {
                let _slot = slot;
                bundle.load_hooks(&runtime)
            })
            .await
            .map_err(AlienError::into_generic)?
            .map_err(AlienError::into_generic)?;
            loaded = loaded.with_hooks(extension.name.clone(), module);
        }
        Ok(Arc::new(loaded))
    }

    /// The host for controller modules and one of `project_id`'s slots on it, to compile
    /// `extension`'s modules in: a compile holds a thread and a module's memory as a step does.
    /// None with local controllers, which run as processes.
    async fn compile_slot(
        &self,
        project_id: &str,
        extension: &ExtensionManifest,
    ) -> Result<Option<(Arc<WasmHost>, ModuleSlot)>, AlienError> {
        if self.local_controllers.is_some() {
            return Ok(None);
        }
        let host = self.module_host()?;
        let slot = host
            .slot(project_id)
            .await
            .into_alien_error()
            .context(GenericError {
                message: format!(
                    "the modules of extension '{}' could not wait to compile",
                    extension.name
                ),
            })?;
        Ok(Some((host, slot)))
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

#[cfg(test)]
mod fetch_tests {
    use super::*;
    use alien_infra::extension_bundle::testing::TestBundle;
    use async_trait::async_trait;

    /// A store whose every lookup fails after `after`.
    struct SlowFailingStore {
        after: std::time::Duration,
    }

    #[async_trait]
    impl ExtensionBundleStore for SlowFailingStore {
        async fn put(&self, _: &Subject, _: &str, _: &str, _: Vec<u8>) -> Result<(), AlienError> {
            unreachable!("this test uploads nothing")
        }

        async fn get(
            &self,
            _: &Subject,
            _: &str,
            _: &str,
        ) -> Result<Option<BundleObject>, AlienError> {
            tokio::time::sleep(self.after).await;
            Err(AlienError::new(GenericError {
                message: "the store did not answer".to_string(),
            }))
        }
    }

    /// Callers queued on one bundle wait at most one fetch's time for their turn, and a caller
    /// whose turn comes then gets a fetch's full time, so a failing fetch holds no caller
    /// longer than two.
    #[tokio::test(start_paused = true)]
    async fn each_caller_of_a_failing_fetch_waits_at_most_its_turn_and_its_fetch() {
        let bundles = ExtensionBundles::new(Arc::new(SlowFailingStore {
            after: FETCH_TIMEOUT - std::time::Duration::from_secs(10),
        }));
        let extension = TestBundle::new("acme", "1.0.0", &[]).manifest;
        let started = tokio::time::Instant::now();

        let waits = futures::future::join_all((0..3).map(|_| async {
            let result = bundles
                .open(&Subject::system(), "default", &extension)
                .await;
            (result.err(), started.elapsed())
        }))
        .await;

        for (error, waited) in &waits {
            assert!(error.is_some(), "every fetch fails");
            assert!(*waited <= 2 * FETCH_TIMEOUT, "a caller waited {waited:?}");
        }
        // Two callers fetch in turn; the third never gets a turn and gives up after its wait.
        let gave_up: Vec<_> = waits
            .iter()
            .filter_map(|(error, _)| error.as_ref())
            .filter(|error| error.code == "EXTENSION_BUNDLE_UNAVAILABLE")
            .collect();
        assert_eq!(gave_up.len(), 1, "{waits:?}");
        assert!(gave_up[0].retryable, "{}", gave_up[0]);
    }

    /// A store whose first lookup never answers, and whose later ones answer with `zip` after
    /// `after`.
    struct StalledThenSlowStore {
        zip: Vec<u8>,
        after: std::time::Duration,
        lookups: std::sync::atomic::AtomicUsize,
    }

    #[async_trait]
    impl ExtensionBundleStore for StalledThenSlowStore {
        async fn put(&self, _: &Subject, _: &str, _: &str, _: Vec<u8>) -> Result<(), AlienError> {
            unreachable!("this test uploads nothing")
        }

        async fn get(
            &self,
            _: &Subject,
            _: &str,
            _: &str,
        ) -> Result<Option<BundleObject>, AlienError> {
            if self
                .lookups
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                == 0
            {
                std::future::pending::<()>().await;
            }
            tokio::time::sleep(self.after).await;
            Ok(Some(BundleObject::Bytes(self.zip.clone())))
        }
    }

    /// A caller that queued behind a fetch that stalled gets a fetch's full time once its turn
    /// comes, not what is left of its own wait.
    #[tokio::test(start_paused = true)]
    async fn a_caller_whose_turn_comes_late_gets_a_full_fetch() {
        let bundle = TestBundle::new("acme", "1.0.0", &[]);
        let bundles = ExtensionBundles::new(Arc::new(StalledThenSlowStore {
            zip: bundle.zip.clone(),
            after: std::time::Duration::from_secs(105),
            lookups: Default::default(),
        }));

        let system = Subject::system();
        let first = bundles.open(&system, "default", &bundle.manifest);
        let second = async {
            tokio::time::sleep(std::time::Duration::from_secs(100)).await;
            bundles.open(&system, "default", &bundle.manifest).await
        };
        let (first, second) = tokio::join!(first, second);

        let error = first.err().expect("the stalled fetch fails");
        assert_eq!(error.code, "EXTENSION_BUNDLE_UNAVAILABLE", "{error}");
        assert!(error.retryable, "{error}");
        second.expect("the second fetch has its full time");
    }

    /// A store that answers with a URL.
    struct UrlStore(String);

    #[async_trait]
    impl ExtensionBundleStore for UrlStore {
        async fn put(&self, _: &Subject, _: &str, _: &str, _: Vec<u8>) -> Result<(), AlienError> {
            unreachable!("this test uploads nothing")
        }

        async fn get(
            &self,
            _: &Subject,
            _: &str,
            _: &str,
        ) -> Result<Option<BundleObject>, AlienError> {
            Ok(Some(BundleObject::Url(self.0.clone())))
        }
    }

    /// A download from a server that takes the connection and never answers ends at the fetch's
    /// time, as unavailable and retryable.
    #[tokio::test(start_paused = true)]
    async fn a_download_from_a_server_that_never_answers_is_unavailable() {
        // The kernel accepts the connection into the backlog; nothing ever reads or answers it.
        let stalled = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/core.zip", stalled.local_addr().unwrap());
        // The paused clock would jump past the connect timeout before the connection is made;
        // this test is about the fetch's timeout once it is.
        let bundles =
            ExtensionBundles::new(Arc::new(UrlStore(url))).with_http(reqwest::Client::new());
        let extension = TestBundle::new("acme", "1.0.0", &[]).manifest;
        let started = tokio::time::Instant::now();

        let error = bundles
            .open(&Subject::system(), "default", &extension)
            .await
            .err()
            .expect("a download with no answer fails");

        assert_eq!(error.code, "EXTENSION_BUNDLE_UNAVAILABLE", "{error}");
        assert!(error.retryable, "{error}");
        assert!(error.to_string().contains("not fetched within"), "{error}");
        assert!(
            started.elapsed() >= FETCH_TIMEOUT
                && started.elapsed() <= FETCH_TIMEOUT + std::time::Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
        drop(stalled);
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;
    use alien_infra::extension_bundle::testing::TestBundle;
    use async_trait::async_trait;

    struct BytesStore(Vec<u8>);

    #[async_trait]
    impl ExtensionBundleStore for BytesStore {
        async fn put(&self, _: &Subject, _: &str, _: &str, _: Vec<u8>) -> Result<(), AlienError> {
            unreachable!("this test uploads nothing")
        }

        async fn get(
            &self,
            _: &Subject,
            _: &str,
            _: &str,
        ) -> Result<Option<BundleObject>, AlienError> {
            Ok(Some(BundleObject::Bytes(self.0.clone())))
        }
    }

    /// A bundle one stack opened is refused to a stack that pins its digest under another name
    /// or version, or with other egress, as a manager that never opened it refuses it, and
    /// still opens for the first stack.
    #[tokio::test]
    async fn each_stack_that_pins_an_opened_bundle_is_checked_against_it() {
        let bundle = TestBundle::with_controller_module(
            "acme",
            "1.0.0",
            b"module",
            &["https://api.acme.example"],
        );
        let system = Subject::system();
        let (mut renamed, mut other_version, mut other_egress) = (
            bundle.manifest.clone(),
            bundle.manifest.clone(),
            bundle.manifest.clone(),
        );
        renamed.name = "other".to_string();
        other_version.version = "1.0.1".to_string();
        other_egress.egress = Vec::new();
        let bundles = ExtensionBundles::new(Arc::new(BytesStore(bundle.zip.clone())));
        bundles
            .open(&system, "default", &bundle.manifest)
            .await
            .expect("the bundle opens for the stack that describes it");

        for other in [&renamed, &other_version, &other_egress] {
            let cold = ExtensionBundles::new(Arc::new(BytesStore(bundle.zip.clone())))
                .open(&system, "default", other)
                .await
                .err()
                .expect("a manager that never opened the bundle refuses it");
            let refused = bundles
                .open(&system, "default", other)
                .await
                .err()
                .expect("a manager that opened the bundle for another stack refuses it");
            assert_eq!(refused.code, "EXTENSION_BUNDLE_REFUSED", "{refused}");
            assert_eq!(refused.code, cold.code, "{refused} / {cold}");
        }
        bundles
            .open(&system, "default", &bundle.manifest)
            .await
            .expect("the bundle still opens for the stack that describes it");
    }

    /// A controller module compiles in one of its project's slots: with the project's slots
    /// held, a deployment of the project waits to build its provider, and a deployment of
    /// another project does not.
    #[tokio::test]
    async fn a_controller_module_compiles_in_one_of_its_project_s_slots() {
        let bundle = TestBundle::with_controller_module("acme", "1.0.0", b"\0asm", &[]);
        // Two process slots: the held one and the other project's compile.
        let host = Arc::new(
            WasmHost::new()
                .unwrap()
                .with_steps_per_project(1)
                .with_steps_per_process(2),
        );
        let bundles = Arc::new(
            ExtensionBundles::new(Arc::new(BytesStore(bundle.zip.clone())))
                .with_module_host(host.clone()),
        );
        let state: DeploymentState = serde_json::from_value(serde_json::json!({
            "platform": "aws", "status": "running",
            "protocolVersion": alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            "currentRelease": {"releaseId": "rel", "stack": {"id": "app", "resources": {}, "extensions": [bundle.manifest]}}
        }))
        .unwrap();
        let provider_of = |project: &'static str| {
            let (bundles, state) = (bundles.clone(), state.clone());
            tokio::spawn(async move {
                // The module does not compile; the provider fails once it tried.
                let error = bundles
                    .provider_for(
                        alien_infra::DefaultPlatformServiceProvider::default(),
                        project,
                        "dep_1",
                        &state,
                        |_| None,
                    )
                    .await
                    .expect_err("a module that does not compile");
                assert_eq!(error.code, "EXTENSION_BUNDLE_REFUSED", "{error}");
            })
        };

        let held = host.slot("proj_busy").await.unwrap();
        let waiting = provider_of("proj_busy");
        let other = provider_of("proj_other");
        tokio::time::timeout(std::time::Duration::from_secs(30), other)
            .await
            .expect("another project compiles while this one's slots are held")
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert!(
            !waiting.is_finished(),
            "a compile ran past its project's limit"
        );

        drop(held);
        tokio::time::timeout(std::time::Duration::from_secs(30), waiting)
            .await
            .expect("the compile runs once a slot of its project is free")
            .unwrap();
        // Both process slots and the project's one are free only if every failed compile gave
        // back its project's slot and the process's.
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            (
                host.slot("proj_busy").await.unwrap(),
                host.slot("proj_other").await.unwrap(),
            )
        })
        .await
        .expect("a failed compile gives its slots back");
    }

    /// The hooks modules a chart route loads compile in one of the project's slots too.
    #[tokio::test]
    async fn a_hooks_module_compiles_in_one_of_its_project_s_slots() {
        let bundle = TestBundle::new("acme", "1.0.0", &[]);
        let host = Arc::new(WasmHost::new().unwrap().with_steps_per_project(1));
        let bundles = Arc::new(
            ExtensionBundles::new(Arc::new(BytesStore(bundle.zip.clone())))
                .with_module_host(host.clone()),
        );
        let stack: Stack = serde_json::from_value(serde_json::json!({
            "id": "app", "resources": {}, "extensions": [bundle.manifest]
        }))
        .unwrap();
        let loaded_for = |project: &'static str| {
            let (bundles, stack) = (bundles.clone(), stack.clone());
            tokio::spawn(async move {
                bundles
                    .loaded_extensions(&Subject::system(), project, &stack)
                    .await
                    .map(|_| ())
            })
        };

        let held = host.slot("proj_busy").await.unwrap();
        let waiting = loaded_for("proj_busy");
        tokio::time::timeout(std::time::Duration::from_secs(30), loaded_for("proj_other"))
            .await
            .expect("another project loads while this one's slots are held")
            .unwrap()
            .expect("the hooks load");
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert!(
            !waiting.is_finished(),
            "a load ran past its project's limit"
        );

        drop(held);
        tokio::time::timeout(std::time::Duration::from_secs(30), waiting)
            .await
            .expect("the load runs once a slot of its project is free")
            .unwrap()
            .expect("the hooks load");
    }

    /// `alien dev` runs each controller as a process, so its manager builds a deployment's
    /// provider without the host for controller modules.
    #[tokio::test]
    async fn local_controllers_need_no_host_for_controller_modules() {
        let bundle = TestBundle::new("acme", "1.0.0", &[]);
        let dir = tempfile::tempdir().unwrap();
        let bundles = ExtensionBundles::new(Arc::new(BytesStore(bundle.zip.clone())))
            .with_local_controllers(LocalControllers::new(
                Arc::new(|_: &str, _: &str| Ok(None)),
                Vec::new(),
                dir.path().join("copies"),
            ))
            .without_module_host();
        let state: DeploymentState = serde_json::from_value(serde_json::json!({
            "platform": "local", "status": "running",
            "protocolVersion": alien_core::CURRENT_DEPLOYMENT_PROTOCOL_VERSION,
            "currentRelease": {"releaseId": "rel", "stack": {"id": "app", "resources": {}, "extensions": [bundle.manifest]}}
        }))
        .unwrap();

        bundles
            .provider_for(
                alien_infra::DefaultPlatformServiceProvider::default(),
                "default",
                "dep_1",
                &state,
                |_| None,
            )
            .await
            .expect("a provider with local controllers");
    }
}
