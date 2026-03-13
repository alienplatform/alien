# Queue

Message queue with at-least-once delivery. Consistent semantics across all platforms with a fixed 30-second lease window.

## Platform Mapping

| Platform | Backend |
|----------|---------|
| AWS | SQS |
| GCP | Cloud Pub/Sub |
| Azure | Service Bus |
| Kubernetes | Redis Streams |
| Local | Redis Streams |

## Design Principles

- **At-least-once delivery**: Messages guaranteed to arrive, may arrive more than once
- **Fixed lease duration**: 30 seconds, not configurable per-call
- **Manual acknowledgment**: Only explicit `ack` removes messages
- **Batch receive**: Up to 10 messages per call
- **JSON/Text payloads**: No raw binary support

### Design Tradeoffs

| Tradeoff | Decision | Rationale |
|----------|----------|-----------|
| Simplicity vs Features | Omit DLQ, FIFO, message attributes | Maintain portability and API stability |
| Throughput vs Portability | Cap batch at 10 | Works everywhere |
| Latency vs Robustness | Fixed 30s lease | No auto-renewal complexity |

## Core API

```rust
pub enum MessagePayload {
    Json(serde_json::Value),
    Text(String),
}

pub struct QueueMessage {
    pub payload: MessagePayload,
    pub receipt_handle: String,  // Opaque, backend-specific
}

pub trait Queue: Send + Sync {
    async fn send(&self, queue: &str, message: MessagePayload) -> Result<()>;
    async fn receive(&self, queue: &str, max_messages: usize) -> Result<Vec<QueueMessage>>;
    async fn ack(&self, queue: &str, receipt_handle: &str) -> Result<()>;
}
```

### Constraints

| Constraint | Limit |
|------------|-------|
| Message size | 64 KB |
| Batch size | 10 messages |
| Lease duration | 30 seconds |

### Acknowledgment

- Call `ack` to permanently remove a message
- If not acknowledged within 30 seconds, the message becomes available again
- `ack` is idempotent—safe to call multiple times with invalid/expired handles

## Function Triggers

Queue messages can trigger Functions. Each invocation receives exactly one message.

```typescript
const queue = new alien.Queue("jobs").build()

const processor = new alien.Function("processor")
  .trigger({ type: "queue", queue })
  .code({ type: "source", src: "./processor" })
  .build()
```

### Platform Integration

| Platform | Mechanism |
|----------|-----------|
| AWS | Lambda Event Source Mapping |
| GCP | Pub/Sub Push Subscription → Cloud Run |
| Azure | Dapr Service Bus → Container Apps + KEDA |

**AWS**: Event source mapping with batch size 1. Lambda deletes message on successful return.

**GCP**: Push subscription POSTs to Cloud Run endpoint. HTTP 2xx = acknowledge.

**Azure**: Dapr component configured with auto lock renewal. KEDA `ScaledObject` provides elastic scaling based on queue depth.

### Visibility Timeout (AWS)

Lambda calculates visibility timeout from function timeout:

```
visibility_timeout = max(30s, min(12h, function_timeout × 6))
```

The 6× multiplier accounts for cold starts and internal retries. The Queue controller analyzes all functions with triggers referencing this queue and uses the maximum required timeout.

### Acknowledgment Flow

Applications don't call `ack` directly when using triggers. HTTP response status determines outcome:

| Response | Result |
|----------|--------|
| 2xx | Message acknowledged |
| 4xx/5xx | Message returns to queue for retry |

## Runtime Integration

alien-runtime transforms platform events into HTTP requests. Applications handle `POST /__queue_message`:

```typescript
app.post('/__queue_message', async (req, res) => {
    const messages = req.body;  // Array of QueueMessage
    
    for (const message of messages) {
        await processMessage(message.payload);
    }
    
    res.status(200).json({ status: 'success' });
});
```

### Message Format

```json
[
  {
    "id": "msg-12345",
    "payload": { "type": "json", "data": { "order_id": 123 } },
    "receiptHandle": "platform-specific-handle",
    "timestamp": "2024-01-01T12:00:00Z",
    "source": "order-processing-queue",
    "attributes": { "priority": "high" },
    "attemptCount": 1
  }
]
```

Fields:
- `id`: Unique message identifier
- `payload`: JSON or Text content
- `receiptHandle`: For manual ack (when not using triggers)
- `timestamp`: When message was sent
- `source`: Queue name
- `attributes`: Platform-specific metadata
- `attemptCount`: Delivery attempt number (if available)

### Event Parsing

alien-runtime parses platform-specific events:

| Platform | Source Event |
|----------|--------------|
| AWS | SQS Event (Lambda) |
| GCP | CloudEvents (Pub/Sub push) |
| Azure | Dapr CloudEvents |

The parsing extracts common fields and normalizes to `QueueMessage` format.

## Idempotency

At-least-once delivery means duplicates are possible. Applications should be idempotent.

**Pattern**: Use message ID as idempotency key

```typescript
app.post('/__queue_message', async (req, res) => {
    const message = req.body[0];
    
    // Use message ID to de-duplicate
    const processed = await db.query(
        'SELECT 1 FROM processed_messages WHERE id = $1',
        [message.id]
    );
    
    if (processed.rows.length > 0) {
        // Already processed, acknowledge without reprocessing
        return res.status(200).json({ status: 'duplicate' });
    }
    
    await processMessage(message);
    await db.query(
        'INSERT INTO processed_messages (id) VALUES ($1)',
        [message.id]
    );
    
    res.status(200).json({ status: 'success' });
});
```

For external APIs, pass the message ID as an idempotency key:

```typescript
await paymentApi.charge({
    amount: message.payload.amount,
    idempotencyKey: message.id,  // API de-duplicates
});
```

## Backend Notes

### SQS

Native visibility timeout maps directly to lease concept. Long polling (20s) reduces empty receives:

```rust
ReceiveMessageInput::builder()
    .visibility_timeout(30)
    .wait_time_seconds(20)
    .max_number_of_messages(10)
```

Invalid receipt handles on `DeleteMessage` return success (idempotent ack).

### Pub/Sub

Uses `ModifyAckDeadline` after pull to set 30-second lease:

```rust
let response = subscriber.pull(request).await?;
subscriber.modify_ack_deadline(ModifyAckDeadlineRequest {
    ack_ids: response.messages.iter().map(|m| m.ack_id.clone()).collect(),
    ack_deadline_seconds: 30,
}).await?;
```

Push subscriptions (for Function triggers) use HTTP request timeout aligned with function timeout.

### Service Bus

Peek-lock mode with queue-level lock duration ≥ 30 seconds:

```rust
// Lock duration configured at queue level
// Complete message using lock token
client.complete_message(queue, lock_token).await?;
```

Lock token errors (expired, invalid) treated as success for idempotent ack.

### Redis Streams (Kubernetes/Local)

Uses consumer groups for at-least-once delivery:

```rust
// Add message
redis::cmd("XADD").arg(stream).arg("*").arg("data").arg(&payload)

// Read with consumer group
redis::cmd("XREADGROUP")
    .arg("GROUP").arg(group).arg(consumer)
    .arg("COUNT").arg(max_messages)
    .arg("BLOCK").arg(timeout_ms)
    .arg("STREAMS").arg(stream).arg(">")

// Acknowledge
redis::cmd("XACK").arg(stream).arg(group).arg(message_id)
```

## Runtime Configuration

```toml
# alien-runtime.toml for AWS Lambda + SQS
[transports.lambda]
type = "lambda"
mode = "streaming"

[handler]
type = "process"
command = "node"
args = ["server.js"]
target_addr = "127.0.0.1:8080"
transports = ["lambda"]
```

```toml
# alien-runtime.toml for GCP Cloud Run + Pub/Sub
[transports.http-server]
type = "http-server"
addr = "0.0.0.0:8080"

[handler]
type = "process"
command = "node"
args = ["server.js"]
target_addr = "127.0.0.1:8080"
transports = ["http-server"]
# CloudEvents middleware auto-enabled
```

## Pull-Based Consumers

For background workers that poll the queue:

```rust
loop {
    let messages = queue.receive("jobs", 10).await?;
    
    for message in messages {
        match process(&message).await {
            Ok(_) => {
                queue.ack("jobs", &message.receipt_handle).await?;
            }
            Err(e) => {
                // Don't ack - message will reappear after 30s
                log::error!("Processing failed: {}", e);
            }
        }
    }
    
    if messages.is_empty() {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
```

Process time must be under 30 seconds or the message reappears mid-processing.


