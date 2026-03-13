/**
 * GitHub Agent - Remote Agent Stack
 *
 * Provides a vault for GitHub integrations and an ARC-enabled function
 * that can be invoked by the control plane or directly over HTTP.
 */
import * as alien from "@alienplatform/core"

const integrations = new alien.Vault("integrations").build()

const agent = new alien.Function("agent")
  .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
  .link(integrations)
  .arcEnabled(true)
  .ingress("public")
  .permissions("execution")
  .build()

export default new alien.Stack("github-agent")
  .add(integrations, "frozen")
  .add(agent, "live")
  .permissions({
    profiles: {
      execution: {},
    },
  })
  .build()
