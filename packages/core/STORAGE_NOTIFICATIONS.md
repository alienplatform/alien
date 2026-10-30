# Storage notifications into queues

A Storage can put an event into a Queue every time an object is created or
overwritten. Workers consume those events with an ordinary queue trigger,
optionally batched, or a direct `queue.receive()` loop.

```ts
import * as alien from "@alienplatform/core"

const work = new alien.Queue("work").build()
const objects = new alien.Storage("objects")
  .notify(work, { prefix: "uploads/", suffix: ".json" })
  .notify(work, { prefix: "uploads/", suffix: ".csv" })
  .build()

const ingest = new alien.Worker("ingest")
  .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
  .permissions("execution")
  .link(objects)
  .link(work)
  .trigger({ type: "queue", queue: work.ref(), batch: { maxMessages: 10, maxWindowSeconds: 5 } })
  .build()

export default new alien.Stack("ingest")
  .add(work, "frozen")
  .add(objects, "frozen")
  .add(ingest, "live")
  .permissions({
    profiles: {
      execution: {
        objects: ["storage/data-read"],
        work: ["queue/data-read"],
      },
    },
  })
  .build()
```

```ts
import { onQueueMessage, storageEventFromMessage } from "@alienplatform/sdk"

onQueueMessage("work", async message => {
  const event = await storageEventFromMessage(message)
  // The object is the record: read it, then process idempotently.
  console.log(event.bucketName, event.objectKey, event.etag)
})
```

## Delivery guarantees

- **At least once.** Redelivery, provider retries and overlapping rules can deliver
  the same object more than once. Make processing idempotent, for example by
  recording `(objectKey, etag)` when an ETag is available.
- **Unordered.** Events for different objects, and for successive overwrites of
  one object, can arrive in any order.
- **The object is the record.** An event is a hint to read the object. Read the
  current object rather than trusting event metadata to describe it.
- **Only new writes.** A rule sees objects created after it takes effect. Objects
  that already exist are not replayed, in the cloud or under `alien dev`.
- **Rule changes are not instant.** Providers apply rule changes asynchronously.
  For a short time after setup or an update, events can still match the old
  filter or miss the new one. If uploads can't pause during a rule change,
  reconcile by listing the Storage afterwards.

## Rules

Each `.notify(queue, { prefix?, suffix?, events? })` adds a rule:

- `events` defaults to `["created"]`, the only supported event type. Overwrites
  count as creations.
- Matching is literal and case sensitive: the key must start with `prefix` and end
  with `suffix`. `*` and `\` have no special meaning. Prefix and suffix may overlap
  inside a short key (`prefix: "a/b"`, `suffix: "b.json"` matches `a/b.json`).
- Omit a filter to match everything on that side.
- **Rules on one Storage must not overlap**: no object key may be able to match two
  rules. Two rules overlap when their prefixes overlap (either is omitted, or one starts
  with the other) *and* their suffixes overlap (either is omitted, or one ends with the
  other). `uploads/` + `.json` and `uploads/` + `.csv` are fine; `uploads/` and
  `uploads/images/` with no suffixes overlap. This is S3's own rule, enforced on every
  platform so a stack behaves the same everywhere. To send one set of objects to two
  queues, consume one queue and fan out in your Worker.

Compile-time checks, on every platform:

| Check | Limit |
|---|---|
| Rules per Storage | at most 10, none overlapping (see above) |
| `prefix` | 1–512 UTF-8 bytes when given |
| `suffix` | 1–128 UTF-8 bytes when given |
| Characters | no control characters (U+0000–U+001F, U+007F) |
| Target | a Queue in the same stack; the Storage depends on it |
| Lifecycle | Storage and Queue both Frozen or both Live |
| Worker storage triggers | a Storage with notifications can't also be a direct storage-trigger source; consume its queue instead |
| Platforms | AWS, GCP, Azure and local. Kubernetes and machine deployments reject notifications at compile time |

## Frozen and Live rules

**Frozen** (setup-owned) rules are created by the setup package (CloudFormation or
Terraform). Adding, changing or removing a rule changes setup: a normal deployment
update fails with `DEPLOYMENT_SETUP_REQUIRED` until setup is rerun with the new
package. Deleting the deployment removes the rules. Retained buckets keep their
data.

**Live** rules are reconciled by the Storage controller on every deployment, using
management permissions the stack already grants or adds automatically: on AWS the
Storage's own `storage/provision` (bucket notification configuration) and the
Queue's `queue/provision` (queue policy), `storage/trigger-management` on Azure, and
`queue/storage-notifications` on GCP (for the topic's publisher grant). Removing a rule or the Storage deletes exactly the rules,
targets and subscriptions it created.

Workers that only consume the queue need no extra permissions.

### AWS: setup owns the bucket's notification configuration

Once a Storage declares notifications, **setup owns its S3 notification
configuration.** CloudFormation sets the bucket's
`NotificationConfiguration.QueueConfigurations`, and Terraform manages
`aws_s3_bucket_notification`. Both write the whole document, so Lambda, SQS, SNS or
EventBridge notifications added to that bucket outside Alien are removed the next
time setup runs. Add more notifications as rules instead.

Live controllers instead read, modify and write the bucket's notification
document. They recognise their own entries by Id (`<prefix>-sn-<hash>-<n>`) and
preserve every entry they don't own.

## How each provider delivers

| | AWS | GCP | Azure | Local (`alien dev`) |
|---|---|---|---|---|
| Path | S3 → SQS | Cloud Storage → Pub/Sub | Blob Storage → Event Grid → Service Bus | file watcher → local queue |
| One rule is | an S3 queue configuration (`s3:ObjectCreated:*`, prefix/suffix filter) | a bucket notification config | an Event Grid subscription on the storage account's system topic | a watcher with its own cursor |
| Prefix filter | at the provider | at the provider | at the provider | in the watcher |
| Suffix filter | at the provider | **in the consumer** (see below) | at the provider | in the watcher |
| Queue grant | one `StorageNotificationIngress` statement on the queue policy: `s3.amazonaws.com`, this account, exactly the notifying buckets | Cloud Storage service agent as publisher on the topic, once per queue | none needed | none |
| `timestamp` | S3 `eventTime` | GCS event time | Event Grid event time | object modification time |

Provider details that matter in production:

- **AWS cost.** S3 delivers directly to SQS: each matching object costs one SQS
  message, and objects no rule matches cost nothing. There is no EventBridge in the
  path. SSE-SQS encrypted queues work as destinations.
- **AWS messages.** S3 URL-encodes keys (`+` for a space); the Worker runtime and
  queue binding decode them, so `objectKey` is the literal key. A message with
  several records becomes one event per record. When a notification configuration
  changes, S3 also sends an `s3:TestEvent` message; consumers acknowledge it and
  never hand it to your code.
- **GCP suffix filtering happens in the consumer.** Cloud Storage filters only by
  prefix. The suffix travels as a message attribute, and the Worker runtime or
  `queue.receive()` acknowledges non-matching messages without handing them to
  your code. Every object under the prefix still costs one Pub/Sub message, and
  with push delivery one Worker request. On a pull binding those messages also
  use up slots in `receive(max)`, so a call can return fewer messages than
  available, or none. Pick prefixes that are selective on their own.
- **GCP quota.** Cloud Storage allows 10 notification configs per bucket per event
  type, and that includes configs created outside Alien.
- **Azure.** All subscriptions share the storage account's one system topic, which
  Azure creates when the first subscription is added. Alien never deletes the
  topic, and subscriptions created elsewhere are left alone.
- **Messages that are not notifications** in the same queue are delivered
  unchanged on every provider. A notification that can't be parsed is also
  delivered unchanged, rather than failing the batch it arrived in, and the SDK
  helpers below report it.

## The event payload

Every provider delivers the same JSON shape (`StorageEvent`):

| Field | Present | Notes |
|---|---|---|
| `eventType` | always | `"created"` |
| `bucketName` | always | the physical bucket or container name |
| `objectKey` | always | the literal key, never URL-decoded |
| `timestamp` | always | RFC 3339, UTC |
| `size` | AWS, GCP, Azure, local | bytes |
| `etag` | AWS, GCP, Azure | the provider's own format (S3 MD5 hex, GCS base64, Azure `0x…`). Compare ETags only within one provider |
| `contentType` | GCP, Azure | |
| `versionId`, `currentTier`, `metadata` | GCP | generation, storage class, custom metadata |

Code that must run everywhere should rely only on the fields marked "always" and
treat the rest as optional.

## Reading events in code

TypeScript, for Worker handlers and messages from `queue.receive()`:

```ts
import { storageEventFromMessage } from "@alienplatform/sdk"

const event = await storageEventFromMessage(message)
```

Rust (binding messages, single Worker messages and messages inside batches):

```rust
use alien_sdk::storage_event_from_message;

let event = storage_event_from_message(&message)?;
println!("{} {}", event.bucket_name, event.object_key);
```

Python (`QueueMessage` from bindings, `QueueEvent` from Worker handlers):

```python
from alienplatform import storage_event_from_message

event = storage_event_from_message(message)
print(event.bucket_name, event.object_key, event.timestamp)
```

Each helper validates the payload and fails with `INVALID_STORAGE_EVENT_MESSAGE`
if it isn't a storage event. None of them acknowledges the message: with a queue
binding, acknowledge only after processing succeeds. With a Worker trigger, the
runtime acknowledges when the handler returns.

## Local development

`alien dev` delivers notifications from the storage manager, independently of
Workers: stopping a Worker doesn't stop events from reaching the queue.

- Each rule keeps an acknowledged cursor under the deployment's data directory.
  A new rule starts from the objects that exist when it starts.
- Restarting `alien dev` replays nothing that was already delivered. Objects
  written or overwritten while it was stopped are delivered once it is back.
  Keep the data directory to keep this state.
- Filesystem events trigger delivery, and a scan every 30 seconds catches anything
  they missed. The watcher compares modification time and size, so it sees the
  latest committed state of each object. An object created and deleted between
  two scans, or overwritten twice, produces at most one event.
- A crash between delivering and recording an event redelivers it, so local
  delivery is also at least once.
