import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  workers_enabled: alien.boolean({
    providedBy: "deployer",
    required: true,
    label: "Run workers",
    description: "Whether this deployment runs the worker and the container.",
    default: true,
  }),
})

const dashboard = new alien.Container("dashboard")
  .code({ type: "image", image: "ghcr.io/acme/dashboard:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .enabled(inputs.workers_enabled)
  .displayName("Customer dashboard")
  .description("Serves the customer dashboard.")
  .permissions("dashboard")
  .build()

const collector = new alien.Daemon("collector")
  .code({ type: "image", image: "ghcr.io/acme/collector:1.0.0" })
  .displayName("Metrics collector")
  .build()

const exporter = new alien.Worker("exporter")
  .code({ type: "image", image: "ghcr.io/acme/exporter:1.0.0" })
  .enabled(inputs.workers_enabled)
  .description("Exports reports on a schedule.")
  .permissions("exporter")
  .build()

const packager = new alien.Build("packager")
  .displayName("Packager")
  .description("Builds customer packages.")
  .permissions("packager")
  .build()

export default new alien.Stack("acme-entry-keys")
  .platforms(["aws", "gcp"])
  .inputs(inputs)
  .add(dashboard, "frozen", { remoteAccess: true })
  .add(collector, "live")
  .add(exporter, "live", { remoteAccess: true })
  .add(packager, "frozen", { remoteAccess: true })
  .permissions({ profiles: { dashboard: {}, exporter: {}, packager: {} } })
  .build()
