import * as alien from "@alienplatform/core"

// A sandbox built from source and published to the deployment's remote caller, which drives it
// itself: the test opens a session and runs commands, including an interactive one, with no
// Worker in between. Frozen because Azure creates its sandbox group at setup.
const agents = new alien.Sandbox("agents")
  .code({
    type: "source",
    src: "./sandbox",
    toolchain: { type: "docker", dockerfile: "Dockerfile" },
  })
  .egress({ mode: "allow" })
  .lifecycle({})
  .build()

// A sandbox published alone installs into the customer's existing resource group. A direct push has
// no such group, so a storage account stands in as the ordinary resource that makes setup create one.
const uploads = new alien.Storage("uploads").build()

export default new alien.Stack("sandbox-remote")
  .add(uploads, "frozen")
  .add(agents, "frozen", { remoteAccess: true })
  .build()
