use super::{batch_end, encode_batch};
use crate::error::{ErrorData, Result};
use crate::traits::QueueSendResult;
use crate::traits::{
    Binding, MessagePayload, Queue, QueueMessage, MAX_BATCH_SIZE, MAX_MESSAGE_BYTES,
};
use alien_aws_clients::sqs::SendMessageBatchEntry;
use alien_aws_clients::sqs::{
    DeleteMessageRequest, Message, ReceiveMessageRequest, SendMessageRequest, SqsApi, SqsClient,
};
use alien_error::{AlienError, Context, ContextError, IntoAlienError};
use async_trait::async_trait;
#[cfg(not(target_arch = "wasm32"))]
use hickory_resolver::{config::LookupIpStrategy, TokioResolver};
#[cfg(not(target_arch = "wasm32"))]
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::Client;
use std::{
    fmt::{Debug, Formatter},
    time::Duration,
};
#[cfg(not(target_arch = "wasm32"))]
use std::{net::SocketAddr, sync::Arc};

/// Reuses concurrent SQS acknowledgement connections without blocking on DNS.
pub(crate) fn create_http_client() -> Result<Client> {
    let builder = Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(10))
        // Four idle connections discard almost every connection after an ack burst.
        // Keep a bounded burst-sized pool and release unused sockets after 30 seconds.
        .pool_max_idle_per_host(512)
        .pool_idle_timeout(Some(Duration::from_secs(30)));

    #[cfg(not(target_arch = "wasm32"))]
    let builder = {
        let mut resolver = TokioResolver::builder_tokio().into_alien_error().context(
            ErrorData::BindingSetupFailed {
                binding_type: "queue.sqs".to_string(),
                reason: "Failed to read system DNS configuration".to_string(),
            },
        )?;
        resolver.options_mut().ip_strategy = LookupIpStrategy::Ipv4AndIpv6;
        builder.dns_resolver(Arc::new(SqsDnsResolver(resolver.build())))
    };

    builder
        .build()
        .into_alien_error()
        .context(ErrorData::BindingSetupFailed {
            binding_type: "queue.sqs".to_string(),
            reason: "Failed to create HTTP client".to_string(),
        })
}

// A per-client resolver avoids changing reqwest's default DNS behavior for other bindings.
// Hickory caches answers according to DNS TTL and uses Tokio I/O instead of getaddrinfo workers.
#[cfg(not(target_arch = "wasm32"))]
struct SqsDnsResolver(TokioResolver);

#[cfg(not(target_arch = "wasm32"))]
impl Resolve for SqsDnsResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let resolver = self.0.clone();
        Box::pin(async move {
            let lookup = resolver.lookup_ip(name.as_str()).await?;
            let addresses: Addrs = Box::new(lookup.into_iter().map(|ip| SocketAddr::new(ip, 0)));
            Ok(addresses)
        })
    }
}

pub struct AwsSqsQueue {
    queue_url: String,
    client: SqsClient,
}

impl Debug for AwsSqsQueue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AwsSqsQueue")
            .field("queue_url", &self.queue_url)
            .finish()
    }
}

impl AwsSqsQueue {
    pub fn new(queue_url: String, client: SqsClient) -> Self {
        Self { queue_url, client }
    }
}

impl Binding for AwsSqsQueue {}

#[async_trait]
impl Queue for AwsSqsQueue {
    async fn send(&self, _queue: &str, message: MessagePayload) -> Result<()> {
        let (body, _ct) = match message {
            MessagePayload::Json(v) => (
                serde_json::to_string(&v).into_alien_error().context(
                    ErrorData::BindingSetupFailed {
                        binding_type: "queue.sqs".to_string(),
                        reason: "Failed to serialize JSON payload".to_string(),
                    },
                )?,
                "application/json".to_string(),
            ),
            MessagePayload::Text(s) => (s, "text/plain; charset=utf-8".to_string()),
        };

        // Client-side validation: check message size
        if body.len() > MAX_MESSAGE_BYTES {
            return Err(alien_error::AlienError::new(
                ErrorData::BindingSetupFailed {
                    binding_type: "queue.sqs".to_string(),
                    reason: format!(
                        "Message size {} bytes exceeds limit of {} bytes",
                        body.len(),
                        MAX_MESSAGE_BYTES
                    ),
                },
            ));
        }

        let req = SendMessageRequest::builder().message_body(body).build();
        self.client
            .send_message(&self.queue_url, req)
            .await
            .map(|_| ())
            .map_err(|e| {
                e.context(ErrorData::BindingSetupFailed {
                    binding_type: "queue.sqs".to_string(),
                    reason: "Failed to send message".to_string(),
                })
            })
    }

    async fn send_batch(
        &self,
        _queue: &str,
        messages: Vec<MessagePayload>,
    ) -> Result<Vec<QueueSendResult>> {
        let (entries, mut results) = encode_batch(messages)?;
        let mut start = 0;
        while start < entries.len() {
            let end = batch_end(&entries, start, 10, 256 * 1024);
            let chunk = &entries[start..end];
            let request = chunk
                .iter()
                .map(|(index, body)| SendMessageBatchEntry {
                    id: index.to_string(),
                    message_body: body.clone(),
                })
                .collect();
            match self
                .client
                .send_message_batch(&self.queue_url, request)
                .await
            {
                Ok(response) => {
                    let response = response.send_message_batch_result;
                    for (index, _) in chunk {
                        let id = index.to_string();
                        let successes = response
                            .successful
                            .iter()
                            .filter(|entry| entry.id == id)
                            .count();
                        let failures = response
                            .failed
                            .iter()
                            .filter(|entry| entry.id == id)
                            .collect::<Vec<_>>();
                        results[*index] = match (successes, failures.as_slice()) {
                            (1, []) => QueueSendResult::Sent,
                            (0, [failure]) => QueueSendResult::Rejected {
                                code: failure.code.clone(),
                                message: failure
                                    .message
                                    .clone()
                                    .unwrap_or_else(|| "SQS rejected the message".to_string()),
                            },
                            _ => QueueSendResult::Unknown {
                                code: "QUEUE_BATCH_RESPONSE_INVALID".to_string(),
                                message: "SQS returned missing or duplicate entry outcomes"
                                    .to_string(),
                            },
                        };
                    }
                }
                Err(error) => {
                    for (index, _) in chunk {
                        results[*index] = QueueSendResult::Unknown {
                            code: error.code.clone(),
                            message: error.to_string(),
                        };
                    }
                }
            }
            start = end;
        }
        Ok(results)
    }

    async fn receive(&self, _queue: &str, max_messages: usize) -> Result<Vec<QueueMessage>> {
        // Client-side validation: check batch size
        if max_messages == 0 || max_messages > MAX_BATCH_SIZE {
            return Err(alien_error::AlienError::new(
                ErrorData::BindingSetupFailed {
                    binding_type: "queue.sqs".to_string(),
                    reason: format!(
                        "Batch size {} is invalid. Must be between 1 and {}",
                        max_messages, MAX_BATCH_SIZE
                    ),
                },
            ));
        }

        let req = ReceiveMessageRequest::builder()
            // The SQS Query-protocol client uses AttributeName.N. AWS keeps
            // this parameter supported for backward compatibility.
            .attribute_names(vec!["ApproximateReceiveCount".to_string()])
            .maybe_max_number_of_messages(Some(max_messages as i32))
            .maybe_wait_time_seconds(Some(20))
            .build();
        let resp = self
            .client
            .receive_message(&self.queue_url, req)
            .await
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.sqs".to_string(),
                reason: "Failed to receive".to_string(),
            })?;
        resp.receive_message_result
            .messages
            .into_iter()
            .map(|m| {
                let attempt = receive_count(&m)?;
                let raw = m.body;
                let payload = serde_json::from_str::<serde_json::Value>(&raw)
                    .map(MessagePayload::Json)
                    .unwrap_or(MessagePayload::Text(raw));
                Ok(QueueMessage {
                    payload,
                    receipt_handle: m.receipt_handle,
                    attempt,
                })
            })
            .collect()
    }

    async fn ack(&self, _queue: &str, receipt_handle: &str) -> Result<()> {
        let req = DeleteMessageRequest::builder()
            .receipt_handle(receipt_handle.to_string())
            .build();
        self.client
            .delete_message(&self.queue_url, req)
            .await
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.sqs".to_string(),
                reason: "Failed to delete message".to_string(),
            })
    }

    async fn nack(&self, _queue: &str, _receipt_handle: &str) -> Result<()> {
        // SQS nack is ChangeMessageVisibility(VisibilityTimeout=0). The
        // alien-aws-clients SqsApi wrapper does not expose that call, so we
        // fail explicitly rather than silently waiting out the lease.
        Err(alien_error::AlienError::new(
            ErrorData::OperationNotSupported {
                operation: "queue.nack".to_string(),
                reason: "AWS SQS nack requires ChangeMessageVisibility, which the SQS client does not expose".to_string(),
            },
        ))
    }

    async fn purge(&self, _queue: &str) -> Result<()> {
        self.client
            .purge_queue(&self.queue_url)
            .await
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.sqs".to_string(),
                reason: "Failed to purge queue".to_string(),
            })
    }
}

fn receive_count(message: &Message) -> Result<u32> {
    let reason = || ErrorData::QueueProviderResponseInvalid {
        reason: format!(
            "SQS message '{}' has no positive ApproximateReceiveCount",
            message.message_id
        ),
    };
    let raw = message
        .attributes
        .as_ref()
        .and_then(|attributes| attributes.get("ApproximateReceiveCount"))
        .ok_or_else(|| AlienError::new(reason()))?;
    let count = raw.parse::<u32>().into_alien_error().context(reason())?;
    if count == 0 {
        return Err(AlienError::new(reason()));
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(not(target_arch = "wasm32"))]
    use alien_aws_clients::{
        AwsClientConfig, AwsClientConfigExt, AwsCredentialProvider, ServiceOverrides,
    };
    #[cfg(not(target_arch = "wasm32"))]
    use axum::{
        body::Body,
        extract::{ConnectInfo, State},
        routing::post,
        Router,
    };
    use std::collections::HashMap;
    #[cfg(not(target_arch = "wasm32"))]
    use std::{
        collections::HashSet,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Mutex,
        },
    };
    #[cfg(not(target_arch = "wasm32"))]
    use tokio::{net::TcpListener, runtime::Builder, sync::Barrier, task::JoinSet};

    fn message(count: Option<&str>) -> Message {
        Message {
            attributes: count.map(|count| {
                HashMap::from([("ApproximateReceiveCount".to_string(), count.to_string())])
            }),
            body: "payload".to_string(),
            md5_of_body: "unused".to_string(),
            md5_of_message_attributes: None,
            message_attributes: None,
            message_id: "message-1".to_string(),
            receipt_handle: "receipt-1".to_string(),
        }
    }

    #[test]
    fn reads_positive_sqs_delivery_attempts() {
        assert_eq!(receive_count(&message(Some("1"))).expect("first"), 1);
        assert_eq!(receive_count(&message(Some("2"))).expect("redelivery"), 2);
    }

    #[test]
    fn rejects_missing_or_invalid_sqs_delivery_attempts() {
        for count in [
            None,
            Some(""),
            Some("not-a-number"),
            Some("0"),
            Some("4294967296"),
        ] {
            let error = receive_count(&message(count)).expect_err("invalid attempt must fail");
            assert!(matches!(
                error.error,
                Some(ErrorData::QueueProviderResponseInvalid { .. })
            ));
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn sqs_reuses_ack_burst_connections_without_dns_blocking_threads() {
        const BURST: usize = 64;
        let threads = Arc::new(AtomicUsize::new(0));
        let started = threads.clone();
        let runtime = Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .on_thread_start(move || {
                started.fetch_add(1, Ordering::SeqCst);
            })
            .build()
            .expect("runtime");

        runtime.block_on(async {
            let peers = Arc::new(Mutex::new(HashSet::new()));
            let state = (Arc::new(Barrier::new(BURST)), peers.clone());
            let app = Router::new()
                .route(
                    "/",
                    post(
                        |State((barrier, peers)): State<(
                            Arc<Barrier>,
                            Arc<Mutex<HashSet<SocketAddr>>>,
                        )>,
                         ConnectInfo(peer): ConnectInfo<SocketAddr>| async move {
                            peers.lock().expect("peers").insert(peer);
                            // Headers reach the client before this streamed metadata body.
                            axum::response::Response::new(Body::from_stream(futures::stream::once(
                                async move {
                                    barrier.wait().await;
                                    Ok::<_, std::convert::Infallible>("<DeleteMessageResponse/>")
                                },
                            )))
                        },
                    ),
                )
                .with_state(state);
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("listen");
            let port = listener.local_addr().expect("address").port();
            let server = tokio::spawn(async move {
                axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .await
                .expect("serve");
            });
            let endpoint = format!("http://localhost:{port}");
            let config = AwsClientConfig::mock().with_service_overrides(ServiceOverrides {
                endpoints: HashMap::from([("sqs".to_string(), endpoint.clone())]),
            });
            let credentials = AwsCredentialProvider::from_config(config)
                .await
                .expect("credentials");
            let queue = Arc::new(AwsSqsQueue::new(
                endpoint,
                SqsClient::new(create_http_client().expect("SQS HTTP client"), credentials),
            ));
            tokio::time::timeout(Duration::from_secs(10), async {
                for _ in 0..2 {
                    let mut requests = JoinSet::new();
                    for _ in 0..BURST {
                        let queue = queue.clone();
                        requests.spawn(async move {
                            queue
                                .ack("pushes", "receipt")
                                .await
                                .expect("acknowledgement");
                        });
                    }
                    while let Some(request) = requests.join_next().await {
                        request.expect("request task");
                    }
                }
            })
            .await
            .expect("bursts must complete");
            assert_eq!(peers.lock().expect("peers").len(), BURST);
            let spawned = threads.load(Ordering::SeqCst);
            assert!(
                spawned <= 2,
                "DNS spawned {} blocking threads",
                spawned.saturating_sub(2)
            );
            server.abort();
            assert!(server.await.expect_err("server stopped").is_cancelled());
        });
    }
}
