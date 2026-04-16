import * as alien from "@alienplatform/core"

const events = new alien.Kv("events").build()

const api = new alien.Function("api")
  .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
  .commandsEnabled(true)
  .ingress("public")
  .link(events)
  .permissions("execution")
  .build()

export default new alien.Stack("webhook-api")
  .add(events, "frozen")
  .add(api, "live")
  .permissions({
    profiles: {
      execution: {
        "*": ["kv/data-read", "kv/data-write"],
      },
    },
  })
  .build()
