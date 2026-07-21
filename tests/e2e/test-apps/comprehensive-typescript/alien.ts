import * as alien from "@alienplatform/core"

// Postgres is Local-only in the OSS e2e suite: only the embedded Local controller ships in this repo,
// so declaring it on a cloud target would ask the executor to provision a backend with no registered
// controller. Gate on the target platform the e2e harness exposes to config evaluation.
const isLocal = process.env.ALIEN_TARGET_PLATFORM === "local"
// Remote Storage is intentionally limited to native AWS/GCP/Azure deployments.
const supportsRemoteStorage = ["aws", "gcp", "azure"].includes(
  process.env.ALIEN_TARGET_PLATFORM ?? "",
)

const storage = new alien.Storage("alien-storage").build()
const vault = new alien.Vault("alien-vault").build()
const kv = new alien.Kv("alien-kv").build()
const queue = new alien.Queue("alien-queue").build()
// Dedicated queue for trigger-delivery tests. `alien-queue` is consumed by the
// app's own send/receive/ack endpoint, so a platform queue trigger on it would
// race that consumer. This queue has exactly one consumer: the queue trigger.
const eventsQueue = new alien.Queue("alien-events-queue").build()
const postgres = isLocal ? new alien.Postgres("alien-postgres").build() : undefined

let workerBuilder = new alien.Worker("alien-ts-worker")
  .code({
    type: "source",
    src: "./",
    toolchain: {
      type: "typescript",
      entrypoint: "dist/index.js",
    },
  })
  .memoryMb(2048)
  .timeoutSeconds(180)
  .permissions("execution")
  .publicEndpoint("api")
  .environment({ NODE_ENV: "production" })
  .readinessProbe({ method: "GET", path: "/hello" })
  .link(storage)
  .link(vault)
  .link(kv)
  .link(queue)
  .link(eventsQueue)
  .commandsEnabled(true)
  // Event triggers under test: each one must invoke the registered handler in
  // src/index.ts, which records the event in `alien-kv` for read-back.
  .trigger({ type: "queue", queue: eventsQueue.ref() })
  .trigger({ type: "storage", storage: storage.ref(), events: ["created"] })
  .trigger({ type: "schedule", cron: "* * * * *" })
if (postgres) {
  workerBuilder = workerBuilder.link(postgres)
}
const worker = workerBuilder.build()

const executionPermissions = [
  "storage/data-read",
  "storage/data-write",
  "vault/data-read",
  "vault/data-write",
  "kv/data-read",
  "kv/data-write",
  "queue/data-read",
  "queue/data-write",
  "worker/execute",
]
if (postgres) {
  executionPermissions.push("postgres/data-access")
}

let stackBuilder = new alien.Stack("alien-ts-stack")
  .permissions({
    profiles: {
      execution: {
        "*": executionPermissions,
      },
    },
  })
  .add(storage, "frozen", { remoteAccess: supportsRemoteStorage })
  .add(vault, "frozen")
  .add(kv, "frozen")
  .add(queue, "frozen")
  .add(eventsQueue, "frozen")
if (postgres) {
  stackBuilder = stackBuilder.add(postgres, "live")
}
const stack = stackBuilder.add(worker, "live").build()

export default stack
