import * as alien from "@alienplatform/core"

const agent = new alien.Daemon("agent")
  .code({ type: "source", src: ".", toolchain: { type: "rust", binaryName: "acme-agent" } })
  .healthCheck({ port: 8080, path: "/health" })
  .build()

export default new alien.Stack("acme-agent").platforms(["machines"]).add(agent, "live").build()
