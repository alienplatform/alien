//! Persistent storage on an Azure sandbox: the binding claims the key, grants the agent a SAS for
//! the key's directory, and reaches the agent's storage state; reads and erasure use the
//! binding's own identity. The same steps as on AWS, over Azure's store and agent channel.

use super::AzureSandbox;
use crate::error::{ErrorData, Result};
use crate::providers::sandbox::agent_protocol::AgentTransport;
use crate::providers::sandbox::persistent_store::KeyStore;
use crate::traits::{Sandbox as _, SandboxState, SandboxStorageStatus};
use alien_error::AlienError;
use alien_sandbox_storage::ObjectStore as _;

impl AzureSandbox {
    /// Supplies the store the declaration's own setup created.
    pub(crate) fn with_persistent_grants(
        mut self,
        grants: Option<std::sync::Arc<super::super::azure_storage::AzureGrants>>,
    ) -> Self {
        self.persistent_grants = grants;
        self
    }

    pub(super) fn key_store(&self, key: &str, operation: &str) -> Result<KeyStore> {
        let grants = self.persistent_grants.as_ref().ok_or_else(|| {
            AlienError::new(ErrorData::OperationNotSupported {
                operation: operation.to_string(),
                reason: "this sandbox declares no persistent storage".to_string(),
            })
        })?;
        grants.key_store(key).map_err(|e| {
            AlienError::new(ErrorData::InvalidInput {
                operation_context: operation.to_string(),
                details: e.to_string(),
                field_name: Some("storageKey".to_string()),
            })
        })
    }

    /// Claims `key` for the new sandbox, then hands its agent the claimed commit and a SAS for the
    /// key's directory. The claim comes first, so a sandbox still running for the key can no
    /// longer publish over the tree this one restores.
    pub(super) async fn attach_storage(&self, sandbox_id: &str, key: &str) -> Result<()> {
        let unreachable = |reason: String| {
            AlienError::new(ErrorData::SandboxUnreachable {
                operation: "sandbox.create".to_string(),
                reason,
            })
        };
        let grants = self.persistent_grants.as_ref().ok_or_else(|| {
            unreachable("the binding has no persistent store to attach".to_string())
        })?;
        let store = grants
            .key_store(key)
            .map_err(|e| unreachable(format!("storage key '{key}': {e}")))?;
        let claim = alien_sandbox_storage::takeover(&store, sandbox_id)
            .await
            .map_err(|e| unreachable(format!("could not claim storage key '{key}': {e}")))?;
        let commit = match alien_sandbox_storage::read_head(&store)
            .await
            .map_err(|e| unreachable(format!("could not read storage key '{key}': {e}")))?
        {
            Some((head, _)) => head.commit,
            None => None,
        };
        let grant = grants
            .grant(key)
            .await
            .map_err(|e| unreachable(format!("could not sign the storage grant: {e}")))?;
        let body = serde_json::json!({
            "key": key,
            "writer": sandbox_id,
            "headVersion": claim.head_version,
            "commit": commit,
            "grant": grant,
        });
        let request = self
            .request(sandbox_id, reqwest::Method::PUT, "/v1/storage")
            .await?
            .json(&body);
        let response = self
            .send(sandbox_id, request, "sandbox.create")
            .await
            .map_err(|e| unreachable(format!("could not attach persistent storage: {e}")))?;
        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(unreachable(format!(
                "the agent refused persistent storage ({status}): {}",
                text.chars().take(500).collect::<String>()
            )));
        }
        Ok(())
    }

    pub(super) async fn agent_storage(
        &self,
        sandbox_id: &str,
        method: reqwest::Method,
        path: &str,
        operation: &str,
    ) -> Result<SandboxStorageStatus> {
        let request = self.request(sandbox_id, method, path).await?;
        let response = self.send(sandbox_id, request, operation).await?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(AlienError::new(ErrorData::SandboxCommandFailed {
                failure: "storage".to_string(),
                reason: format!("{status}: {}", text.chars().take(500).collect::<String>()),
            }));
        }
        serde_json::from_str(&text).map_err(|e| {
            AlienError::new(ErrorData::UnexpectedResponseFormat {
                provider: "azure-sandbox".to_string(),
                binding_name: operation.to_string(),
                field: "persistentStorage".to_string(),
                response_json: format!("{e}: {}", text.chars().take(500).collect::<String>()),
            })
        })
    }

    pub(super) async fn read_key_file(&self, key: &str, path: &str) -> Result<Vec<u8>> {
        let store = self.key_store(key, "sandbox.readStorageFile")?;
        alien_sandbox_storage::read_file(&store, key, path)
            .await
            .map_err(|e| {
                AlienError::new(ErrorData::SandboxCommandFailed {
                    failure: "readStorageFile".to_string(),
                    reason: e.to_string(),
                })
            })
    }

    /// Erases a key: refused while a sandbox that is not terminated holds it; HEAD first, so a
    /// writer that slipped in can no longer move it and no reader finds a HEAD whose manifest is
    /// gone.
    pub(super) async fn erase_key(&self, key: &str) -> Result<()> {
        let store = self.key_store(key, "sandbox.deleteStorage")?;
        let failed = |reason: String| {
            AlienError::new(ErrorData::SandboxCommandFailed {
                failure: "deleteStorage".to_string(),
                reason,
            })
        };
        if let Some((head, _)) = alien_sandbox_storage::read_head(&store)
            .await
            .map_err(|e| failed(e.to_string()))?
        {
            if let Some(holder) = self.get(&head.writer).await? {
                if holder.state != SandboxState::Terminated {
                    return Err(failed(format!(
                        "storage key '{key}' is held by sandbox '{}'; terminate it first",
                        head.writer
                    )));
                }
            }
        }
        store
            .delete(alien_sandbox_storage::HEAD_OBJECT)
            .await
            .map_err(|e| failed(e.to_string()))?;
        for object in store.list("").await.map_err(|e| failed(e.to_string()))? {
            store
                .delete(&object)
                .await
                .map_err(|e| failed(e.to_string()))?;
        }
        Ok(())
    }
}
