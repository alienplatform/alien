import * as alien from "@alienplatform/core"

// A sandbox with persistent storage and the Worker that holds its binding. On GCP the binding
// pulls the sandbox's commits into the store the setup package created, so the Worker is the
// replicator: every check runs inside one Worker request, which keeps the binding's pull loop
// running, and ends with a sync before the request returns.
const agents = new alien.Sandbox("agents")
  .code({
    type: "source",
    src: "./sandbox",
    toolchain: { type: "docker", dockerfile: "Dockerfile" },
  })
  .egress({ mode: "allow" })
  .lifecycle({ maxLifetimeSeconds: 1800 })
  .persistentStorage({ path: "/workspace", maxSize: "2Gi", exclude: ["tmp/"] })
  .build()

const api = new alien.Worker("api")
  .code({
    type: "source",
    src: "./",
    toolchain: { type: "typescript", entrypoint: "dist/index.js" },
  })
  .memoryMb(1024)
  .timeoutSeconds(900)
  .publicEndpoint("api")
  .readinessProbe({ method: "GET", path: "/health" })
  .link(agents)
  .permissions("execution")
  .build()

// Frozen: on GCP persistent storage needs the bucket the setup package creates.
export default new alien.Stack("sandbox-storage")
  .add(agents, "frozen")
  .add(api, "live")
  .permissions({
    profiles: {
      execution: {
        agents: ["sandbox/execute", "sandbox/management"],
      },
    },
  })
  .build()
