import * as alien from "@alienplatform/core"

// `tasks` moves a message to `tasks-failed` after five failed deliveries.
const failed = new alien.Queue("tasks-failed").build()
const tasks = new alien.Queue("tasks").deadLetter(failed, { maxAttempts: 5 }).build()
const results = new alien.Storage("results").build()
const worker = new alien.Worker("consumer")
  .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
  .permissions("execution")
  .publicEndpoint("api")
  .readinessProbe({ method: "GET", path: "/health" })
  .timeoutSeconds(10)
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
