import * as alien from "@alienplatform/core"

// `tasks` moves a message to `tasks-failed` after five failed deliveries. Setting
// DLQ_E2E_NO_DEAD_LETTER=1 declares the same stack without the setting, to show what a
// message that always fails does without it.
const withDeadLetter = process.env.DLQ_E2E_NO_DEAD_LETTER !== "1"
// DLQ_E2E_LIVE=1 makes the four queues Live instead of Frozen.
const queueLifecycle = process.env.DLQ_E2E_LIVE === "1" ? "live" : "frozen"
const failed = new alien.Queue("tasks-failed").build()
const tasksBuilder = new alien.Queue("tasks")
const tasks = (
  withDeadLetter ? tasksBuilder.deadLetter(failed, { maxAttempts: 5 }) : tasksBuilder
).build()
// `pulled` has no trigger: the Worker consumes it with `receive`, which is the pull path.
const pulledFailed = new alien.Queue("pulled-failed").build()
const pulledBuilder = new alien.Queue("pulled")
const pulled = (
  withDeadLetter ? pulledBuilder.deadLetter(pulledFailed, { maxAttempts: 5 }) : pulledBuilder
).build()
const results = new alien.Storage("results").build()
// DLQ_E2E_TWO_QUEUES=1 adds a second dead-letter queue with its own Worker, to show that two
// queues share the one project-level grant and the last one removes it. This is a MANUAL
// fixture: no automated test or CI case sets it, it is run by hand against a real project.
const twoQueues = process.env.DLQ_E2E_TWO_QUEUES === "1"
const second = twoQueues ? new alien.Queue("second-failed").build() : undefined
const secondTasks =
  second !== undefined
    ? new alien.Queue("second").deadLetter(second, { maxAttempts: 5 }).build()
    : undefined
const secondWorker =
  secondTasks !== undefined
    ? new alien.Worker("consumer-second")
        .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
        .permissions("execution")
        // It serves the same app as `consumer` (the harness may talk to either), so it links the
        // same queues and storage.
        .publicEndpoint("api-second")
        .readinessProbe({ method: "GET", path: "/health" })
        .timeoutSeconds(30)
        .link(secondTasks)
        .link(tasks)
        .link(failed)
        .link(pulled)
        .link(pulledFailed)
        .link(results)
        .trigger({
          type: "queue",
          queue: secondTasks.ref(),
          batch: { maxMessages: 10, maxWindowSeconds: 5 },
        })
        .build()
    : undefined
const worker = new alien.Worker("consumer")
  .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
  .permissions("execution")
  .publicEndpoint("api")
  .readinessProbe({ method: "GET", path: "/health" })
  .timeoutSeconds(Number(process.env.DLQ_E2E_WORKER_TIMEOUT ?? 45))
  .link(tasks)
  .link(failed)
  .link(pulled)
  .link(pulledFailed)
  .link(results)
  .trigger({ type: "queue", queue: tasks.ref(), batch: { maxMessages: 10, maxWindowSeconds: 5 } })
  .build()

let stack = new alien.Stack("queue-dead-letter")
  .platforms(["aws", "gcp", "azure", "local"])
  .add(failed, queueLifecycle)
  .add(tasks, queueLifecycle)
  .add(pulledFailed, queueLifecycle)
  .add(pulled, queueLifecycle)
  .add(results, "frozen")
  .add(worker, "live")

if (second !== undefined && secondTasks !== undefined && secondWorker !== undefined) {
  stack = stack
    .add(second, queueLifecycle)
    .add(secondTasks, queueLifecycle)
    .add(secondWorker, "live")
}

export default stack
  .permissions({
    profiles: {
      execution:
        process.env.DLQ_E2E_LIVE === "1"
          ? // AWS rejects resource-scoped data permissions on Live resources.
            {
              "*": [
                "queue/data-read",
                "queue/data-write",
                "storage/data-read",
                "storage/data-write",
              ],
            }
          : {
              tasks: ["queue/data-read", "queue/data-write"],
              "tasks-failed": ["queue/data-read", "queue/data-write"],
              pulled: ["queue/data-read", "queue/data-write"],
              "pulled-failed": ["queue/data-read", "queue/data-write"],
              results: ["storage/data-read", "storage/data-write"],
            },
    },
  })
  .build()
