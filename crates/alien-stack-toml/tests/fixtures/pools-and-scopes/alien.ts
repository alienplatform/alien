import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  fleetSeed: alien.secret({
    providedBy: "developer",
    required: true,
    label: "Fleet seed",
    description: "Shared seed for fleet identities.",
    env: { name: "FLEET_SEED", targetResources: ["server", "worker"] },
  }),
  workerEnabled: alien.boolean({
    providedBy: "deployer",
    required: true,
    label: "Create worker",
    description: "Whether this deployment gets the 'worker' daemon.",
    default: true,
    platforms: ["aws", "gcp"],
  }),
  workerToken: alien.secret({
    providedBy: "developer",
    required: true,
    platforms: ["aws"],
    label: "WORKER_TOKEN",
    description: "Token the worker presents to the scheduler.",
    env: { name: "WORKER_TOKEN", targetResources: ["worker"] },
  }),
})

const cluster = new alien.ComputeCluster("edge-runtime")
  .pool("servers", {
    requirements: {
      cpu: "8",
      memory: "32Gi",
      architecture: "arm64",
      dataDisk: { size: "500G" },
      accelerators: [{ type: "nvidia-l4", count: 1 }],
    },
    scale: { type: "fixed", machines: 3 },
    failureDomainSpread: 3,
  })
  .pool("workers", {
    requirements: {
      cpu: 0.5,
      memory: "1.5Gi",
      ephemeralStorage: "100Gi",
      dataDisk: { size: { min: "0Gi", default: "200Gi", max: "4Ti" } },
    },
    scale: { type: "autoscale", min: 1, max: { min: 2, max: 20, default: 10 } },
  })
  .build()

const artifacts = new alien.Storage("artifacts").versioning(true).publicRead(true).build()

const server = new alien.Daemon("server")
  .code({ type: "image", image: "ghcr.io/acme/edge-server:1.4.0" })
  .cluster("edge-runtime")
  .pool("servers")
  .command(["edge-server", "--role", "server"])
  .cpu({ min: "500m", desired: "500m" })
  .stopGracePeriod(600)
  .healthCheck({ port: 9000, path: "/ready", method: "HEAD", timeoutSeconds: 2, failureThreshold: 5 })
  .link(artifacts)
  .permissions("server")
  .publicEndpoint("console", 9000)
  .build()

const worker = new alien.Daemon("worker")
  .code({ type: "source", src: "crates/worker", toolchain: { type: "rust", binaryName: "edge-worker" } })
  .cluster("edge-runtime")
  .memory("256Mi")
  .stopGracePeriod(5_400)
  .enabled(inputs.workerEnabled)
  .build()

export default new alien.Stack("edge-fleet")
  .inputs(inputs)
  .platforms(["machines", "aws", "gcp"])
  .add(cluster, "frozen")
  .add(artifacts, "live")
  .add(server, "live")
  .add(worker, "frozen")
  .permissions({ profiles: { server: { artifacts: ["storage/data-read"] } } })
  .build()
