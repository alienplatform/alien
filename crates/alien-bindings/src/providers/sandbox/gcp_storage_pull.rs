//! Persistent storage on GCP: the binding pulls the agent's spool through the `:execute` proxy,
//! because a sandbox in a PSC tenant has no route to Cloud Storage.
//!
//! Each [`PullChannel`] call is one `:execute` request carrying one agent envelope op
//! (`storagePending`, `storageRead`, `storageAck`, `storageSeed`). Reads and seed pages are at
//! most [`GCP_STORAGE_PAGE_BYTES`] raw bytes, the same page as `readFile`, so a reply stays under
//! the proxy's 2 MB limit after both base64 layers.

use alien_error::{AlienError, Context};
use alien_sandbox_storage::{
    AckOutcome, ErrorData as StorageError, Pending, PullChannel, Result as StorageResult,
};
use async_trait::async_trait;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::json;

use super::{GcpAgentPlatformSandbox, AGENT_PROTOCOL_VERSION, BASE64, GCP_READ_PAGE_BYTES};

/// Raw bytes one storage read or seed page carries over `:execute`.
pub const GCP_STORAGE_PAGE_BYTES: u64 = GCP_READ_PAGE_BYTES as u64;

/// One sandbox's agent, as the pull transport reaches it through `:execute`.
pub struct AgentPlatformStorageChannel<'a> {
    provider: &'a GcpAgentPlatformSandbox,
    sandbox_id: String,
}

impl GcpAgentPlatformSandbox {
    /// The pull channel to `sandbox_id`'s agent.
    pub fn storage_channel(
        &self,
        sandbox_id: &str,
    ) -> crate::error::Result<AgentPlatformStorageChannel<'_>> {
        Self::checked_sandbox_id("sandbox.storage", sandbox_id)?;
        Ok(AgentPlatformStorageChannel {
            provider: self,
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

impl AgentPlatformStorageChannel<'_> {
    async fn call(
        &self,
        op: &str,
        object: &str,
        mut envelope: serde_json::Value,
    ) -> StorageResult<Vec<u8>> {
        envelope["v"] = json!(AGENT_PROTOCOL_VERSION);
        envelope["op"] = json!(op);
        self.provider
            .execute_op(&self.sandbox_id, "sandbox.storage", envelope)
            .await
            .context(StorageError::StoreFailed {
                operation: op.to_string(),
                object: object.to_string(),
                reason: "the agent could not be reached through :execute".to_string(),
            })
    }

    /// An op that answers `{}`; anything else is the agent's refusal.
    fn confirm(op: &str, object: &str, body: &[u8]) -> StorageResult<()> {
        match serde_json::from_slice::<serde_json::Value>(body) {
            Ok(serde_json::Value::Object(map)) if map.is_empty() => Ok(()),
            _ => Err(failed(op, object, super::truncated(body))),
        }
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
impl PullChannel for AgentPlatformStorageChannel<'_> {
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

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::sync::Arc;

    use alien_core::sandbox_capability::{
        SandboxCapabilityClaims, SandboxOperationClass, SandboxSessionIdentity,
    };
    use alien_gcp_clients::gcp::agent_platform::MockAgentPlatformApi;
    use alien_sandbox_agent::exec::ExecIdentity;
    use alien_sandbox_agent::server::{router, AgentAuthorization, AgentState};
    use alien_sandbox_storage::{
        read_head, restore, seed, takeover, Committer, Entry, FileSource, MemoryStore, PullOutcome,
        Puller, Spool, TreeEntry, TreeEntryKind, TreeSink,
    };
    use ed25519_compact::{KeyPair, Seed};

    use super::*;

    /// A real agent on loopback, holding a spool.
    struct Agent {
        url: String,
        token: String,
        spool: Arc<Spool>,
        _root: tempfile::TempDir,
    }

    async fn agent(head_etag: &str) -> Agent {
        let root = tempfile::TempDir::new().unwrap();
        let keys = KeyPair::from_seed(Seed::new([9u8; 32]));
        let spool = Arc::new(Spool::new(root.path().join("spool"), head_etag).unwrap());
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
            storage_spool: Default::default(),
        });
        state.storage_spool.set(Arc::clone(&spool)).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        tokio::spawn(async move {
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
            let body = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async move {
                    reqwest::Client::new()
                        .post(url)
                        .bearer_auth(token)
                        .header("content-type", "application/json")
                        .body(input)
                        .send()
                        .await
                        .unwrap()
                        .bytes()
                        .await
                        .unwrap()
                })
            });
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
        fn open(&mut self, _: &str) -> StorageResult<Box<dyn std::io::Read + '_>> {
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
        let first = agent(&start.head_etag).await;
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
        Committer::new("sandbox-1", start.head_etag.clone(), None, None)
            .commit(first.spool.as_ref(), "k", walk, &mut Bytes(content.clone()))
            .await
            .unwrap();

        let envelopes = Arc::new(std::sync::Mutex::new(Vec::new()));
        let provider = through_execute(&first, Arc::clone(&envelopes));
        let channel = provider.storage_channel("sb-1").unwrap();
        let mut puller = Puller::new("sandbox-1", start.head_etag, GCP_STORAGE_PAGE_BYTES);
        let outcome = puller.pull(&channel, &store, "k").await.unwrap();
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

        let next = takeover(&store, "sandbox-2").await.unwrap();
        let second = agent(&next.head_etag).await;
        let provider = through_execute(&second, Arc::new(std::sync::Mutex::new(Vec::new())));
        let channel = provider.storage_channel("sb-2").unwrap();
        let (head, _) = read_head(&store).await.unwrap().unwrap();
        let manifest = seed(
            &channel,
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
}
