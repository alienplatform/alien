import * as alien from "@alienplatform/core"

const storage = new alien.Storage("alien-storage").build()
const artifactRegistry = new alien.ArtifactRegistry("test-alien-artifact-registry").build()
const vault = new alien.Vault("alien-vault").build()
const kv = new alien.Kv("alien-kv").build()
const queue = new alien.Queue("alien-queue").build()

const build = new alien.Build("test-alien-build")
  .environment({ TEST_VAR: "test-value" })
  .permissions("build-execution")
  .build()

const container = new alien.Container("alien-rs-ctr")
  .code({
    type: "source",
    src: "./",
    toolchain: {
      type: "rust",
      binaryName: "alien-test-server",
    },
  })
  .memory("512Mi")
  .cpu(0.5)
  .permissions("execution")
  .port(3000)
  .expose("http")
  .environment({ RUST_LOG: "info" })
  .readinessProbe({ method: "GET", path: "/hello" })
  .link(storage)
  .link(build)
  .link(artifactRegistry)
  .link(vault)
  .link(kv)
  .link(queue)
  .build()

// Note: Containers don't support event subscriptions
// No storage.onEvent() or queue.onMessage() for containers

const stack = new alien.Stack("alien-rs-stack")
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
  .add(container, "live")
  .build()

export default stack
