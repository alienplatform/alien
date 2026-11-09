// The alien.ts equivalent of alien.toml. Both resolve to the same stack.
import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  jobs_api_token: alien.secret({
    providedBy: "developer",
    required: true,
    label: "Jobs API token",
    description: "Token the worker uses to pull jobs from your control plane.",
    env: { name: "JOBS_API_TOKEN", targetResources: ["job-worker"] },
  }),
})

const compute = new alien.ComputeCluster("compute")
  .pool("workers", {
    requirements: { cpu: 4, memory: "8Gi" },
    scale: { type: "fixed", machines: { min: 1, max: 20, default: 3 } },
  })
  .build()

const worker = new alien.Daemon("job-worker")
  .code({ type: "source", src: ".", toolchain: { type: "rust", binaryName: "job-worker" } })
  .cluster("compute")
  .cpu(3)
  .memory("6Gi")
  .stopGracePeriod(600)
  .healthCheck({ port: 8080, path: "/healthz" })
  .environment({ RUST_LOG: "info" })
  .build()

export default new alien.Stack("acme-jobs")
  .platforms(["machines"])
  .inputs(inputs)
  .add(compute, "frozen")
  .add(worker, "live")
  .build()
