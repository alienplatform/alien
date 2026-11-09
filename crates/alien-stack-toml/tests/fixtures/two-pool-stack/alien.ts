import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  log_sink: alien.string({
    providedBy: "developer",
    required: true,
    label: "Log sink",
    description: "Where shipped logs are sent.",
    format: "url",
    env: { name: "SHIP_TARGET", targetResources: ["log-shipper"] },
  }),
})

const telemetry = {
  OTEL_EXPORTER_OTLP_ENDPOINT: "https://otel.acme.example:4317",
  OTEL_SERVICE_NAMESPACE: "acme-pipeline",
  OTEL_RESOURCE_ATTRIBUTES: "team=platform,tier=batch",
}

const cluster = new alien.ComputeCluster("pipeline")
  .pool("frontend", {
    requirements: {
      cpu: 4,
      memory: "16Gi",
      ephemeralStorage: "50Gi",
      architecture: "x86_64",
    },
    scale: { type: "autoscale", min: 2, max: 8 },
  })
  .pool("batch", {
    requirements: {
      cpu: 16,
      memory: "64Gi",
      architecture: "arm64",
      dataDisk: { size: "1Ti" },
      accelerators: [{ type: "nvidia-l4", count: 2 }],
    },
    scale: { type: "fixed", machines: 1 },
    failureDomainSpread: 2,
  })
  .build()

const artifacts = new alien.Storage("artifacts").versioning(true).build()

const api = new alien.Container("api")
  .code({ type: "image", image: "ghcr.io/acme/pipeline-api:4.0.1" })
  .cluster("pipeline")
  .pool("frontend")
  .cpu(1)
  .memory("2Gi")
  .port(8080)
  .autoScale({ min: 2, desired: 2, max: 6, targetCpuPercent: 65 })
  .healthCheck({ path: "/healthz", port: 8080 })
  .publicEndpoint("http", 8080)
  .environment({ RUST_LOG: "info", ARTIFACT_PREFIX: "builds/" })
  .link(artifacts)
  .permissions("api")
  .build()

const logShipper = new alien.Daemon("log-shipper")
  .code({ type: "source", src: ".", toolchain: { type: "rust", binaryName: "log-shipper" } })
  .cluster("pipeline")
  .pool("frontend")
  .cpu(0.25)
  .memory("128Mi")
  .environment({ ...telemetry, SHIP_BATCH_SIZE: "200" })
  .link(artifacts)
  .permissions("log-shipper")
  .build()

const gpuMonitor = new alien.Daemon("gpu-monitor")
  .code({ type: "image", image: "ghcr.io/acme/gpu-monitor:0.9.4" })
  .cluster("pipeline")
  .pool("batch")
  .runtime({
    privileged: true,
    pidNamespace: "host",
    networkMode: "host",
    mounts: [{ source: "/", target: "/host" }],
    user: "0",
  })
  .stopGracePeriod(30)
  .environment({ ...telemetry, POLL_INTERVAL_SECONDS: "15" })
  .build()

export default new alien.Stack("acme-pipeline")
  .inputs(inputs)
  .platforms(["machines", "aws"])
  .add(cluster, "frozen")
  .add(artifacts, "frozen")
  .add(api, "live")
  .add(logShipper, "live")
  .add(gpuMonitor, "live")
  .permissions({
    profiles: {
      api: { artifacts: ["storage/data-read", "storage/data-write"] },
      "log-shipper": { artifacts: ["storage/data-write"] },
    },
  })
  .build()
