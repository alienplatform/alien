use super::{
    batch_end, encode_batch,
    storage_notifications::{aws_queue_payload, AwsQueueDelivery},
};
use crate::error::{ErrorData, Result};
use crate::traits::{
    Binding, MessagePayload, Queue, QueueAckResult, QueueMessage, QueueSendResult, MAX_BATCH_SIZE,
    MAX_MESSAGE_BYTES,
};
use alien_aws_clients::sqs::{
    DeleteMessageRequest, Message, ReceiveMessageRequest, SendMessageBatchEntry,
    SendMessageRequest, SqsApi, SqsClient,
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
        let resolver =
            resolver
                .build()
                .into_alien_error()
                .context(ErrorData::BindingSetupFailed {
                    binding_type: "queue.sqs".to_string(),
                    reason: "Failed to build DNS resolver".to_string(),
                })?;
        builder.dns_resolver(Arc::new(SqsDnsResolver(resolver)))
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

    /// S3 event notifications are normalized into one `StorageEvent` message per record,
    /// so a receive can return more messages than `max_messages` SQS messages; those from
    /// one SQS message share its receipt handle. S3 test events are deleted, not returned.
    async fn receive(&self, queue: &str, max_messages: usize) -> Result<Vec<QueueMessage>> {
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
        let mut messages = Vec::new();
        let mut discarded = Vec::new();
        for message in resp.receive_message_result.messages {
            let attempt = receive_count(&message)?;
            match aws_queue_payload(&message.body) {
                // S3 normally sends one record per notification. When an SQS message holds
                // several, each becomes its own QueueMessage sharing the one receipt handle,
                // so acknowledging any of them deletes the whole SQS message.
                AwsQueueDelivery::Deliver(payloads) => {
                    messages.extend(payloads.into_iter().map(|payload| QueueMessage {
                        payload: match payload {
                            alien_core::MessagePayload::Json(value) => MessagePayload::Json(value),
                            alien_core::MessagePayload::Text(text) => MessagePayload::Text(text),
                        },
                        receipt_handle: message.receipt_handle.clone(),
                        attempt,
                    }))
                }
                AwsQueueDelivery::Discard => discarded.push(message.receipt_handle),
            }
        }
        if !discarded.is_empty() {
            let outcomes = self.ack_batch(queue, &discarded).await?;
            if let Some(QueueAckResult::Failed { code, message }) = outcomes
                .into_iter()
                .find(|outcome| !matches!(outcome, QueueAckResult::Acknowledged))
            {
                return Err(AlienError::new(ErrorData::BindingSetupFailed {
                    binding_type: "queue.sqs".to_string(),
                    reason: format!("Failed to delete S3 test notifications ({code}): {message}"),
                }));
            }
        }
        Ok(messages)
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

    async fn ack_batch(&self, _queue: &str, receipts: &[String]) -> Result<Vec<QueueAckResult>> {
        let mut results = Vec::with_capacity(receipts.len());
        for chunk in receipts.chunks(10) {
            match self
                .client
                .delete_message_batch(&self.queue_url, chunk.to_vec())
                .await
            {
                Ok(response) => {
                    let response = response.delete_message_batch_result;
                    let known_id = |id: &str| (0..chunk.len()).any(|index| id == index.to_string());
                    let unknown = response.successful.iter().any(|entry| !known_id(&entry.id))
                        || response.failed.iter().any(|entry| !known_id(&entry.id));
                    for index in 0..chunk.len() {
                        let id = index.to_string();
                        let successes = response
                            .successful
                            .iter()
                            .filter(|entry| entry.id == id)
                            .count();
                        let failures: Vec<_> = response
                            .failed
                            .iter()
                            .filter(|entry| entry.id == id)
                            .collect();
                        results.push(match (unknown, successes, failures.as_slice()) {
                            (false, 1, []) => QueueAckResult::Acknowledged,
                            (false, 0, [failure]) => QueueAckResult::Failed {
                                code: failure.code.clone(),
                                message: failure
                                    .message
                                    .clone()
                                    .unwrap_or_else(|| "SQS rejected completion".to_string()),
                            },
                            _ => QueueAckResult::Failed {
                                code: "QUEUE_BATCH_RESPONSE_INVALID".to_string(),
                                message: "SQS returned missing, duplicate or unknown outcomes"
                                    .to_string(),
                            },
                        });
                    }
                }
                Err(error) => results.extend(std::iter::repeat_n(
                    QueueAckResult::Failed {
                        code: error.code.clone(),
                        message: error.to_string(),
                    },
                    chunk.len(),
                )),
            }
        }
        Ok(results)
    }

    async fn renew_lease(
        &self,
        _queue: &str,
        receipt_handle: &str,
        duration_seconds: u32,
    ) -> Result<()> {
        crate::traits::validate_lease_duration(duration_seconds)?;
        self.client
            .change_message_visibility(&self.queue_url, receipt_handle, duration_seconds)
            .await
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.sqs".to_string(),
                reason: "Failed to renew delivery lease".to_string(),
            })
    }

    async fn nack(&self, _queue: &str, receipt_handle: &str) -> Result<()> {
        self.client
            .change_message_visibility(&self.queue_url, receipt_handle, 0)
            .await
            .context(ErrorData::BindingSetupFailed {
                binding_type: "queue.sqs".to_string(),
                reason: "Failed to release delivery lease".to_string(),
            })
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
    use crate::providers::queue::storage_notifications::aws_fixtures;
    #[cfg(not(target_arch = "wasm32"))]
    use alien_aws_clients::{
        AwsClientConfig, AwsClientConfigExt, AwsCredentialProvider, ServiceOverrides,
    };
    use alien_core::{StorageEvent, StorageEventType};
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

    #[tokio::test]
    async fn batch_completion_chunks_requests_and_preserves_partial_failures() {
        let calls = Arc::new(Mutex::new(Vec::<HashMap<String, String>>::new()));
        let app = Router::new().route("/", post(
            |State(calls): State<Arc<Mutex<Vec<HashMap<String, String>>>>>,
             body: String| async move {
                let form: HashMap<String, String> = url::form_urlencoded::parse(body.as_bytes()).into_owned().collect();
                assert_eq!(form["Action"], "DeleteMessageBatch");
                let count = form.keys().filter(|key| key.ends_with(".ReceiptHandle")).count();
                assert!((1..=10).contains(&count));
                let mut entries = String::new();
                for index in 0..count {
                    assert_eq!(form[&format!("DeleteMessageBatchRequestEntry.{}.Id", index + 1)], index.to_string());
                    if form[&format!("DeleteMessageBatchRequestEntry.{}.ReceiptHandle", index + 1)] == "invalid" {
                        entries.push_str(&format!("<BatchResultErrorEntry><Id>{index}</Id><Code>ReceiptHandleIsInvalid</Code><SenderFault>true</SenderFault></BatchResultErrorEntry>"));
                    } else {
                        entries.push_str(&format!("<DeleteMessageBatchResultEntry><Id>{index}</Id></DeleteMessageBatchResultEntry>"));
                    }
                }
                calls.lock().expect("calls").push(form);
                format!("<DeleteMessageBatchResponse><DeleteMessageBatchResult>{entries}</DeleteMessageBatchResult></DeleteMessageBatchResponse>")
            }
        )).with_state(calls.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listen");
        let endpoint = format!("http://{}", listener.local_addr().expect("address"));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
        let config = AwsClientConfig::mock().with_service_overrides(ServiceOverrides {
            endpoints: HashMap::from([("sqs".to_string(), endpoint.clone())]),
        });
        let credentials = AwsCredentialProvider::from_config(config)
            .await
            .expect("credentials");
        let queue = AwsSqsQueue::new(
            endpoint,
            SqsClient::new(reqwest::Client::new(), credentials),
        );
        let mut receipts: Vec<_> = (0..12).map(|index| format!("receipt-{index}")).collect();
        receipts[4] = "invalid".to_string();
        let outcomes = queue.ack_batch("q", &receipts).await.expect("batch");
        assert_eq!(outcomes.len(), 12);
        for (index, outcome) in outcomes.iter().enumerate() {
            if index == 4 {
                assert!(
                    matches!(outcome, QueueAckResult::Failed { code, .. } if code == "ReceiptHandleIsInvalid")
                );
            } else {
                assert_eq!(*outcome, QueueAckResult::Acknowledged);
            }
        }
        assert_eq!(calls.lock().expect("calls").len(), 2);
        assert!(queue.ack_batch("q", &[]).await.expect("empty").is_empty());
        assert_eq!(calls.lock().expect("calls").len(), 2);
        server.abort();
        assert!(server.await.expect_err("aborted").is_cancelled());
    }

    #[tokio::test]
    async fn receive_normalizes_s3_notifications_and_deletes_test_events() {
        let deleted = Arc::new(Mutex::new(Vec::<String>::new()));
        let app = Router::new()
            .route(
                "/",
                post(
                    |State(deleted): State<Arc<Mutex<Vec<String>>>>, body: String| async move {
                        let form: HashMap<String, String> =
                            url::form_urlencoded::parse(body.as_bytes())
                                .into_owned()
                                .collect();
                        match form["Action"].as_str() {
                            "ReceiveMessage" => {
                                let messages: String = [
                                    ("test-event", aws_fixtures::test_event()),
                                    (
                                        "two-records",
                                        aws_fixtures::notification(vec![
                                            aws_fixtures::s3_put("in/with+space.json"),
                                            aws_fixtures::s3_put("in/plus%2Bsign.json"),
                                        ]),
                                    ),
                                    ("ordinary", "plain text".to_string()),
                                ]
                                .into_iter()
                                .map(|(receipt, body)| {
                                    let body = body
                                        .replace('&', "&amp;")
                                        .replace('<', "&lt;")
                                        .replace('"', "&quot;");
                                    format!(
                                        "<Message><MessageId>{receipt}-id</MessageId>\
                                         <ReceiptHandle>{receipt}</ReceiptHandle>\
                                         <MD5OfBody>unused</MD5OfBody><Body>{body}</Body>\
                                         <Attribute><Name>ApproximateReceiveCount</Name>\
                                         <Value>2</Value></Attribute></Message>"
                                    )
                                })
                                .collect();
                                format!("<ReceiveMessageResponse><ReceiveMessageResult>{messages}</ReceiveMessageResult></ReceiveMessageResponse>")
                            }
                            "DeleteMessageBatch" => {
                                assert!(!form.contains_key(
                                    "DeleteMessageBatchRequestEntry.2.ReceiptHandle"
                                ));
                                deleted.lock().expect("deleted").push(
                                    form["DeleteMessageBatchRequestEntry.1.ReceiptHandle"].clone(),
                                );
                                "<DeleteMessageBatchResponse><DeleteMessageBatchResult><DeleteMessageBatchResultEntry><Id>0</Id></DeleteMessageBatchResultEntry></DeleteMessageBatchResult></DeleteMessageBatchResponse>".to_string()
                            }
                            action => panic!("unexpected SQS action {action}"),
                        }
                    },
                ),
            )
            .with_state(deleted.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listen");
        let endpoint = format!("http://{}", listener.local_addr().expect("address"));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
        let config = AwsClientConfig::mock().with_service_overrides(ServiceOverrides {
            endpoints: HashMap::from([("sqs".to_string(), endpoint.clone())]),
        });
        let credentials = AwsCredentialProvider::from_config(config)
            .await
            .expect("credentials");
        let queue = AwsSqsQueue::new(
            endpoint,
            SqsClient::new(reqwest::Client::new(), credentials),
        );

        let messages = queue.receive("q", 3).await.expect("receive");

        assert_eq!(*deleted.lock().expect("deleted"), ["test-event"]);
        assert_eq!(messages.len(), 3);
        let mut keys = Vec::new();
        for message in &messages[..2] {
            assert_eq!(message.receipt_handle, "two-records");
            assert_eq!(message.attempt, 2);
            let MessagePayload::Json(value) = &message.payload else {
                panic!("S3 records must become StorageEvent JSON");
            };
            let event: StorageEvent = serde_json::from_value(value.clone()).expect("StorageEvent");
            assert_eq!(event.event_type, StorageEventType::Created);
            assert_eq!(event.bucket_name, "n2081-s3d-78f48e");
            keys.push(event.object_key);
        }
        assert_eq!(keys, ["in/with space.json", "in/plus+sign.json"]);
        assert_eq!(messages[2].receipt_handle, "ordinary");
        assert!(matches!(&messages[2].payload, MessagePayload::Text(text) if text == "plain text"));
        server.abort();
        assert!(server.await.expect_err("aborted").is_cancelled());
    }

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
    fn sqs_dns_uses_wire_answers_and_caches_both_address_families() {
        let threads = Arc::new(AtomicUsize::new(0));
        let started = threads.clone();
        let runtime = Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .on_thread_start(move || {
                started.fetch_add(1, Ordering::SeqCst);
            })
            .build()
            .unwrap();
        runtime.block_on(async {
            let socket = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let endpoint = socket.local_addr().unwrap();
            let queries = Arc::new(AtomicUsize::new(0));
            let seen = queries.clone();
            let server = tokio::spawn(async move {
                let mut buffer = [0; 2048];
                loop {
                    let (length, peer) = socket.recv_from(&mut buffer).await.unwrap();
                    seen.fetch_add(1, Ordering::SeqCst);
                    let mut end = 12;
                    while buffer[end] != 0 {
                        end += usize::from(buffer[end]) + 1;
                    }
                    let end = end + 5;
                    assert!(end <= length);
                    let kind = u16::from_be_bytes([buffer[end - 4], buffer[end - 3]]);
                    let missing = &buffer[13..20] == b"missing";
                    let ttl = if &buffer[13..20] == b"expired" {
                        0u32
                    } else {
                        60u32
                    };
                    let mut response = buffer[..12].to_vec();
                    response[2..12].copy_from_slice(&[
                        0x81,
                        if missing { 0x83 } else { 0x80 },
                        0,
                        1,
                        0,
                        u8::from(!missing),
                        0,
                        0,
                        0,
                        0,
                    ]);
                    response.extend_from_slice(&buffer[12..end]);
                    if !missing {
                        let address = match kind {
                            1 => vec![127, 0, 0, 1],
                            28 => std::net::Ipv6Addr::LOCALHOST.octets().to_vec(),
                            _ => panic!("unexpected record type {kind}"),
                        };
                        response.extend_from_slice(&[0xc0, 0x0c]);
                        response.extend_from_slice(&kind.to_be_bytes());
                        response.extend_from_slice(&[0, 1]);
                        response.extend_from_slice(&ttl.to_be_bytes());
                        response.extend_from_slice(&[0, address.len() as u8]);
                        response.extend(address);
                    }
                    socket.send_to(&response, peer).await.unwrap();
                }
            });
            let mut nameserver = hickory_resolver::config::NameServerConfig::udp(endpoint.ip());
            nameserver.connections[0].port = endpoint.port();
            let config =
                hickory_resolver::config::ResolverConfig::from_name_servers(vec![nameserver]);
            let mut builder = TokioResolver::builder_with_config(
                config,
                hickory_resolver::net::runtime::TokioRuntimeProvider::default(),
            );
            builder.options_mut().ip_strategy = LookupIpStrategy::Ipv4AndIpv6;
            let resolver = SqsDnsResolver(builder.build().unwrap());
            tokio::time::timeout(Duration::from_secs(5), async {
                let addresses: HashSet<_> = resolver
                    .resolve("queue.fixture.test.".parse().unwrap())
                    .await
                    .unwrap()
                    .collect();
                assert_eq!(
                    addresses,
                    HashSet::from(["127.0.0.1:0".parse().unwrap(), "[::1]:0".parse().unwrap()])
                );
                assert_eq!(queries.load(Ordering::SeqCst), 2);
                assert!(resolver
                    .resolve("missing.fixture.test.".parse().unwrap())
                    .await
                    .is_err());
                let requests = queries.load(Ordering::SeqCst);
                for _ in 0..64 {
                    let cached: HashSet<_> = resolver
                        .resolve("queue.fixture.test.".parse().unwrap())
                        .await
                        .unwrap()
                        .collect();
                    assert_eq!(cached, addresses);
                }
                assert_eq!(
                    queries.load(Ordering::SeqCst),
                    requests,
                    "cached lookups must not send more DNS queries"
                );
                // A zero-TTL answer is already expired; no short wall-clock window is needed.
                for attempt in 1..=2 {
                    let expired: HashSet<_> = resolver
                        .resolve("expired.fixture.test.".parse().unwrap())
                        .await
                        .unwrap()
                        .collect();
                    assert_eq!(expired, addresses);
                    assert_eq!(
                        queries.load(Ordering::SeqCst),
                        requests + attempt * 2,
                        "expired answers must be resolved again"
                    );
                }
            })
            .await
            .expect("wire DNS must finish without blocking");
            server.abort();
            assert!(server.await.unwrap_err().is_cancelled());
            assert!(
                threads.load(Ordering::SeqCst) <= 2,
                "DNS must not create blocking workers"
            );
        });
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
