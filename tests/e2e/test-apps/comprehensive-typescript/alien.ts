import * as alien from "@alienplatform/core"

const storage = new alien.Storage("alien-storage").build()
const artifactRegistry = new alien.ArtifactRegistry("test-alien-artifact-registry").build()
const vault = new alien.Vault("alien-vault").build()
const kv = new alien.Kv("alien-kv").build()
const queue = new alien.Queue("alien-queue").build()
const serviceAccount = new alien.ServiceAccount("test-alien-sa").build()

const fn = new alien.Function("alien-ts-fn")
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
  .ingress("public")
  .environment({ NODE_ENV: "production" })
  .readinessProbe({ method: "GET", path: "/hello" })
  .link(storage)
  .link(artifactRegistry)
  .link(vault)
  .link(kv)
  .link(queue)
  .link(serviceAccount)
  .commandsEnabled(true)
  .build()

const stack = new alien.Stack("alien-ts-stack")
  .permissions({
    profiles: {
      execution: {
        "*": [
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
          "function/execute",
        ],
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
  .add(fn, "live")
  .build()

export default stack
