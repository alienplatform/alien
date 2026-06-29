import * as alien from "@alienplatform/core"

const agent = new alien.Worker("agent")
  .code({
    type: "source",
    src: "./",
    toolchain: { type: "rust", binaryName: "basic-worker" },
  })
  .commandsEnabled(true)
  .publicEndpoint("api")
  .permissions("execution")
  .build()

export default new alien.Stack("basic-worker")
  .add(agent, "live")
  .permissions({
    profiles: {
      execution: {},
    },
  })
  .build()
