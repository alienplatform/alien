import * as alien from "@alienplatform/core"

// Encrypted local storage for events
const events = new alien.Storage("events").build()

// TODO: Change to Worker resource once implemented
// Workers are designed for endpoint agents - they don't require HTTP endpoints
// For now using Function with minimal HTTP health check
const agent = new alien.Function("agent")
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
  .add(events, "frozen")
  .add(agent, "live")
  .permissions({
    profiles: {
      execution: {},
    },
  })
  .build()
