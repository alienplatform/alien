import * as alien from "@alienplatform/core"

// A Live sandbox whose /workspace belongs to a caller-chosen storage key, and a Worker that drives
// sessions on it. Persistent storage refuses remote access, so the test reaches the sandbox only
// through this Worker.
const agents = new alien.Sandbox("agents")
  .code({
    type: "source",
    src: "./sandbox",
    toolchain: { type: "docker", dockerfile: "Dockerfile" },
  })
  .egress("none")
  .lifecycle({ maxLifetimeSeconds: 1800 })
  .persistentStorage({ path: "/workspace", exclude: ["scratch/"], maxSize: "2Gi" })
  .build()

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
  .permissions("execution")
  .build()

export default new alien.Stack("sandbox-storage")
  .add(agents, "live")
  .add(api, "live")
  .permissions({
    profiles: {
      execution: {
        "*": ["sandbox/execute", "sandbox/management"],
      },
    },
  })
  .build()
