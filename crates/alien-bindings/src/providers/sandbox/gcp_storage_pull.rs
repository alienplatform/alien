//! Persistent storage on GCP: the binding pulls the agent's spool through the `:execute` proxy,
//! because a sandbox in a PSC tenant has no route to Cloud Storage.
//!
//! Each [`PullChannel`] call is one `:execute` request carrying one agent envelope op
//! (`storagePending`, `storageRead`, `storageAck`, `storageSeed`). Reads and seed pages are at
//! most [`GCP_STORAGE_PAGE_BYTES`] raw bytes, the same page as `readFile`, so a reply stays under
//! the proxy's 2 MB limit after both base64 layers.
//!
//! The binding claims a key for a new sandbox, seeds the claimed commit into the agent's spool,
//! attaches, and then pulls every [`PULL_INTERVAL`] for as long as it holds the sandbox. A binding
//! that starts again (a new workload process) resumes pulling a sandbox the first time it touches
//! it, if HEAD still names that sandbox as the writer. While no binding pulls, commits wait in the
//! sandbox's spool; the latest one is published with the next pull.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use alien_core::bindings::SandboxPersistentStorageBinding;
use alien_error::{AlienError, Context, ContextError as _};
use alien_gcp_clients::gcp::agent_platform::AgentPlatformApi;
use alien_sandbox_storage::{
    AckOutcome, ErrorData as StorageError, ObjectName, Pending, PullChannel, PullOutcome, Puller,
    Result as StorageResult,
};
use async_trait::async_trait;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::json;
use tracing::{debug, warn};

use super::{
    is_not_found, GcpAgentPlatformSandbox, AGENT_PROTOCOL_VERSION, BASE64, GCP_READ_PAGE_BYTES,
};
use crate::error::{ErrorData, Result};
use crate::providers::sandbox::persistent_store::KeyStore;
use crate::traits::{SandboxStorageState, SandboxStorageStatus};

/// Raw bytes one storage read or seed page carries over `:execute`.
pub const GCP_STORAGE_PAGE_BYTES: u64 = GCP_READ_PAGE_BYTES as u64;

/// How often the binding asks a sandbox for commits. The agent commits at most once a second
/// after a change, so a write is published about two seconds after it lands.
#[cfg(not(test))]
pub const PULL_INTERVAL: Duration = Duration::from_secs(1);
#[cfg(test)]
pub const PULL_INTERVAL: Duration = Duration::from_millis(20);

/// How long a commit may wait in the sandbox, unpublished, before the status says `behind`: a
/// commit after a quiet second plus a pull every second, with room for a slow pull.
const BEHIND_AFTER: Duration = Duration::from_secs(10);

/// Pulls a `sync` makes before it gives up on its commit reaching the store.
const SYNC_PULL_ATTEMPTS: u32 = 30;

/// One sandbox's agent, as the pull transport reaches it through `:execute`. Owns what it needs,
/// so a pull loop can outlive the call that started it.
#[derive(Clone)]
pub struct AgentPlatformStorageChannel {
    client: Arc<dyn AgentPlatformApi>,
    engine: String,
    sandbox_id: String,
}

impl GcpAgentPlatformSandbox {
    /// The pull channel to `sandbox_id`'s agent.
    pub fn storage_channel(&self, sandbox_id: &str) -> Result<AgentPlatformStorageChannel> {
        Self::checked_sandbox_id("sandbox.storage", sandbox_id)?;
        Ok(AgentPlatformStorageChannel {
            client: Arc::clone(&self.client),
            engine: self.engine.clone(),
            sandbox_id: sandbox_id.to_string(),
        })
    }
}

fn failed(operation: &str, object: &str, reason: impl Into<String>) -> AlienError<StorageError> {
    AlienError::new(StorageError::StoreFailed {
        operation: operation.to_string(),
        object: object.to_string(),
        reason: reason.into(),
    })
}

impl AgentPlatformStorageChannel {
    async fn execute(&self, op: &str, mut envelope: serde_json::Value) -> Result<Vec<u8>> {
        envelope["v"] = json!(AGENT_PROTOCOL_VERSION);
        envelope["op"] = json!(op);
        let body = serde_json::to_vec(&envelope).map_err(|error| {
            AlienError::new(ErrorData::SerializationFailed {
                message: format!("could not encode the {op} envelope: {error}"),
            })
        })?;
        self.client
            .execute(&self.engine, &self.sandbox_id, &body)
            .await
            .map_err(|error| GcpAgentPlatformSandbox::execute_failed(op, error))
    }

    async fn call(
        &self,
        op: &str,
        object: &str,
        envelope: serde_json::Value,
    ) -> StorageResult<Vec<u8>> {
        // The cause's message goes into the reason: a caller that sees only the top message (the
        // Node addon flattens the chain) must still tell an agent's refusal from a lost call.
        match self.execute(op, envelope).await {
            Ok(body) => Ok(body),
            Err(error) => {
                let reason = format!("the :execute call failed: {}", error.message);
                Err(error.context(StorageError::StoreFailed {
                    operation: op.to_string(),
                    object: object.to_string(),
                    reason,
                }))
            }
        }
    }

    /// An op that answers `{}`; anything else is the agent's refusal.
    fn confirm(op: &str, object: &str, body: &[u8]) -> StorageResult<()> {
        match serde_json::from_slice::<serde_json::Value>(body) {
            Ok(serde_json::Value::Object(map)) if map.is_empty() => Ok(()),
            _ => Err(failed(op, object, super::truncated(body))),
        }
    }

    /// A storage op that answers the agent's storage status.
    async fn status_op(
        &self,
        op: &str,
        operation: &str,
        envelope: serde_json::Value,
    ) -> Result<SandboxStorageStatus> {
        let body = self.execute(op, envelope).await?;
        serde_json::from_slice(&body).map_err(|_| {
            AlienError::new(ErrorData::SandboxCommandFailed {
                failure: "storage".to_string(),
                reason: format!("{operation}: {}", super::truncated(&body)),
            })
        })
    }
}

#[derive(Deserialize)]
struct PendingReply {
    pending: Option<Pending>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadReply {
    contents_base64: String,
}

#[async_trait]
impl PullChannel for AgentPlatformStorageChannel {
    async fn pending(&self) -> StorageResult<Option<Pending>> {
        let body = self.call("storagePending", "", json!({})).await?;
        let reply: PendingReply = serde_json::from_slice(&body)
            .map_err(|_| failed("storagePending", "", super::truncated(&body)))?;
        Ok(reply.pending)
    }

    async fn read(&self, object: &str, offset: u64, len: u64) -> StorageResult<Vec<u8>> {
        let length = len.min(GCP_STORAGE_PAGE_BYTES);
        let body = self
            .call(
                "storageRead",
                object,
                json!({ "object": object, "offset": offset, "length": length }),
            )
            .await?;
        let reply: ReadReply = serde_json::from_slice(&body)
            .map_err(|_| failed("storageRead", object, super::truncated(&body)))?;
        let bytes = BASE64
            .decode(reply.contents_base64)
            .map_err(|e| failed("storageRead", object, format!("not base64: {e}")))?;
        if bytes.len() as u64 > length {
            return Err(failed(
                "storageRead",
                object,
                format!("the agent answered {} bytes for {length}", bytes.len()),
            ));
        }
        Ok(bytes)
    }

    async fn ack(&self, generation: u64, outcome: AckOutcome) -> StorageResult<()> {
        let body = self
            .call(
                "storageAck",
                "HEAD",
                json!({ "generation": generation, "outcome": outcome }),
            )
            .await?;
        Self::confirm("storageAck", "HEAD", &body)
    }

    async fn seed(
        &self,
        object: &str,
        offset: u64,
        bytes: Vec<u8>,
        last: bool,
    ) -> StorageResult<()> {
        if bytes.len() as u64 > GCP_STORAGE_PAGE_BYTES {
            return Err(failed(
                "storageSeed",
                object,
                format!("a page carries at most {GCP_STORAGE_PAGE_BYTES} bytes"),
            ));
        }
        let body = self
            .call(
                "storageSeed",
                object,
                json!({
                    "object": object,
                    "offset": offset,
                    "contentsBase64": BASE64.encode(bytes),
                    "last": last,
                }),
            )
            .await?;
        Self::confirm("storageSeed", object, &body)
    }
}

/// A declaration's persistent store (a Cloud Storage bucket setup created) and the pulls this
/// binding runs for its sandboxes.
pub struct GcpPersistentStore {
    storage: SandboxPersistentStorageBinding,
    inner: Arc<dyn object_store::ObjectStore>,
    prefix: String,
    pulls: Mutex<HashMap<String, Arc<SandboxPull>>>,
}

/// One sandbox's pull: the key it holds, its puller (one pull at a time, whether the loop or a
/// `sync` runs it) and what the last pull reported.
struct SandboxPull {
    key: String,
    puller: tokio::sync::Mutex<Puller>,
    last: Mutex<PullReport>,
    task: Mutex<Option<tokio::task::AbortHandle>>,
}

#[derive(Default, Clone)]
struct PullReport {
    error: Option<String>,
    superseded: bool,
}

impl std::fmt::Debug for GcpPersistentStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GcpPersistentStore")
            .field("storage", &self.storage)
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}

impl Drop for GcpPersistentStore {
    fn drop(&mut self) {
        for pull in self.pulls.get_mut().expect("pull map lock").values() {
            pull.abort();
        }
    }
}

impl SandboxPull {
    fn abort(&self) {
        if let Some(task) = self.task.lock().expect("pull task lock").take() {
            task.abort();
        }
    }

    /// One pull, recorded for `storage_status`.
    async fn pull(
        &self,
        channel: &AgentPlatformStorageChannel,
        store: &KeyStore,
    ) -> StorageResult<PullOutcome> {
        let outcome = self
            .puller
            .lock()
            .await
            .pull(channel, store, &self.key)
            .await;
        let mut last = self.last.lock().expect("pull report lock");
        match &outcome {
            Ok(_) => last.error = None,
            Err(error) if matches!(error.error, Some(StorageError::Superseded { .. })) => {
                last.superseded = true;
                last.error = Some(error.to_string());
            }
            Err(error) => last.error = Some(error.to_string()),
        }
        outcome
    }
}

impl GcpPersistentStore {
    /// The store for `storage`, whose locator must be a Cloud Storage bucket, reached through
    /// `inner` (a client for that bucket).
    pub fn new(
        storage: SandboxPersistentStorageBinding,
        inner: Arc<dyn object_store::ObjectStore>,
        prefix: String,
    ) -> Self {
        Self {
            storage,
            inner,
            prefix,
            pulls: Mutex::new(HashMap::new()),
        }
    }

    /// The declaration.
    pub fn declaration(&self) -> &SandboxPersistentStorageBinding {
        &self.storage
    }

    fn key_store(&self, key: &str, operation: &str) -> Result<KeyStore> {
        KeyStore::new(Arc::clone(&self.inner), &self.prefix, key).map_err(|e| {
            AlienError::new(ErrorData::InvalidInput {
                operation_context: operation.to_string(),
                details: e.to_string(),
                field_name: Some("storageKey".to_string()),
            })
        })
    }

    fn pull_of(&self, sandbox_id: &str) -> Option<Arc<SandboxPull>> {
        self.pulls
            .lock()
            .expect("pull map lock")
            .get(sandbox_id)
            .cloned()
    }

    /// Stops pulling `sandbox_id`.
    fn forget(&self, sandbox_id: &str) {
        if let Some(pull) = self.pulls.lock().expect("pull map lock").remove(sandbox_id) {
            pull.abort();
        }
    }

    /// Starts the loop that pulls `channel`'s sandbox into `key` every [`PULL_INTERVAL`] until the
    /// sandbox is gone or another sandbox claims the key. A sandbox already pulled keeps its loop.
    fn start(&self, channel: AgentPlatformStorageChannel, key: &str, puller: Puller) -> Result<()> {
        let store = self.key_store(key, "sandbox.storage")?;
        let sandbox_id = channel.sandbox_id.clone();
        let pull = {
            let mut pulls = self.pulls.lock().expect("pull map lock");
            if pulls.contains_key(&sandbox_id) {
                return Ok(());
            }
            let pull = Arc::new(SandboxPull {
                key: key.to_string(),
                puller: tokio::sync::Mutex::new(puller),
                last: Mutex::new(PullReport::default()),
                task: Mutex::new(None),
            });
            pulls.insert(sandbox_id.clone(), Arc::clone(&pull));
            pull
        };
        let looping = Arc::clone(&pull);
        let task = tokio::spawn(async move {
            let mut tick = tokio::time::interval(PULL_INTERVAL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                match looping.pull(&channel, &store).await {
                    Ok(PullOutcome::Idle) => {}
                    Ok(PullOutcome::Published { commit, bytes }) => {
                        debug!(sandbox = %channel.sandbox_id, seq = commit.map(|c| c.seq), bytes, "persistent storage published");
                    }
                    Err(error) if matches!(error.error, Some(StorageError::Superseded { .. })) => {
                        warn!(sandbox = %channel.sandbox_id, key = %looping.key, "another sandbox claimed the key; pulling stopped");
                        return;
                    }
                    Err(error) => {
                        // A failed pull is retried, unless the sandbox is gone.
                        if let Err(read) = channel
                            .client
                            .get_sandbox(&channel.engine, &channel.sandbox_id)
                            .await
                        {
                            if is_not_found(&read) {
                                debug!(sandbox = %channel.sandbox_id, "sandbox gone; pulling stopped");
                                return;
                            }
                        }
                        warn!(sandbox = %channel.sandbox_id, %error, "persistent storage pull failed; retrying");
                    }
                }
            }
        });
        *pull.task.lock().expect("pull task lock") = Some(task.abort_handle());
        Ok(())
    }
}

impl GcpAgentPlatformSandbox {
    /// Supplies the declaration's persistent store.
    pub fn with_persistent_store(mut self, store: Option<GcpPersistentStore>) -> Self {
        self.persistent = store.map(Arc::new);
        self
    }

    pub(super) fn persistent_declaration(&self) -> Option<&SandboxPersistentStorageBinding> {
        self.persistent
            .as_deref()
            .map(GcpPersistentStore::declaration)
    }

    fn persistent_store(&self, operation: &str) -> Result<&GcpPersistentStore> {
        self.persistent.as_deref().ok_or_else(|| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: operation.to_string(),
                reason: "this sandbox declares no persistent storage".to_string(),
            })
        })
    }

    /// Claims `key` for the new sandbox, copies the claimed commit into its agent's spool,
    /// attaches (the agent restores and opens the image gate), and starts pulling. The claim
    /// comes first, so a sandbox still running for the key can no longer publish over the tree
    /// this one restores.
    pub(super) async fn attach_storage(&self, sandbox_id: &str, key: &str) -> Result<()> {
        let unreachable = |reason: String| {
            AlienError::new(ErrorData::SandboxUnreachable {
                operation: "sandbox.create".to_string(),
                reason,
            })
        };
        let persistent = self.persistent_store("sandbox.create")?;
        let store = persistent.key_store(key, "sandbox.create")?;
        let claim = alien_sandbox_storage::takeover(&store, sandbox_id)
            .await
            .map_err(|e| unreachable(format!("could not claim storage key '{key}': {e}")))?;
        if let Err(error) = alien_sandbox_storage::collect_garbage(&store).await {
            warn!(%error, key, "persistent storage garbage collection failed");
        }
        let commit: Option<ObjectName> = alien_sandbox_storage::read_head(&store)
            .await
            .map_err(|e| unreachable(format!("could not read storage key '{key}': {e}")))?
            .and_then(|(head, _)| head.commit);
        let channel = self.storage_channel(sandbox_id)?;
        if let Some(commit) = &commit {
            alien_sandbox_storage::seed(&channel, &store, commit, GCP_STORAGE_PAGE_BYTES)
                .await
                .map_err(|e| unreachable(format!("could not seed storage key '{key}': {e}")))?;
        }
        channel
            .status_op(
                "storageAttach",
                "sandbox.create",
                json!({ "attach": {
                    "key": key,
                    "writer": sandbox_id,
                    "headVersion": claim.head_version,
                    "commit": commit,
                }}),
            )
            .await
            .map_err(|e| unreachable(format!("the agent refused persistent storage: {e}")))?;
        persistent.start(
            channel,
            key,
            Puller::new(sandbox_id, claim.head_version, GCP_STORAGE_PAGE_BYTES),
        )
    }

    /// Pulls `sandbox_id` again after this binding started anew: the agent names its key, and
    /// HEAD still naming this sandbox as the writer says nothing claimed the key since. Returns
    /// whether the sandbox is pulled now.
    pub(super) async fn resume_pull(&self, sandbox_id: &str) -> Result<bool> {
        let Some(persistent) = self.persistent.as_deref() else {
            return Ok(false);
        };
        if persistent.pull_of(sandbox_id).is_some() {
            return Ok(true);
        }
        let channel = self.storage_channel(sandbox_id)?;
        let status = channel
            .status_op("storageStatus", "sandbox.persistentStorage", json!({}))
            .await?;
        let Some(key) = status.key else {
            return Ok(false);
        };
        let store = persistent.key_store(&key, "sandbox.persistentStorage")?;
        let Some((head, version)) = alien_sandbox_storage::read_head(&store)
            .await
            .map_err(|e| storage_failed("sandbox.persistentStorage", e))?
        else {
            return Ok(false);
        };
        if head.writer != sandbox_id {
            return Ok(false);
        }
        persistent.start(
            channel,
            &key,
            Puller::new(sandbox_id, version, GCP_STORAGE_PAGE_BYTES),
        )?;
        Ok(true)
    }

    /// The agent's view, with `head` and `lastSyncedAt` taken from the store: on GCP a commit is
    /// durable only once the binding published it.
    pub(super) async fn pulled_status(&self, sandbox_id: &str) -> Result<SandboxStorageStatus> {
        let persistent = self.persistent_store("sandbox.persistentStorage")?;
        self.resume_pull(sandbox_id).await?;
        let channel = self.storage_channel(sandbox_id)?;
        let mut status = channel
            .status_op("storageStatus", "sandbox.persistentStorage", json!({}))
            .await?;
        let Some(key) = status.key.clone() else {
            return Ok(status);
        };
        let store = persistent.key_store(&key, "sandbox.persistentStorage")?;
        let head = alien_sandbox_storage::read_head(&store)
            .await
            .map_err(|e| storage_failed("sandbox.persistentStorage", e))?;
        match head {
            Some((head, _)) if head.writer == sandbox_id => {
                status.head = head.commit.map(|commit| commit.seq);
                status.last_synced_at = Some(head.updated_at);
            }
            _ => {
                status.state = SandboxStorageState::Stopped;
                status.last_error = Some(format!("another sandbox claimed storage key '{key}'"));
                return Ok(status);
            }
        }
        // What the sandbox committed that no pull has published yet. Not durable: on GCP a write
        // is published only while a binding pulls (a running workload), so a sandbox left
        // unpulled longer than a commit takes reports `behind` and how much is waiting.
        let pending = channel
            .pending()
            .await
            .map_err(|e| storage_failed("sandbox.persistentStorage", e))?;
        let (pending_bytes, behind) = unpublished(pending.as_ref(), super::wall_now());
        status.pending_bytes = Some(pending_bytes);
        if behind && status.state == SandboxStorageState::Replicating {
            status.state = SandboxStorageState::Behind;
            status.last_error = Some(format!(
                "{pending_bytes} bytes committed in the sandbox are not in the store yet; on GCP \
                 writes are published while a workload with this binding runs"
            ));
        }
        if let Some(pull) = persistent.pull_of(sandbox_id) {
            let last = pull.last.lock().expect("pull report lock").clone();
            if last.superseded {
                status.state = SandboxStorageState::Stopped;
                status.last_error = last.error;
            } else if let Some(error) = last.error {
                if status.state != SandboxStorageState::Stopped {
                    status.state = SandboxStorageState::Behind;
                    status.last_error = Some(error);
                }
            }
        }
        Ok(status)
    }

    /// Commits in the sandbox, then publishes until nothing is pending, so every write made before
    /// the call is in the store when it returns.
    pub(super) async fn pull_sync(&self, sandbox_id: &str) -> Result<SandboxStorageStatus> {
        let persistent = self.persistent_store("sandbox.sync")?;
        if !self.resume_pull(sandbox_id).await? {
            return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                failure: "storage".to_string(),
                reason: format!(
                    "sandbox '{sandbox_id}' holds no storage key this binding can publish"
                ),
            }));
        }
        let channel = self.storage_channel(sandbox_id)?;
        let synced = channel
            .status_op("storageSync", "sandbox.sync", json!({}))
            .await?;
        let pull = persistent
            .pull_of(sandbox_id)
            .expect("resume_pull registered the pull");
        let store = persistent.key_store(&pull.key, "sandbox.sync")?;
        // Done once HEAD names the sync's commit or a later one. The agent commits at most once a
        // second, so a sandbox that keeps writing cannot keep this going: the first publish after
        // the sync carries a commit at least as new. An idle pull means another puller (the loop,
        // or another binding instance) may have published it; the store says whether it did.
        let reached = |seq: Option<u64>| seq >= synced.head;
        let mut attempts = 0;
        loop {
            attempts += 1;
            match pull.pull(&channel, &store).await {
                Ok(PullOutcome::Published { commit, .. })
                    if reached(commit.as_ref().map(|c| c.seq)) =>
                {
                    break
                }
                Ok(PullOutcome::Published { .. }) => {}
                Ok(PullOutcome::Idle) => {
                    let head = alien_sandbox_storage::read_head(&store)
                        .await
                        .map_err(|e| storage_failed("sandbox.sync", e))?;
                    if let Some((head, _)) = head {
                        if head.writer == sandbox_id && reached(head.commit.as_ref().map(|c| c.seq))
                        {
                            break;
                        }
                    }
                    tokio::time::sleep(PULL_INTERVAL).await;
                }
                Err(error) => return Err(storage_failed("sandbox.sync", error)),
            }
            if attempts >= SYNC_PULL_ATTEMPTS {
                return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                    failure: "storage".to_string(),
                    reason: format!(
                        "sandbox.sync: the commit did not reach the store after {attempts} pulls"
                    ),
                }));
            }
        }
        self.pulled_status(sandbox_id).await
    }

    /// Stops pulling a sandbox that is gone.
    pub(super) fn forget_pull(&self, sandbox_id: &str) {
        if let Some(persistent) = self.persistent.as_deref() {
            persistent.forget(sandbox_id);
        }
    }

    pub(super) fn storage_key_store(&self, key: &str, operation: &str) -> Result<KeyStore> {
        self.persistent_store(operation)?.key_store(key, operation)
    }
}

/// The bytes a sandbox committed that are not in the store, and whether they have waited longer
/// than [`BEHIND_AFTER`] since the oldest unpublished commit (an older agent names only its
/// latest commit's time). The sandbox's clock and this one may differ a little; the threshold is
/// far above that.
fn unpublished(pending: Option<&Pending>, now: chrono::DateTime<chrono::Utc>) -> (u64, bool) {
    let Some(pending) = pending else {
        return (0, false);
    };
    let bytes = pending.objects.iter().map(|o| o.size).sum();
    let committed_at = pending.since.or_else(|| {
        serde_json::from_str::<alien_sandbox_storage::Head>(&pending.head)
            .ok()
            .map(|head| head.updated_at)
    });
    let behind = committed_at.is_none_or(|at| {
        now.signed_duration_since(at)
            > chrono::Duration::from_std(BEHIND_AFTER).expect("a small duration")
    });
    (bytes, behind)
}

fn storage_failed(operation: &str, error: AlienError<StorageError>) -> AlienError<ErrorData> {
    AlienError::new(ErrorData::SandboxCommandFailed {
        failure: "storage".to_string(),
        reason: format!("{operation}: {error}"),
    })
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::net::SocketAddr;
    use std::sync::Arc;

    use alien_core::sandbox_capability::{
        SandboxCapabilityClaims, SandboxOperationClass, SandboxSessionIdentity,
    };
    use alien_error::ContextError as _;
    use alien_gcp_clients::gcp::agent_platform::MockAgentPlatformApi;
    use alien_sandbox_agent::exec::ExecIdentity;
    use alien_sandbox_agent::server::{router, AgentAuthorization, AgentState};
    use alien_sandbox_storage::{
        read_head, restore, seed, takeover, Committer, Entry, FileSource, MemoryStore, PullOutcome,
        Puller, Spool, TreeEntry, TreeEntryKind, TreeSink,
    };
    use ed25519_compact::{KeyPair, Seed};

    use super::*;

    /// The agents run on a runtime of their own. The mocked client runs its closures under a
    /// lock and blocks on the agent's answer; with the agents on the test's runtime, the binding's
    /// pull loop calling `:execute` meanwhile could take every worker and the agent would never
    /// answer.
    fn agent_runtime() -> &'static tokio::runtime::Runtime {
        static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
        RUNTIME.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(4)
                .enable_all()
                .build()
                .unwrap()
        })
    }

    /// One `:execute` delivery: the envelope posted to the agent's root route, its body back.
    fn post_to_agent(url: String, token: String, input: Vec<u8>) -> bytes::Bytes {
        let (tx, rx) = std::sync::mpsc::channel();
        agent_runtime().spawn(async move {
            let body = reqwest::Client::new()
                .post(url)
                .bearer_auth(token)
                .header("content-type", "application/json")
                .body(input)
                .send()
                .await
                .unwrap()
                .bytes()
                .await
                .unwrap();
            let _ = tx.send(body);
        });
        tokio::task::block_in_place(|| rx.recv().unwrap())
    }

    /// A real agent on loopback whose sandbox declares persistent storage, with its spool.
    struct Agent {
        url: String,
        token: String,
        spool: Arc<Spool>,
        workspace: std::path::PathBuf,
        _root: tempfile::TempDir,
    }

    async fn agent() -> Agent {
        let root = tempfile::TempDir::new().unwrap();
        let keys = KeyPair::from_seed(Seed::new([9u8; 32]));
        let storage = Arc::new(
            alien_sandbox_agent::storage::attach::PersistentStorage::with_staging(
                alien_sandbox_agent::storage::attach::StorageConfig {
                    path: root.path().join("workspace").display().to_string(),
                    exclude: vec![],
                    max_size_bytes: None,
                },
                root.path().join("staging"),
            ),
        );
        let spool = storage.spool().await.unwrap();
        let state = Arc::new(AgentState {
            image_process: alien_sandbox_agent::image_command::ImageProcess::not_configured(),
            #[cfg(target_os = "linux")]
            gateway_egress: None,
            session_root: root.path().canonicalize().unwrap(),
            authorization: AgentAuthorization::Capability {
                public_key: keys.pk,
                identity: SandboxSessionIdentity {
                    session_id: "s".to_string(),
                    generation: 1,
                },
            },
            exec_identity: {
                use std::os::unix::fs::MetadataExt;
                let metadata = std::fs::metadata(root.path()).unwrap();
                ExecIdentity {
                    uid: metadata.uid(),
                    gid: metadata.gid(),
                }
            },
            preview: Default::default(),
            output_cap: 1 << 20,
            jobs: Default::default(),
            interactive: Default::default(),
            agent_binary: std::path::PathBuf::from("/bin/false"),
            storage: Some(storage),
        });
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        agent_runtime().spawn(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            axum::serve(
                listener,
                router(state).into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });
        let token = alien_core::sandbox_capability_token::mint(
            &SandboxCapabilityClaims {
                session_id: "s".to_string(),
                operation: SandboxOperationClass::Execute,
                generation: 1,
                expires_at: chrono::Utc::now().timestamp() + 300,
                key_id: "k1".to_string(),
                port: None,
            },
            &keys.sk,
        )
        .unwrap();
        Agent {
            url,
            token,
            spool,
            workspace: root.path().join("workspace"),
            _root: root,
        }
    }

    /// `:execute` as the proxy behaves: the envelope reaches the agent's root route verbatim and
    /// the agent's body comes back; a reply above 2 MB is refused as Google refuses it.
    fn through_execute(
        agent: &Agent,
        envelopes: Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
    ) -> GcpAgentPlatformSandbox {
        let mut client = MockAgentPlatformApi::new();
        let url = agent.url.clone();
        let token = agent.token.clone();
        client.expect_execute().returning(move |_, _, input| {
            envelopes
                .lock()
                .unwrap()
                .push(serde_json::from_slice(input).unwrap());
            let (url, token, input) = (url.clone(), token.clone(), input.to_vec());
            let body = post_to_agent(url, token, input);
            // The reply carries the agent's body base64-encoded once more.
            assert!(
                BASE64.encode(&body).len() < 2_000_000,
                "a reply of {} bytes would be refused by :execute",
                body.len()
            );
            Ok(body.to_vec())
        });
        GcpAgentPlatformSandbox::new(
            Arc::new(client),
            "projects/p/locations/us-central1/reasoningEngines/e".to_string(),
            "projects/p/locations/us-central1/sandboxTemplates/t".to_string(),
            Some(3600),
        )
    }

    struct Bytes(Vec<u8>);
    impl FileSource for Bytes {
        fn open(&mut self, _: &str) -> StorageResult<Box<dyn std::io::Read + Send + '_>> {
            Ok(Box::new(&self.0[..]))
        }
    }

    struct Collect(Vec<u8>);
    impl TreeSink for Collect {
        fn dir(&mut self, _: &Entry) -> StorageResult<()> {
            Ok(())
        }
        fn begin_file(&mut self, _: &Entry) -> StorageResult<()> {
            Ok(())
        }
        fn append(&mut self, bytes: &[u8]) -> StorageResult<()> {
            self.0.extend_from_slice(bytes);
            Ok(())
        }
        fn end_file(&mut self, _: &Entry) -> StorageResult<()> {
            Ok(())
        }
        fn symlink(&mut self, _: &Entry, _: &str) -> StorageResult<()> {
            Ok(())
        }
        fn hard_link(&mut self, _: &Entry, _: &str) -> StorageResult<()> {
            Ok(())
        }
        fn finish(&mut self, _: &[&Entry]) -> StorageResult<()> {
            Ok(())
        }
    }

    /// A 3 MB file crosses `:execute` in pages that each fit its reply limit, is published to the
    /// store, and seeds a second sandbox's spool through the same channel.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_commit_crosses_execute_in_pages_under_the_reply_limit() {
        let store = MemoryStore::new();
        let start = takeover(&store, "sandbox-1").await.unwrap();
        let first = agent().await;
        let content: Vec<u8> = (0..3_000_000u32).map(|i| (i % 241) as u8).collect();
        let walk = vec![TreeEntry {
            path: "data.bin".to_string(),
            kind: TreeEntryKind::File {
                size: content.len() as u64,
                change_token: 1,
            },
            mode: 0o644,
            uid: 1000,
            gid: 1000,
            mtime_ns: 1,
        }];
        Committer::new("sandbox-1", first.spool.head_version(), None, None)
            .commit(first.spool.as_ref(), "k", walk, &mut Bytes(content.clone()))
            .await
            .unwrap();

        let envelopes = Arc::new(std::sync::Mutex::new(Vec::new()));
        let provider = through_execute(&first, Arc::clone(&envelopes));
        let channel = provider.storage_channel("sb-1").unwrap();
        let channel = &channel;
        let mut puller = Puller::new("sandbox-1", start.head_version, GCP_STORAGE_PAGE_BYTES);
        let outcome = puller.pull(channel, &store, "k").await.unwrap();
        assert!(
            matches!(outcome, PullOutcome::Published { .. }),
            "{outcome:?}"
        );
        let reads = envelopes
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e["op"] == "storageRead")
            .count();
        assert!(reads >= 4, "3 MB in pages of 900 KB: {reads} reads");
        assert_eq!(first.spool.pending(), None, "the ack reached the agent");

        takeover(&store, "sandbox-2").await.unwrap();
        let second = agent().await;
        let provider = through_execute(&second, Arc::new(std::sync::Mutex::new(Vec::new())));
        let channel = provider.storage_channel("sb-2").unwrap();
        let channel = &channel;
        let (head, _) = read_head(&store).await.unwrap().unwrap();
        let manifest = seed(
            channel,
            &store,
            head.commit.as_ref().unwrap(),
            GCP_STORAGE_PAGE_BYTES,
        )
        .await
        .unwrap();
        let mut restored = Collect(Vec::new());
        restore(second.spool.as_ref(), &manifest, &mut restored)
            .await
            .unwrap();
        assert!(restored.0 == content);
    }

    // ---- the binding: create, pull loop, sync, status, takeover ----------------------------

    /// Sandboxes as Agent Platform runs them, each a real agent reached through `:execute`.
    #[derive(Default)]
    struct Fleet {
        waiting: std::sync::Mutex<Vec<Agent>>,
        running: std::sync::Mutex<std::collections::HashMap<String, Agent>>,
        deleted: std::sync::Mutex<std::collections::HashSet<String>>,
        next: std::sync::atomic::AtomicU64,
    }

    fn not_found(
        id: &str,
    ) -> AlienError<alien_gcp_clients::gcp::agent_platform::AgentPlatformErrorData> {
        AlienError::new(alien_client_core::ErrorData::RemoteResourceNotFound {
            resource_type: "SandboxEnvironment".to_string(),
            resource_name: id.to_string(),
        })
        .context(
            alien_gcp_clients::gcp::agent_platform::AgentPlatformErrorData::RequestFailed {
                operation: "get sandbox".to_string(),
                message: id.to_string(),
            },
        )
    }

    fn running(id: &str) -> alien_gcp_clients::gcp::agent_platform::SandboxEnvironment {
        alien_gcp_clients::gcp::agent_platform::SandboxEnvironment {
            name: Some(format!(
                "projects/p/locations/us-central1/reasoningEngines/e/sandboxEnvironments/{id}"
            )),
            display_name: None,
            state: Some("STATE_RUNNING".to_string()),
            create_time: None,
            sandbox_environment_template: None,
            expire_time: None,
            connection_info: None,
            extra: Default::default(),
        }
    }

    fn fleet_client(fleet: Arc<Fleet>) -> MockAgentPlatformApi {
        let mut client = MockAgentPlatformApi::new();
        let f = Arc::clone(&fleet);
        client.expect_create_sandbox().returning(move |_, _| {
            let id = format!(
                "{}",
                1000 + f.next.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            );
            let agent = f.waiting.lock().unwrap().remove(0);
            f.running.lock().unwrap().insert(id.clone(), agent);
            Ok(alien_gcp_clients::gcp::longrunning::Operation {
                name: Some("projects/p/locations/us-central1/operations/op".to_string()),
                metadata: None,
                done: Some(true),
                result: Some(
                    alien_gcp_clients::gcp::longrunning::OperationResult::Response {
                        response: serde_json::to_value(running(&id)).unwrap(),
                    },
                ),
            })
        });
        let f = Arc::clone(&fleet);
        client.expect_get_sandbox().returning(move |_, id| {
            if f.running.lock().unwrap().contains_key(id) {
                Ok(running(id))
            } else {
                Err(not_found(id))
            }
        });
        let f = Arc::clone(&fleet);
        client.expect_delete_sandbox().returning(move |_, id| {
            match f.running.lock().unwrap().remove(id) {
                Some(_) => {
                    f.deleted.lock().unwrap().insert(id.to_string());
                    Ok(())
                }
                None => Err(not_found(id)),
            }
        });
        let f = Arc::clone(&fleet);
        client.expect_execute().returning(move |_, id, input| {
            let (url, token) = match f.running.lock().unwrap().get(id) {
                Some(agent) => (agent.url.clone(), agent.token.clone()),
                None => return Err(not_found(id)),
            };
            let input = input.to_vec();
            let body = post_to_agent(url, token, input);
            Ok(body.to_vec())
        });
        client
    }

    fn declared() -> alien_core::bindings::SandboxPersistentStorageBinding {
        alien_core::bindings::SandboxPersistentStorageBinding {
            path: "/workspace".to_string(),
            store: alien_core::bindings::PersistentStoreLocator::Gcp {
                bucket: alien_core::BindingValue::Value("b".to_string()),
                prefix: "keys/".to_string(),
            },
            exclude: vec![],
            max_size_bytes: None,
        }
    }

    async fn fleet(
        agents: usize,
    ) -> (
        Arc<Fleet>,
        GcpAgentPlatformSandbox,
        Arc<object_store::memory::InMemory>,
    ) {
        let fleet = Arc::new(Fleet::default());
        for _ in 0..agents {
            let agent = agent().await;
            fleet.waiting.lock().unwrap().push(agent);
        }
        let bucket = Arc::new(object_store::memory::InMemory::new());
        let provider = GcpAgentPlatformSandbox::new(
            Arc::new(fleet_client(Arc::clone(&fleet))),
            "projects/p/locations/us-central1/reasoningEngines/e".to_string(),
            "projects/p/locations/us-central1/sandboxTemplates/t".to_string(),
            Some(3600),
        )
        .with_persistent_store(Some(GcpPersistentStore::new(
            declared(),
            Arc::clone(&bucket) as Arc<dyn object_store::ObjectStore>,
            "keys/".to_string(),
        )));
        (fleet, provider, bucket)
    }

    fn workspace(fleet: &Fleet, id: &str) -> std::path::PathBuf {
        fleet.running.lock().unwrap()[id].workspace.clone()
    }

    async fn eventually<F: std::future::Future<Output = bool>>(mut check: impl FnMut() -> F) {
        for _ in 0..200 {
            if check().await {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("not reached within 10 s");
    }

    fn create(key: &str) -> crate::traits::CreateSandboxRequest {
        crate::traits::CreateSandboxRequest {
            storage_key: Some(key.to_string()),
            ..Default::default()
        }
    }

    /// A key outlives its sandbox: what the first sandbox wrote is published by the pull loop
    /// with no call from the caller, restored into the next sandbox at create, and readable
    /// without one.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_key_written_by_one_sandbox_is_restored_into_the_next() {
        use crate::traits::Sandbox as _;
        let (fleet, provider, _bucket) = fleet(2).await;
        let first = provider.create(create("project-1")).await.unwrap();
        std::fs::write(
            workspace(&fleet, &first.sandbox_id).join("notes.txt"),
            b"one",
        )
        .unwrap();
        // No sync: the agent commits after a quiet second and the binding's loop publishes.
        eventually(|| async {
            provider
                .read_storage_file("project-1", "notes.txt")
                .await
                .ok()
                == Some(b"one".to_vec())
        })
        .await;
        let status = provider
            .storage_status(&first.sandbox_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(status.state, SandboxStorageState::Replicating);
        assert_eq!(status.key.as_deref(), Some("project-1"));
        assert!(status.head.is_some(), "{status:?}");

        provider.terminate(&first.sandbox_id).await.unwrap();
        let second = provider.create(create("project-1")).await.unwrap();
        assert_eq!(
            std::fs::read(workspace(&fleet, &second.sandbox_id).join("notes.txt")).unwrap(),
            b"one",
            "restored before create returned"
        );
    }

    /// `sync` returns once a write is in the store, not merely committed inside the sandbox.
    #[tokio::test(flavor = "multi_thread")]
    async fn sync_returns_once_the_write_is_in_the_store() {
        use crate::traits::Sandbox as _;
        let (fleet, provider, _bucket) = fleet(1).await;
        let sandbox = provider.create(create("k")).await.unwrap();
        // Stop the loop so only the sync can publish.
        provider.forget_pull(&sandbox.sandbox_id);
        std::fs::write(workspace(&fleet, &sandbox.sandbox_id).join("a"), b"synced").unwrap();
        let status = provider.sync_storage(&sandbox.sandbox_id).await.unwrap();
        assert_eq!(
            provider.read_storage_file("k", "a").await.unwrap(),
            b"synced"
        );
        let (head, _) =
            alien_sandbox_storage::read_head(&provider.storage_key_store("k", "t").unwrap())
                .await
                .unwrap()
                .unwrap();
        assert_eq!(
            status.head,
            head.commit.map(|c| c.seq),
            "status reports the store's HEAD"
        );
    }

    /// A binding that starts again (a new workload process) finds its sandbox and pulls it
    /// again; a terminate publishes the last writes before the sandbox goes.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_new_binding_resumes_pulling_and_terminate_publishes_first() {
        use crate::traits::Sandbox as _;
        let (fleet, provider, bucket) = fleet(1).await;
        let sandbox = provider.create(create("k")).await.unwrap();
        drop(provider);

        let again = GcpAgentPlatformSandbox::new(
            Arc::new(fleet_client(Arc::clone(&fleet))),
            "projects/p/locations/us-central1/reasoningEngines/e".to_string(),
            "projects/p/locations/us-central1/sandboxTemplates/t".to_string(),
            Some(3600),
        )
        .with_persistent_store(Some(GcpPersistentStore::new(
            declared(),
            bucket as Arc<dyn object_store::ObjectStore>,
            "keys/".to_string(),
        )));
        again.get(&sandbox.sandbox_id).await.unwrap().unwrap();
        std::fs::write(workspace(&fleet, &sandbox.sandbox_id).join("late"), b"kept").unwrap();
        again.terminate(&sandbox.sandbox_id).await.unwrap();
        assert_eq!(again.read_storage_file("k", "late").await.unwrap(), b"kept");
    }

    /// A second sandbox claiming the key stops the first: its status says `stopped` and its
    /// later writes never reach the store.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_sandbox_whose_key_was_claimed_reports_stopped() {
        use crate::traits::Sandbox as _;
        let (fleet, provider, _bucket) = fleet(2).await;
        let old = provider.create(create("k")).await.unwrap();
        let new = provider.create(create("k")).await.unwrap();
        std::fs::write(workspace(&fleet, &new.sandbox_id).join("owner"), b"new").unwrap();
        provider.sync_storage(&new.sandbox_id).await.unwrap();
        std::fs::write(workspace(&fleet, &old.sandbox_id).join("owner"), b"old").unwrap();
        eventually(|| async {
            provider
                .storage_status(&old.sandbox_id)
                .await
                .unwrap()
                .unwrap()
                .state
                == SandboxStorageState::Stopped
        })
        .await;
        assert!(provider.sync_storage(&old.sandbox_id).await.is_err());
        assert_eq!(
            provider.read_storage_file("k", "owner").await.unwrap(),
            b"new"
        );
        assert!(
            provider.delete_storage("k").await.is_err(),
            "a running holder keeps its key"
        );
        provider.terminate(&new.sandbox_id).await.unwrap();
        provider.delete_storage("k").await.unwrap();
        assert!(provider.read_storage_file("k", "owner").await.is_err());
    }

    /// Without a declaration a key is refused, and with one a create needs it.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_declaration_makes_the_key_required() {
        use crate::traits::Sandbox as _;
        let (_fleet, provider, _bucket) = fleet(0).await;
        let error = provider
            .create(crate::traits::CreateSandboxRequest::default())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("storageKey"), "{error}");
    }

    #[test]
    fn writes_unpublished_for_longer_than_a_pull_takes_are_behind() {
        let now = chrono::Utc::now();
        let pending = |since: Option<chrono::DateTime<chrono::Utc>>,
                       head_at: chrono::DateTime<chrono::Utc>| Pending {
            generation: 3,
            head: serde_json::json!({ "commit": null, "writer": "1001", "updatedAt": head_at })
                .to_string(),
            objects: vec![
                alien_sandbox_storage::PendingObject {
                    name: "packs/a".to_string(),
                    size: 700,
                },
                alien_sandbox_storage::PendingObject {
                    name: "manifests/a".to_string(),
                    size: 50,
                },
            ],
            since,
        };
        let secs = |n: i64| chrono::Duration::seconds(n);
        assert_eq!(unpublished(None, now), (0, false));
        // The oldest commit decides, not the latest: a sandbox that keeps writing while nothing
        // pulls is behind although its newest commit is a second old.
        assert_eq!(
            unpublished(Some(&pending(Some(now - secs(60)), now - secs(1))), now),
            (750, true)
        );
        assert_eq!(
            unpublished(Some(&pending(Some(now - secs(2)), now - secs(1))), now),
            (750, false)
        );
        // An older agent: only the latest commit's time is known.
        assert_eq!(
            unpublished(Some(&pending(None, now - secs(60))), now),
            (750, true)
        );
        assert_eq!(
            unpublished(Some(&pending(None, now - secs(1))), now),
            (750, false)
        );
    }

    #[test]
    fn a_page_fits_the_execute_reply_limit_after_both_encodings() {
        // Raw page -> base64 in the agent's JSON -> base64 again in the reply.
        let once = (GCP_STORAGE_PAGE_BYTES as usize).div_ceil(3) * 4 + 64;
        let twice = once.div_ceil(3) * 4;
        assert!(twice < 2_000_000, "{twice}");
        assert_eq!(
            GCP_STORAGE_PAGE_BYTES,
            alien_sandbox_agent::server::STORAGE_PAGE_BYTES,
            "the binding asks for what the agent serves"
        );
    }

    /// The proxy relays an agent's refusal as an API error. Its text must reach the top message,
    /// because the Node addon shows only that; every failure used to read "the agent could not be
    /// reached through :execute", the agent's answer included.
    #[tokio::test]
    async fn an_agent_refusal_is_named_in_the_storage_error_message() {
        let mut client = MockAgentPlatformApi::new();
        client.expect_execute().returning(|_, _, _| {
            let body = serde_json::json!({ "error": { "code": 400, "message":
                "Execution Failed. Error Details: REQUEST_INVALID: 'packs/x' is not a pack or manifest name"
            }})
            .to_string();
            Err(AlienError::new(alien_client_core::ErrorData::HttpResponseError {
                message: "Request failed with HTTP 400".to_string(),
                url: "https://example.invalid/:execute".to_string(),
                http_status: 400,
                http_request_text: None,
                http_response_text: Some(body),
            })
            .context(alien_gcp_clients::gcp::agent_platform::AgentPlatformErrorData::ExecuteFailed {
                sandbox: "s1".to_string(),
                message: "the API rejected or cut short the request".to_string(),
            }))
        });
        let provider = GcpAgentPlatformSandbox::new(
            Arc::new(client),
            "projects/p/locations/us-central1/reasoningEngines/e".to_string(),
            "projects/p/locations/us-central1/sandboxTemplates/t".to_string(),
            Some(3600),
        );
        let channel = provider.storage_channel("s1").unwrap();
        let error = PullChannel::read(&channel, "packs/x", 0, 10)
            .await
            .expect_err("the agent refused");
        let shown = storage_failed("sandbox.sync", error).message;
        assert!(shown.contains("is not a pack or manifest name"), "{shown}");
        assert!(!shown.contains("could not be reached"), "{shown}");
    }
}
