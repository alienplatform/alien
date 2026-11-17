import * as alien from "@alienplatform/core"

// `tasks` moves a message to `tasks-failed` after five failed deliveries. Setting
// DLQ_E2E_NO_DEAD_LETTER=1 declares the same stack without the setting, to show what a
// message that always fails does without it.
const failed = new alien.Queue("tasks-failed").build()
const tasksBuilder = new alien.Queue("tasks")
const tasks = (
  process.env.DLQ_E2E_NO_DEAD_LETTER === "1"
    ? tasksBuilder
    : tasksBuilder.deadLetter(failed, { maxAttempts: 5 })
).build()
const results = new alien.Storage("results").build()
const worker = new alien.Worker("consumer")
  .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
  .permissions("execution")
  .publicEndpoint("api")
  .readinessProbe({ method: "GET", path: "/health" })
  .timeoutSeconds(Number(process.env.DLQ_E2E_WORKER_TIMEOUT ?? 30))
  .link(tasks)
  .link(failed)
  .link(results)
  .trigger({ type: "queue", queue: tasks.ref(), batch: { maxMessages: 10, maxWindowSeconds: 5 } })
  .build()

export default new alien.Stack("queue-dead-letter")
  .platforms(["aws", "gcp", "azure", "local"])
  .add(failed, "frozen")
  .add(tasks, "frozen")
  .add(results, "frozen")
  .add(worker, "live")
  .permissions({
    profiles: {
      execution: {
        tasks: ["queue/data-read", "queue/data-write"],
        "tasks-failed": ["queue/data-read", "queue/data-write"],
        results: ["storage/data-read", "storage/data-write"],
      },
    },
  })
  .build()
