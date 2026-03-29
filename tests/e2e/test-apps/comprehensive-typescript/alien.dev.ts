import * as alien from "@alienplatform/core"

const storage = new alien.Storage("alien-storage").build()
const vault = new alien.Vault("alien-vault").build()
const kv = new alien.Kv("alien-kv").build()

const fn = new alien.Function("alien-ts-fn")
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
  .environment({ NODE_ENV: "development" })
  .readinessProbe({ method: "GET", path: "/hello" })
  .link(storage)
  .link(vault)
  .link(kv)
  .build()

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
