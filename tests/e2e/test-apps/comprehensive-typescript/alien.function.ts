import * as alien from "@alienplatform/core"

const storage = new alien.Storage("test-alien-storage").build()
const vault = new alien.Vault("test-alien-vault").build()
const kv = new alien.Kv("test-alien-kv").build()
const queue = new alien.Queue("test-alien-queue").build()

const fn = new alien.Function("test-alien-ts-function")
  .code({
    type: "source",
    src: "./",
    toolchain: {
      type: "typescript",
      entrypoint: "dist/index.js",
    },
  })
  .memoryMb(512)
  .timeoutSeconds(180)
  .permissions("execution")
  .ingress("public")
  .environment({ NODE_ENV: "production" })
  .readinessProbe({ method: "GET", path: "/hello" })
  .link(storage)
  .link(vault)
  .link(kv)
  .link(queue)
  .build()

// Event subscriptions (uncomment when supported by @alienplatform/core)
// storage.onEvent("*", fn)
// queue.onMessage(fn)

const stack = new alien.Stack("test-alien-ts-stack")
  .permissions({
    profiles: {
      execution: {
        "*": [
          "storage/data-read",
          "storage/data-write",
          "vault/data-read",
          "vault/data-write",
          "kv/data-read",
          "kv/data-write",
          "queue/data-read",
          "queue/data-write",
          "function/execute",
        ],
      },
    },
  })
  .add(storage, "frozen")
  .add(vault, "frozen")
  .add(kv, "frozen")
  .add(queue, "frozen")
  .add(fn, "live")
  .build()

export default stack
