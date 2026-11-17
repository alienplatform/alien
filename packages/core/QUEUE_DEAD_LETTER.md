# Dead-letter queues

A Queue can move a message to another Queue after it fails to be processed a number
of times. Without it, a message that always fails behaves differently on each cloud
(retried until retention ends, retried for days, or parked where nothing reads it).

```ts
import * as alien from "@alienplatform/core"

const failed = new alien.Queue("tasks-failed").build()
const tasks = new alien.Queue("tasks").deadLetter(failed, { maxAttempts: 5 }).build()

const worker = new alien.Worker("consumer")
  .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
  .permissions("execution")
  .link(tasks)
  .link(failed)
  .trigger({ type: "queue", queue: tasks.ref() })
  .build()

export default new alien.Stack("jobs")
  .add(failed, "frozen")
  .add(tasks, "frozen")
  .add(worker, "live")
  .permissions({
    profiles: {
      execution: {
        tasks: ["queue/data-read"],
        "tasks-failed": ["queue/data-read", "queue/data-write"],
      },
    },
  })
  .build()
```

`maxAttempts` is 5 to 100 and defaults to 5. The same setting in `alien.toml`:

```toml
[queue.tasks]
dead-letter = { queue = "tasks-failed", max-attempts = 5 }
```

The target is a normal Queue. Link it, `receive` from it, or trigger a Worker from it:

```ts
import { queue } from "@alienplatform/sdk"

const failedTasks = await queue("tasks-failed").receive(10)
// Inspect, fix, then send the payload to `tasks` again if you want to retry it.
```

## What to expect

- **The payload is kept.** There is no portable failure reason, and no redrive API:
  re-send a message to its original queue yourself.
- **The count is approximate.** It is each cloud's own delivery count. SQS counts every
  receive, including a manual `receive` that is never acknowledged. Pub/Sub is
  best-effort. A batch `retry` counts as a failed delivery. A message may move after a
  few more or fewer deliveries than `maxAttempts`.
- **Delivery stays at least once.** A message can still be delivered more than once
  before it moves. On `alien dev`, a crash while a message moves can leave a copy in both
  queues; it is never lost.
- **A target keeps messages longer on AWS.** A queue that is a dead-letter target keeps
  messages for 14 days, because a moved message keeps its original send time.
- **`receive` reports `attempt`.** On GCP the attempt comes from Pub/Sub once the queue
  has a dead letter; without one it is always 1.
- **Queues without `deadLetter` are unchanged** on every cloud.

## Rules checked when you build

- `maxAttempts` is between 5 and 100.
- The target is a Queue of the same stack, is not the source, and has no dead letter of
  its own.
- Both queues are Frozen or both are Live.
- Kubernetes and machines reject the setting: Alien creates no queue there, so set it on
  the external queue.

## Changing it

Adding, changing or removing `deadLetter` is a normal update. For a Frozen queue, the
change is applied by rerunning the setup (the deployment reports that setup is required);
the same queue is changed in place and the messages in it are kept. A Live queue changes
with the deployment. Removing `deadLetter` clears the setting in the cloud.

An older manager or runtime cannot read a stack that has `deadLetter` and reports an
unknown field `deadLetter`. Update them before releasing a stack that uses it.
