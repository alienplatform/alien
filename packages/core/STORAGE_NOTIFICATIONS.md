# Storage notifications into queues

Storage notification declarations, validation, GCP/Azure Terraform setup and
controllers, and GCP/Azure queue payload normalization are available in this
branch. AWS provisioning and local delivery are still under development.

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

The intended delivery paths are S3 → EventBridge → SQS, Cloud Storage → Pub/Sub,
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
reusing the storage account's shared system topic. Each Storage owns only its
subscriptions; updates and deletion preserve subscriptions managed elsewhere.
Default Event Grid destination authentication requires no extra queue-side grant,
so adding notifications does not update the Queue's IAM. Live Storage adds the
existing `storage/trigger-management` permission set to management permissions;
Frozen rules keep those writes in setup. Retained-storage cleanup waits until
Azure confirms its subscriptions are gone and preserves the shared topic.

AWS EventBridge introduces event-delivery charges beyond SQS; enabling it on a
bucket sends supported S3 events to EventBridge even when rules filter them out.

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

Delivery is at least once and unordered. Overwrites produce another event, and
redelivery or overlapping rules can produce duplicates. The object is the record;
the notification is a hint to read it. Make processing idempotent, for example by
tracking `(objectKey, etag)` when an ETag is available.
