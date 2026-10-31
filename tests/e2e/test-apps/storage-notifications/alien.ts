import * as alien from "@alienplatform/core"

const jobs = new alien.Queue("jobs").build()
const objects = new alien.Storage("objects")
  .notify(jobs, { prefix: "in/", suffix: ".json" })
  .notify(jobs, { prefix: "other/", suffix: ".data" })
  .notify(jobs, { prefix: "other/", suffix: ".csv" })
  .build()
const second = new alien.Storage("second")
  .notify(jobs, { prefix: "shared/", suffix: ".json" })
  .build()
const results = new alien.Storage("results").build()
const worker = new alien.Worker("consumer")
  .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
  .permissions("execution")
  .publicEndpoint("api")
  .readinessProbe({ method: "GET", path: "/health" })
  .timeoutSeconds(120)
  .link(objects)
  .link(second)
  .link(results)
  .link(jobs)
  .trigger({ type: "queue", queue: jobs.ref(), batch: { maxMessages: 10, maxWindowSeconds: 5 } })
  .build()

export default new alien.Stack("storage-notifications")
  .platforms(["aws", "gcp", "azure", "local"])
  .add(jobs, "frozen")
  .add(objects, "frozen")
  .add(second, "frozen")
  .add(results, "frozen")
  .add(worker, "live")
  .permissions({
    profiles: {
      execution: {
        objects: ["storage/data-read", "storage/data-write"],
        second: ["storage/data-read", "storage/data-write"],
        results: ["storage/data-read", "storage/data-write"],
        jobs: ["queue/data-read"],
      },
    },
  })
  .build()
