//! Direct client for the local manager's queue service. Requests are never retried.
use crate::{
    error::{ErrorData, Result},
    traits::{Binding, MessagePayload, Queue, QueueAckResult, QueueMessage, QueueSendResult},
};
use alien_core::bindings::LocalHttpQueueBinding;
use alien_error::{AlienError, Context, IntoAlienError};
use async_trait::async_trait;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::time::Duration;

/// Wire payload preserves JSON scalars as well as text.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum Payload {
    Json(serde_json::Value),
    Text(String),
}
impl From<MessagePayload> for Payload {
    fn from(value: MessagePayload) -> Self {
        match value {
            MessagePayload::Json(v) => Self::Json(v),
            MessagePayload::Text(v) => Self::Text(v),
        }
    }
}
impl From<Payload> for MessagePayload {
    fn from(value: Payload) -> Self {
        match value {
            Payload::Json(v) => Self::Json(v),
            Payload::Text(v) => Self::Text(v),
        }
    }
}

/// Queue-specific protocol on the manager's existing HTTP listener.
#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Request {
    Send {
        message: Payload,
    },
    SendBatch {
        messages: Vec<Payload>,
    },
    Receive {
        max_messages: usize,
    },
    Ack {
        receipt: String,
    },
    AckBatch {
        receipts: Vec<String>,
    },
    Nack {
        receipt: String,
    },
    Renew {
        receipt: String,
        duration_seconds: u32,
    },
    Purge,
    Stats,
}

/// Received message; receipts remain opaque across this boundary.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub payload: Payload,
    pub receipt_handle: String,
    pub attempt: u32,
}
impl From<QueueMessage> for Message {
    fn from(value: QueueMessage) -> Self {
        Self {
            payload: value.payload.into(),
            receipt_handle: value.receipt_handle,
            attempt: value.attempt,
        }
    }
}
impl From<Message> for QueueMessage {
    fn from(value: Message) -> Self {
        Self {
            payload: value.payload.into(),
            receipt_handle: value.receipt_handle,
            attempt: value.attempt,
        }
    }
}

/// Counts observed in one database snapshot, without another process opening the store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueStats {
    pub total: u64,
    pub visible: u64,
    pub in_flight: u64,
    pub delayed: u64,
}

/// Runtime-less HTTP binding to a single manager-owned queue.
pub struct LocalHttpQueue {
    endpoint: String,
    token: String,
    client: reqwest::Client,
}
impl std::fmt::Debug for LocalHttpQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalHttpQueue")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}
impl LocalHttpQueue {
    pub fn new(binding: LocalHttpQueueBinding) -> Result<Self> {
        let endpoint =
            binding
                .endpoint
                .into_value("queue", "endpoint")
                .context(ErrorData::config_invalid(
                    "queue",
                    "Unresolved local queue endpoint",
                ))?;
        let token = binding.token.filter(|v| !v.is_empty()).ok_or_else(|| {
            AlienError::new(ErrorData::config_invalid(
                "queue",
                "Missing local queue credential; restart the workload with the current manager",
            ))
        })?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .timeout(Duration::from_secs(30))
            .build()
            .into_alien_error()
            .context(operation_error("initialize"))?;
        Ok(Self {
            endpoint,
            token,
            client,
        })
    }

    async fn request<T: DeserializeOwned>(&self, request: Request) -> Result<T> {
        let response = self
            .client
            .post(&self.endpoint)
            .bearer_auth(&self.token)
            .json(&request)
            .send()
            .await
            .into_alien_error()
            .context(operation_error("request"))?;
        if !response.status().is_success() {
            let error: AlienError<ErrorData> = response
                .json()
                .await
                .into_alien_error()
                .context(operation_error("decode error response"))?;
            return Err(error);
        }
        response
            .json()
            .await
            .into_alien_error()
            .context(operation_error("decode response"))
    }

    /// Observe the queue through its owner, including leased and delayed messages.
    pub async fn stats(&self) -> Result<QueueStats> {
        self.request(Request::Stats).await
    }
}
fn operation_error(operation: &str) -> ErrorData {
    ErrorData::QueueOperationFailed {
        operation: operation.to_string(),
        reason:
            "Local queue HTTP operation failed; acceptance may be unknown after a transport failure"
                .to_string(),
    }
}
impl Binding for LocalHttpQueue {}
#[async_trait]
impl Queue for LocalHttpQueue {
    async fn send(&self, _: &str, message: MessagePayload) -> Result<()> {
        self.request(Request::Send {
            message: message.into(),
        })
        .await
    }
    async fn send_batch(
        &self,
        _: &str,
        messages: Vec<MessagePayload>,
    ) -> Result<Vec<QueueSendResult>> {
        // Validate all payloads before any request. Ten maximum-size messages fit
        // below the 8 MiB route limit even with JSON's worst-case sixfold escaping.
        let (_, mut results) = super::encode_batch(messages.clone())?;
        let mut pending = messages
            .into_iter()
            .enumerate()
            .filter_map(|(index, message)| {
                (!matches!(results[index], QueueSendResult::Rejected { .. }))
                    .then_some((index, message.into()))
            })
            .collect::<Vec<(usize, Payload)>>()
            .into_iter();
        loop {
            let chunk = pending
                .by_ref()
                .take(crate::traits::MAX_BATCH_SIZE)
                .collect::<Vec<_>>();
            if chunk.is_empty() {
                break;
            }
            let (indices, messages): (Vec<_>, Vec<_>) = chunk.into_iter().unzip();
            match self
                .request::<Vec<QueueSendResult>>(Request::SendBatch { messages })
                .await
            {
                Ok(outcomes) if outcomes.len() == indices.len() => {
                    for (index, outcome) in indices.into_iter().zip(outcomes) {
                        results[index] = outcome;
                    }
                }
                Ok(_) => {
                    // Keep the prefilled Unknown outcome for each unconfirmed input.
                }
                Err(error) => {
                    for index in indices {
                        results[index] = QueueSendResult::Unknown {
                            code: error.code.clone(),
                            message: error.to_string(),
                        };
                    }
                }
            }
        }
        Ok(results)
    }

    async fn receive(&self, _: &str, max_messages: usize) -> Result<Vec<QueueMessage>> {
        let messages: Vec<Message> = self.request(Request::Receive { max_messages }).await?;
        Ok(messages.into_iter().map(Into::into).collect())
    }
    async fn ack(&self, _: &str, receipt: &str) -> Result<()> {
        self.request(Request::Ack {
            receipt: receipt.into(),
        })
        .await
    }
    async fn ack_batch(&self, _: &str, receipts: &[String]) -> Result<Vec<QueueAckResult>> {
        let mut results = Vec::with_capacity(receipts.len());
        for chunk in receipts.chunks(crate::traits::MAX_BATCH_SIZE) {
            match self
                .request::<Vec<QueueAckResult>>(Request::AckBatch {
                    receipts: chunk.to_vec(),
                })
                .await
            {
                Ok(outcomes) if outcomes.len() == chunk.len() => results.extend(outcomes),
                Ok(_) => results.extend(chunk.iter().map(|_| QueueAckResult::Failed {
                    code: "QUEUE_BATCH_RESPONSE_INVALID".into(),
                    message: "Response count did not match input count".into(),
                })),
                Err(error) => results.extend(chunk.iter().map(|_| QueueAckResult::Failed {
                    code: error.code.clone(),
                    message: error.to_string(),
                })),
            }
        }
        Ok(results)
    }

    async fn nack(&self, _: &str, receipt: &str) -> Result<()> {
        self.request(Request::Nack {
            receipt: receipt.into(),
        })
        .await
    }
    async fn renew_lease(&self, _: &str, receipt: &str, duration_seconds: u32) -> Result<()> {
        self.request(Request::Renew {
            receipt: receipt.into(),
            duration_seconds,
        })
        .await
    }
    async fn purge(&self, _: &str) -> Result<()> {
        self.request(Request::Purge).await
    }
}
