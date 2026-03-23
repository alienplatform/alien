/**
 * Minimal Cloud Agent - alien.ts
 *
 * The smallest possible Alien agent configuration.
 * One Function, one command handler, demo mode built-in.
 */
import * as alien from "@alienplatform/core"

const agent = new alien.Function("agent")
  .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
  .commandsEnabled(true)
  .ingress("public")
  .permissions("execution")
  .build()

export default new alien.Stack("minimal-cloud-agent")
  .add(agent, "live")
  .permissions({
    profiles: {
      execution: {},
    },
  })
  .build()
