import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  api_token: alien.secret({
    providedBy: "developer",
    required: true,
    label: "API token",
    description: "Token the worker and the packager present to the control plane.",
    env: { name: "API_TOKEN", targetResources: ["configured", "packager"] },
  }),
  region: alien.string({
    providedBy: "deployer",
    required: true,
    label: "Region",
    description: "Region the worker reports from.",
    env: { name: "REGION", targetResources: ["configured", "packager"], type: "plain" },
  }),
})

const ported = new alien.Container("ported")
  .code({ type: "image", image: "ghcr.io/acme/ported:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .ports([8080, 9090])
  .permissions("ported")
  .build()

const described = new alien.Daemon("described")
  .code({ type: "image", image: "ghcr.io/acme/described:1.0.0" })
  .description("Resident agent that relays HTTP.")
  .publicEndpoint("relay", 7000, "http")
  .build()

const configured = new alien.Worker("configured")
  .code({ type: "image", image: "ghcr.io/acme/configured:1.0.0" })
  .displayName("Configured worker")
  .environment({
    APP_MODE: "standard",
    APP_GREETING: 'hello, "world"',
    APP_PATTERN: "^[a-z]+$ # kept",
    LOG_LEVEL: "warn",
    APP_REGION: "eu-west-1",
    MODE: "worker",
  })
  .permissions("configured")
  .build()

const packager = new alien.Build("packager")
  .environment({
    APP_MODE: "standard",
    APP_GREETING: 'hello, "world"',
    APP_PATTERN: "^[a-z]+$ # kept",
    LOG_LEVEL: "warn",
    APP_REGION: "eu-west-1",
    CHANNEL: "stable",
  })
  .permissions("packager")
  .build()

export default new alien.Stack("acme-workload-keys")
  .platforms(["machines", "aws"])
  .inputs(inputs)
  .add(ported, "live")
  .add(described, "live", { remoteAccess: true })
  .add(configured, "frozen")
  .add(packager, "live")
  .permissions({ profiles: { ported: {}, configured: {}, packager: {} } })
  .build()
