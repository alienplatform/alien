import * as alien from "@alienplatform/core"

const storage = new alien.Storage("test-alien-storage").build()
const artifactRegistry = new alien.ArtifactRegistry("test-alien-artifact-registry").build()
const vault = new alien.Vault("test-alien-vault").build()
const kv = new alien.Kv("test-alien-kv").build()
const queue = new alien.Queue("test-alien-queue").build()

const build = new alien.Build("test-alien-build")
  .environment({ TEST_VAR: "test-value" })
  .permissions("build-execution")
  .build()

const fn = new alien.Function("test-alien-function")
  .code({
    type: "source",
    src: "./",
    toolchain: {
      type: "rust",
      binaryName: "alien-test-server",
    },
  })
  .memoryMb(512)
  .timeoutSeconds(180)
  .permissions("execution")
  .ingress("public")
  .environment({ RUST_LOG: "info" })
  .readinessProbe({ method: "GET", path: "/hello" })
  .link(storage)
  .link(build)
  .link(artifactRegistry)
  .link(vault)
  .link(kv)
  .link(queue)
  .build()

// Storage event subscription for testing storage events
// storage.onEvent("*", fn)

// // Queue message subscription for testing queue message delivery
// queue.onMessage(fn)

const stack = new alien.Stack("test-alien-stack")
  .permissions({
    profiles: {
      execution: {
        "*": [
          "storage/data-read",
          "storage/data-write",
          "build/execute",
          "artifact-registry/pull",
          "artifact-registry/push",
          "artifact-registry/provision",
          "vault/data-read",
          "vault/data-write",
          "kv/data-read",
          "kv/data-write",
          "queue/data-read",
          "queue/data-write",
          "function/execute",
        ],
      },
      "build-execution": {
        "*": ["build/logs-and-artifacts"],
      },
    },
  })
  .add(storage, "frozen")
  .add(build, "frozen")
  .add(artifactRegistry, "frozen")
  .add(vault, "frozen")
  .add(kv, "frozen")
  .add(queue, "frozen")
  .add(fn, "live")
  .build()

export default stack
