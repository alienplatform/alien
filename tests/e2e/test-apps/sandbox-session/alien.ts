import * as alien from "@alienplatform/core"

// A Live sandbox built from source, and a Worker that opens a session in it and runs a command.
// On AWS the sandbox's controller renders the image-build bundle into a bucket the deployment
// owns, from the agent-layered image `alien release` pushed to the manager's registry.
const agents = new alien.Sandbox("agents")
  .code({
    type: "source",
    src: "./sandbox",
    toolchain: { type: "docker", dockerfile: "Dockerfile" },
  })
  .egress({ mode: "allow" })
  .lifecycle({ maxLifetimeSeconds: 900 })
  .build()

const uploads = new alien.Storage("uploads").build()

const api = new alien.Worker("api")
  .code({
    type: "source",
    src: "./",
    toolchain: { type: "typescript", entrypoint: "dist/index.js" },
  })
  .memoryMb(1024)
  .timeoutSeconds(300)
  .publicEndpoint("api")
  .readinessProbe({ method: "GET", path: "/health" })
  .link(agents)
  .link(uploads)
  .permissions("execution")
  .build()

export default new alien.Stack("sandbox-session")
  .add(uploads, "frozen")
  .add(agents, "live")
  .add(api, "live")
  .permissions({
    profiles: {
      execution: {
        "*": ["sandbox/execute", "sandbox/management", "storage/data-read", "storage/data-write"],
      },
    },
  })
  .build()
