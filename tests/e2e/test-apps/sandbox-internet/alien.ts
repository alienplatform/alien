import * as alien from "@alienplatform/core"

// A Google Cloud sandbox with egress `internet`: every connection leaves through the deployment's
// egress gateway, under the rules each sandbox is created with. Published to the remote caller so
// the test drives sessions itself.
const agents = new alien.Sandbox("agents")
  .code({
    type: "source",
    src: "./sandbox",
    toolchain: { type: "docker", dockerfile: "Dockerfile" },
  })
  .egress("internet")
  .lifecycle({})
  .build()

export default new alien.Stack("sandbox-internet").add(agents, "frozen", { remoteAccess: true }).build()
