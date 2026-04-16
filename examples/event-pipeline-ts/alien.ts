import * as alien from "@alienplatform/core"

const inbox = new alien.Queue("inbox").build()
const data = new alien.Storage("data").build()
const events = new alien.Kv("events").build()

const processor = new alien.Function("processor")
  .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
  .commandsEnabled(true)
  .ingress("private")
  .link(inbox)
  .link(data)
  .link(events)
  .trigger({ type: "queue", queue: inbox.ref() })
  .trigger({ type: "storage", storage: data.ref(), events: ["created"] })
  .trigger({ type: "schedule", cron: "0 * * * *" })
  .permissions("execution")
  .build()

export default new alien.Stack("event-pipeline")
  .add(inbox, "frozen")
  .add(data, "frozen")
  .add(events, "frozen")
  .add(processor, "live")
  .permissions({
    profiles: {
      execution: {
        "*": [
          "queue/data-read",
          "queue/data-write",
          "storage/data-read",
          "storage/data-write",
          "kv/data-read",
          "kv/data-write",
        ],
      },
    },
  })
  .build()
