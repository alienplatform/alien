import * as alien from "@alienplatform/core"

// A sandbox with persistent storage and the Worker that holds its binding. On GCP the binding
// pulls the sandbox's commits into the store the setup package created, so the Worker is the
// replicator: every check runs inside one Worker request, which keeps the binding's pull loop
// running, and ends with a sync before the request returns.
// Every check runs on `agents`, declared egress none. `open` is the same sandbox declared with
// direct internet access, for the round trip and isolation checks under that policy. A per-create
// egress policy needs the gateway (`internet`), which this test does not set up.
const storage = { path: "/workspace", maxSize: "2Gi", exclude: ["tmp/"] }
const agents = new alien.Sandbox("agents")
  .code({
    type: "source",
    src: "./sandbox",
    toolchain: { type: "docker", dockerfile: "Dockerfile" },
  })
  .egress({ mode: "none" })
  .lifecycle({ maxLifetimeSeconds: 1800 })
  .persistentStorage(storage)
  .build()

const open = new alien.Sandbox("open")
  .code({
    type: "source",
    src: "./sandbox",
    toolchain: { type: "docker", dockerfile: "Dockerfile" },
  })
  .egress({ mode: "allow" })
  .lifecycle({ maxLifetimeSeconds: 1800 })
  .persistentStorage(storage)
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
  .link(open)
  .permissions("execution")
  .build()

// Frozen: on GCP persistent storage needs the bucket the setup package creates.
export default new alien.Stack("sandbox-storage")
  .add(agents, "frozen")
  .add(open, "frozen")
  .add(api, "live")
  .permissions({
    profiles: {
      execution: {
        agents: ["sandbox/execute", "sandbox/management"],
        open: ["sandbox/execute", "sandbox/management"],
      },
    },
  })
  .build()
