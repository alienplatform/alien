import * as alien from "@alienplatform/core"

const credentials = new alien.Vault("credentials").build()
const cache = new alien.Kv("cache").build()

const connector = new alien.Function("connector")
  .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
  .commandsEnabled(true)
  .ingress("private")
  .link(credentials)
  .link(cache)
  .permissions("execution")
  .build()

export default new alien.Stack("data-connector")
  .add(credentials, "frozen")
  .add(cache, "frozen")
  .add(connector, "live")
  .permissions({
    profiles: {
      execution: {
        "*": ["vault/data-read", "kv/data-read", "kv/data-write"],
      },
    },
  })
  .build()
