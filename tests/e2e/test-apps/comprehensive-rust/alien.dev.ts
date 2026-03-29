import * as alien from "@alienplatform/core"

// Simplified config for local dev - excludes Build and Queue resources
// that don't have local controllers

const storage = new alien.Storage("alien-storage").build()
const vault = new alien.Vault("alien-vault").build()
const kv = new alien.Kv("alien-kv").build()

const fn = new alien.Function("alien-rs-fn")
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
  .environment({ RUST_LOG: "info", NODE_ENV: "development" })
  .readinessProbe({ method: "GET", path: "/hello" })
  .link(storage)
  .link(vault)
  .link(kv)
  .build()

const stack = new alien.Stack("alien-rs-stack")
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
          "function/execute",
        ],
      },
    },
  })
  .add(storage, "frozen")
  .add(vault, "frozen")
  .add(kv, "frozen")
  .add(fn, "live")
  .build()

export default stack
