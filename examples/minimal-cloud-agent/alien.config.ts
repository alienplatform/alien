/**
 * Minimal Cloud Agent - alien.config.ts
 *
 * The smallest possible Alien agent configuration.
 * One Function, one ARC command, demo mode built-in.
 */
import * as alien from "@alienplatform/core"

const agent = new alien.Function("agent")
  .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
  .arcEnabled(true)
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
