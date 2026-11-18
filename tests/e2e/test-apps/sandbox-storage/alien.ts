import * as alien from "@alienplatform/core"

// A sandbox built from source whose /workspace is persistent storage. It is not published
// remotely (a persistent storage sandbox never is): the test drives the binding the deployment's
// controller publishes, as the deployment's own code would: create with a storage key, write,
// sync, terminate, restore into a new sandbox, read without a sandbox and erase the key. Frozen
// because Azure creates its sandbox group and the store at setup.
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

// The execution identity creates the sandboxes, so setup gives it the store grants. The test
// calls with its own principal holding exactly the roles setup gave this identity.
export default new alien.Stack("sandbox-storage")
  .permissions({ profiles: { execution: { agents: ["sandbox/execute"] } } })
  .add(uploads, "frozen")
  .add(agents, "frozen")
  .build()
