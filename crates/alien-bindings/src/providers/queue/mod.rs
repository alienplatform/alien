#[cfg(feature = "aws")]
pub mod aws_sqs;

#[cfg(feature = "gcp")]
pub mod gcp_pubsub;

#[cfg(feature = "azure")]
pub mod azure_service_bus;
#[cfg(feature = "local")]
pub mod local;

#[cfg(feature = "aws")]
pub use aws_sqs::AwsSqsQueue;
#[cfg(feature = "azure")]
pub use azure_service_bus::AzureServiceBusQueue;
#[cfg(feature = "gcp")]
pub use gcp_pubsub::GcpPubSubQueue;

use crate::error::{ErrorData, Result};
use crate::traits::{MessagePayload, QueueSendResult, MAX_MESSAGE_BYTES};
use alien_error::{Context, IntoAlienError};

/// Encode before sending so serialization errors cannot hide an earlier send.
fn encode_batch(
    messages: Vec<MessagePayload>,
) -> Result<(Vec<(usize, String)>, Vec<QueueSendResult>)> {
    let mut entries = Vec::with_capacity(messages.len());
    let mut results = Vec::with_capacity(messages.len());
    for (index, message) in messages.into_iter().enumerate() {
        let body = match message {
            MessagePayload::Text(body) => body,
            MessagePayload::Json(value) => serde_json::to_string(&value)
                .into_alien_error()
                .context(ErrorData::SerializationFailed {
                    message: "Queue batch JSON encoding".to_string(),
                })?,
        };
        if body.is_empty() || body.len() > MAX_MESSAGE_BYTES {
            results.push(QueueSendResult::Rejected {
                code: "QUEUE_MESSAGE_SIZE_INVALID".to_string(),
                message: format!(
                    "Message must contain 1 to {MAX_MESSAGE_BYTES} bytes; got {}",
                    body.len()
                ),
            });
        } else {
            entries.push((index, body));
            results.push(QueueSendResult::Unknown {
                code: "QUEUE_BATCH_RESPONSE_INVALID".to_string(),
                message: "Provider did not confirm this message".to_string(),
            });
        }
    }
    Ok((entries, results))
}

/// Keep requests bounded by both message count and encoded body size.
fn batch_end(
    entries: &[(usize, String)],
    start: usize,
    max_count: usize,
    max_bytes: usize,
) -> usize {
    let mut bytes = 0;
    let mut end = start;
    while end < entries.len() && end - start < max_count {
        let size = entries[end].1.len();
        if end > start && bytes + size > max_bytes {
            break;
        }
        bytes += size;
        end += 1;
    }
    end
}
