import * as alien from "@alienplatform/core"

const agent = new alien.Function("agent")
  .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
  .commandsEnabled(true)
  .ingress("public")
  .permissions("execution")
  .build()

export default new alien.Stack("basic-function")
  .add(agent, "live")
  .permissions({
    profiles: {
      execution: {},
    },
  })
  .build()
