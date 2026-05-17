import * as alien from "@alienplatform/core"

// Encrypted local storage for events
const events = new alien.Storage("events").build()

const agent = new alien.Daemon("agent")
  .code({
    type: "source",
    src: ".",
    toolchain: {
      type: "rust",
      binaryName: "endpoint-agent",
    },
  })
  .link(events)
  .commandsEnabled(true)
  .permissions("execution")
  .build()

export default new alien.Stack("endpoint-agent")
  .platforms(["local"])
  .add(events, "frozen")
  .add(agent, "live")
  .permissions({
    profiles: {
      execution: {},
    },
  })
  .build()
