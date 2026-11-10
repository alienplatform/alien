import * as alien from "@alienplatform/core"

const models = new alien.Storage("models").build()
const scratch = new alien.Storage("scratch").build()

const cluster = new alien.ComputeCluster("acme-nodes")
  .displayName("Acme nodes")
  .description("Machines that run acme workloads.")
  .containerCidr("10.96.0.0/16")
  .nodePermissions(
    { models: ["storage/data-read"], scratch: ["storage/data-read", "storage/data-write"] },
    { platforms: ["aws", "gcp"] },
  )
  .pool("general", {
    requirements: { cpu: 2, memory: "8Gi" },
    scale: { type: "fixed", machines: 2 },
  })
  .build()

export default new alien.Stack("acme-cluster-options")
  .platforms(["aws", "gcp", "azure"])
  .add(models, "frozen")
  .add(scratch, "frozen")
  .add(cluster, "live")
  .build()
