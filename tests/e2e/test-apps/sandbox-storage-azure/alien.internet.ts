import * as alien from "@alienplatform/core"

// The sandbox of alien.ts with internet egress: commands reach the internet through the
// deployment's gateway, the agent reaches its store directly, and nothing else leaves the guest.
const agents = new alien.Sandbox("agents")
  .code({
    type: "source",
    src: "./sandbox",
    toolchain: { type: "docker", dockerfile: "Dockerfile" },
  })
  .egress("internet")
  .persistentStorage({ path: "/workspace", exclude: ["tmp/"], maxSize: "2Gi" })
  .lifecycle({})
  .build()

const uploads = new alien.Storage("uploads").build()

export default new alien.Stack("sandbox-storage")
  .permissions({ profiles: { execution: { agents: ["sandbox/execute"] } } })
  .add(uploads, "frozen")
  .add(agents, "frozen")
  .build()
