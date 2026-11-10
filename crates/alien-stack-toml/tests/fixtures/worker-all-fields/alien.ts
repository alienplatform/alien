import * as alien from "@alienplatform/core"

const jobs = new alien.Queue("jobs").build()
const uploads = new alien.Storage("uploads").build()
const state = new alien.Kv("state").build()

const processor = new alien.Worker("processor")
  .code({
    type: "source",
    src: "workers/processor",
    toolchain: { type: "typescript", binaryName: "processor" },
  })
  .commandsEnabled(true)
  .concurrencyLimit(25)
  .memoryMb(2048)
  .timeoutSeconds(600)
  .readinessProbe({ method: "GET", path: "/ready" })
  .publicEndpoint("hooks")
  .trigger({
    type: "queue",
    queue: jobs.ref(),
    batch: { maxMessages: 20, maxWindowSeconds: 45 },
  })
  .trigger({ type: "storage", storage: uploads.ref(), events: ["created"] })
  .trigger({ type: "schedule", cron: "0 */6 * * *" })
  .link(jobs)
  .link(uploads)
  .link(state)
  .permissions("processor")
  .environment({ BATCH_LIMIT: "500", REGION: "global" })
  .build()

export default new alien.Stack("acme-processor")
  .platforms(["aws", "gcp", "azure"])
  .add(jobs, "frozen")
  .add(uploads, "frozen")
  .add(state, "frozen")
  .add(processor, "live")
  .permissions({
    profiles: {
      processor: {
        jobs: ["queue/data-read"],
        uploads: ["storage/data-read"],
        state: ["kv/data-write"],
      },
    },
  })
  .build()
