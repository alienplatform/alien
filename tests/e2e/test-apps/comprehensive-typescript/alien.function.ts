import * as alien from "@alienplatform/core"

const storage = new alien.Storage("alien-storage").build()
const vault = new alien.Vault("alien-vault").build()
const kv = new alien.Kv("alien-kv").build()
const queue = new alien.Queue("alien-queue").build()

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
  .link(vault)
  .link(kv)
  .link(queue)
  .commandsEnabled(true)
  .build()

// Event subscriptions (uncomment when supported by @alienplatform/core)
// storage.onEvent("*", fn)
// queue.onMessage(fn)

const stack = new alien.Stack("alien-ts-stack")
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
