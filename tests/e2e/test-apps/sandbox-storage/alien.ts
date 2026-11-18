import * as alien from "@alienplatform/core"

// A sandbox built from source whose /workspace is persistent storage, published to the
// deployment's remote caller, which drives it itself: create with a storage key, write, sync,
// terminate, restore into a new sandbox, read without a sandbox and erase the key. Frozen because
// Azure creates its sandbox group and the storage store at setup.
const agents = new alien.Sandbox("agents")
  .code({
    type: "source",
    src: "./sandbox",
    toolchain: { type: "docker", dockerfile: "Dockerfile" },
  })
  .egress("none")
  .persistentStorage({ path: "/workspace", exclude: ["tmp/"], maxSize: "2Gi" })
  .lifecycle({})
  .build()

// A sandbox published alone installs into the customer's existing resource group. A direct push has
// no such group, so a storage account stands in as the ordinary resource that makes setup create one.
const uploads = new alien.Storage("uploads").build()

export default new alien.Stack("sandbox-storage")
  .add(uploads, "frozen")
  .add(agents, "frozen", { remoteAccess: true })
  .build()
