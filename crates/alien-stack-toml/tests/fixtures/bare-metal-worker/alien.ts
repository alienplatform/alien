import * as alien from "@alienplatform/core"

const compute = new alien.ComputeCluster("compute")
  .pool("workers", {
    requirements: { cpu: 2, memory: "4Gi" },
    scale: { type: "fixed", machines: { min: 1, max: 20, default: 1 } },
  })
  .build()

const jobWorker = new alien.Daemon("job-worker")
  .code({ type: "source", src: ".", toolchain: { type: "rust", binaryName: "job-worker" } })
  .cluster("compute")
  .pool("workers")
  .runtime({ networkMode: "host" })
  .stopGracePeriod(3600)
  .healthCheck({ port: 8080, path: "/healthz" })
  .build()

export default new alien.Stack("acme-jobs")
  .platforms(["machines"])
  .add(compute, "frozen")
  .add(jobWorker, "live")
  .build()
