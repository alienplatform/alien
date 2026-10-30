# Storage notifications into queues

Storage notifications support AWS CloudFormation and Terraform setup, GCP/Azure
Terraform setup, Live controllers, and local delivery. GCP/Azure queue consumers
normalize provider messages into the same event payload.

Declare a queue destination on a Storage resource:

```ts
import { Queue, Stack, Storage } from "@alienplatform/core"

const work = new Queue("work").build()
const objects = new Storage("objects")
  .notify(work, { prefix: "uploads/", suffix: ".json" })
  .notify(work, { prefix: "uploads/", suffix: ".csv" })
  .build()

export default new Stack("ingest")
  .add(work, "frozen")
  .add(objects, "frozen")
  .build()
```

Each rule defaults to `events: ["created"]`, the only supported event selection.
Prefix and suffix matching is literal and case sensitive: `*` and backslashes
have no special meaning. A key must start with the prefix and end with the
suffix. The two filters may overlap within the key, and multiple matching rules
each produce a delivery.

## Limits and ownership

- At most 10 rules per Storage resource.
- A supplied prefix must contain 1–512 UTF-8 bytes; a supplied suffix must contain
  1–128 UTF-8 bytes. Omit a filter to match every prefix or suffix.
- Filters cannot contain control characters U+0000–U+001F or U+007F. These
  declarations fail at compile time on every platform.
- The target must be a Queue in the same stack. Storage depends on its queues.
- Storage and Queue must have the same lifecycle: both Frozen or both Live.
- A Storage with notifications cannot also be a Worker's direct storage-trigger
  source. Use a queue trigger, optionally with batching, to consume its events.
- Kubernetes and machines do not support these declarations and reject them
  before deployment.

Frozen rules are setup-owned. Adding, editing or removing a rule requires rerunning
setup; an ordinary deployment update returns `DEPLOYMENT_SETUP_REQUIRED`. Live
rules are owned by the Storage controller. Consumers need their existing queue
read permissions; they do not manage notification rules.

AWS rules use EventBridge. The complete event pattern must fit within 4,096
characters. Compile-time validation reserves space for a full 63-character S3
bucket name. Ordinary nonoverlapping filters fit even when JSON escaping expands
them. Highly repetitive overlapping filters can produce many exact alternatives
and exceed the limit; shorten the filters or reduce their overlap. AWS rejects
these declarations at compile time with the Storage ID and rule index.
EventBridge also applies its own wildcard-complexity checks; unusually repetitive
suffixes can be rejected by AWS even when the pattern fits the size limit.
S3 EventBridge activation and rule changes propagate asynchronously. A successful
setup or update does not mean the new filter is already effective: events can
still match the old rule or miss the new rule during propagation. Allow the
provider changes to settle before relying on a new filter; reconcile objects
from Storage when uploads cannot be paused during a rule change. See the
[AWS rule update semantics](https://docs.aws.amazon.com/eventbridge/latest/APIReference/API_PutRule.html).

The delivery paths are S3 → EventBridge → SQS, Cloud Storage → Pub/Sub,
and Blob Storage → Event Grid → Service Bus. GCP suffix filtering happens in the
consumer: nonmatching messages are acknowledged without invoking the handler.
GCP setup and Live controllers create independent notification configurations and
a single publisher grant per destination queue for the project's Cloud Storage
service agent. The queue updates its grant when the first rule starts targeting it
or the last rule stops; unchanged deployments do not rewrite the policy. Rule
updates and teardown leave independently managed notification configurations
alone. The bucket follows the existing setup data-retention policy; removing
notifications does not delete stored objects. Existing out-of-band configurations
also count toward provider notification quotas.
Azure setup and controllers create account-scoped Event Grid subscriptions,
reusing the storage account's shared system topic. Azure creates that topic
automatically if it is absent; the controller does not need a system-topic write
grant and does not own or delete the shared topic. Each Storage owns only its
subscriptions; updates and deletion preserve subscriptions managed elsewhere.
Default Event Grid destination authentication requires no extra queue-side grant
or delivery identity. Existing queue provisioning grants suffice for destination
validation, so adding notifications does not update the Queue's IAM. Live Storage adds the
existing `storage/trigger-management` permission set to management permissions;
Frozen rules keep those writes in setup. Retained-storage cleanup waits until
Azure confirms its subscriptions are gone and leaves unrelated subscriptions
and their shared topic in place. Azure can remove an automatically created topic
after its last subscription is deleted.

Once a Storage declares notifications, setup owns its S3 notification
configuration. Add more notifications as rules, not out of band. CloudFormation
uses the bucket's native EventBridge property; Terraform uses
`aws_s3_bucket_notification`. Setup may replace independently authored Lambda,
SQS or SNS entries on that bucket. The compile-time check forbids mixing these
notifications with a Worker storage trigger on the same source.

Live controllers instead enable EventBridge through a read/modify/write of the
complete bucket notification document, preserving independent entries and filters.
Setup owns the destination queue policy document; preserve extra statements in
that setup definition. Live controllers merge only their owned statement.

Each Storage owns its EventBridge rules and queue targets. The Queue owns one
`StorageNotificationIngress` policy statement, scoped to this account and stack's
rules. Live reconciliation preserves unrelated statements, and unchanged queues
do not read or rewrite the policy. Live notifying Storage adds `storage/queue-notifications` management
permissions; ordinary Storage does not receive these EventBridge grants.

Live rule removal and retained-storage cleanup delete owned targets and rules but
leave the shared EventBridge switch enabled, because independent rules may use it.
A bucket retained by CloudFormation also keeps its EventBridge setting. Charges
can therefore continue on these retained buckets; disable the switch separately
only after confirming no other rules need those events. Terraform removes its
owned notification configuration during teardown. Erasing a bucket removes its
configuration along with the data.

AWS EventBridge introduces event-delivery charges beyond SQS; enabling it on a
bucket sends supported S3 events to EventBridge even when rules filter them out.

## Local development

`alien dev` runs notification delivery in the storage manager, independently of
Workers. Each matching rule puts a canonical event into the local queue. Stopping
a Worker does not stop queue intake. Rule changes stop the old watcher before
starting the new one, and removing or retaining Storage stops its delivery tasks.

Each rule and each direct Worker storage trigger keeps an acknowledged cursor
under the deployment's data directory. Restarting the manager or Worker does not
replay unchanged acknowledged objects. New or overwritten objects retained on disk
while it was stopped are delivered after restart. Keep the data directory to keep
these cursors. Delivery and checkpoint writes are separate: a crash between them
can produce a duplicate, so consumers must remain idempotent.

Filesystem notifications wake reconciliation; a 30-second safety scan covers
missed hints. Reconciliation compares modification time and size and observes the
latest committed state, so it cannot reconstruct intermediate overwrites or
objects created and deleted entirely between scans. Staged multipart files do not
emit events until committed. Cursors retain pending failed deliveries; corrupted
cursors fail explicitly instead of silently replaying the bucket.

## Reading notifications

Queue payloads use the portable `StorageEvent` shape: `eventType`, `bucketName`,
`objectKey`, `timestamp`, and optional `size` and `etag`. For TypeScript Worker
handlers or messages returned by `queue.receive()`, validate the payload with:

```ts
import { onQueueMessage, storageEventFromMessage } from "@alienplatform/sdk"

onQueueMessage("work", async message => {
  const event = await storageEventFromMessage(message)
  console.log(event.bucketName, event.objectKey, event.etag)
})
```

The helper accepts parsed Worker payloads and JSON strings from queue bindings.
It throws a structured `INVALID_STORAGE_EVENT_MESSAGE` error for invalid payloads
and does not acknowledge messages. When using a queue binding, acknowledge only
after processing succeeds.

Rust exposes the same parser for binding messages, Worker single messages, and
messages inside Worker batches:

```rust
use alien_sdk::storage_event_from_message;

// `message` is from queue.receive(), a single handler, or a batch delivery.
let event = storage_event_from_message(&message)?;
println!("{} {}", event.bucket_name, event.object_key);
```

It returns the portable `alien_sdk::StorageEvent`, including its timestamp and
optional metadata. Direct binding applications can use it without the `worker`
feature. Existing direct storage-handler event types remain compatible.

Python accepts `QueueMessage` from bindings and `QueueEvent` from single or batched
Worker handlers:

```python
from alienplatform import storage_event_from_message

event = storage_event_from_message(message)
print(event.bucket_name, event.object_key, event.timestamp)
```

Python returns a typed `StorageEvent` with a timezone-aware UTC timestamp. Both
SDK helpers reject invalid event fields with `INVALID_STORAGE_EVENT_MESSAGE` and
leave acknowledgment to the caller or Worker runtime.

Delivery is at least once and unordered. Overwrites produce another event, and
redelivery or overlapping rules can produce duplicates. The object is the record;
the notification is a hint to read it. Make processing idempotent, for example by
tracking `(objectKey, etag)` when an ETag is available.
