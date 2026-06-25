import * as alien from "@alienplatform/core"

// Postgres is Local-only in the OSS e2e suite: only the embedded Local controller ships in this repo
// (the managed-cloud backends land in a later release), so declaring it on a cloud target would ask
// the executor to provision a backend with no registered controller. Gate on the target platform the
// e2e harness exposes to config evaluation.
const isLocal = process.env.ALIEN_TARGET_PLATFORM === "local"

const storage = new alien.Storage("alien-storage").build()
const artifactRegistry = new alien.ArtifactRegistry("test-alien-artifact-registry").build()
const vault = new alien.Vault("alien-vault").build()
const kv = new alien.Kv("alien-kv").build()
const queue = new alien.Queue("alien-queue").build()
const serviceAccount = new alien.ServiceAccount("test-alien-sa").build()
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
  .link(artifactRegistry)
  .link(vault)
  .link(kv)
  .link(queue)
  .link(serviceAccount)
  .commandsEnabled(true)
if (postgres) {
  workerBuilder = workerBuilder.link(postgres)
}
const worker = workerBuilder.build()

const executionPermissions = [
  "storage/data-read",
  "storage/data-write",
  "artifact-registry/pull",
  "artifact-registry/push",
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
        "test-alien-sa": ["service-account/impersonate"],
      },
    },
  })
  .add(storage, "frozen")
  .add(artifactRegistry, "frozen")
  .add(vault, "frozen")
  .add(kv, "frozen")
  .add(queue, "frozen")
  .add(serviceAccount, "frozen")
if (postgres) {
  stackBuilder = stackBuilder.add(postgres, "live")
}
const stack = stackBuilder.add(worker, "live").build()

export default stack
