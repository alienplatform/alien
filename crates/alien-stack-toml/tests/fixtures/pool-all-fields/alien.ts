import * as alien from "@alienplatform/core"

const cluster = new alien.ComputeCluster("acme-pools")
  .pool("trainers", {
    requirements: {
      cpu: 16,
      memory: "64Gi",
      accelerators: [{ type: "nvidia-a100", count: 4 }],
      dataDisk: { size: "2Ti" },
      ephemeralStorage: "200Gi",
      architecture: "x86_64",
      nestedVirtualization: true,
    },
    scale: { type: "fixed", machines: 4 },
    failureDomainSpread: 3,
    dynamicContainers: true,
  })
  .pool("batch", {
    requirements: {
      cpu: "4",
      memory: "16Gi",
      accelerators: [{ type: "nvidia-l4", count: 1 }],
      dataDisk: { size: { min: "0Gi", default: "100Gi", max: "1Ti" } },
      ephemeralStorage: "50Gi",
      architecture: "arm64",
      nestedVirtualization: false,
    },
    scale: {
      type: "autoscale",
      min: { min: 0, max: 4, default: 1 },
      max: { min: 4, max: 32, default: 8 },
    },
    failureDomainSpread: 2,
  })
  .pool("edge", {
    requirements: { cpu: 1, memory: "2Gi" },
    scale: { type: "fixed", machines: { min: 1, max: 5, default: 2 } },
  })
  .build()

export default new alien.Stack("acme-pool-fields")
  .platforms(["aws", "gcp"])
  .add(cluster, "frozen")
  .build()
