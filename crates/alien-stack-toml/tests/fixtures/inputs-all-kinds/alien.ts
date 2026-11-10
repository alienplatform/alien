import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  service_url: alien.string({
    providedBy: "developer",
    required: true,
    label: "Service URL",
    description: "Base URL of the service the collector reports to.",
    placeholder: "https://reports.example.com",
    format: "url",
    minLength: 12,
    maxLength: 200,
    pattern: "^https://[a-z0-9.-]+(/.*)?$",
    env: "SERVICE_URL",
  }),
  api_key: alien.secret({
    providedBy: ["developer", "deployer"],
    required: true,
    label: "API key",
    description: "Key the collector presents to the reporting service.",
    minLength: 20,
    maxLength: 64,
    pattern: "^[A-Za-z0-9_-]+$",
    env: [
      { name: "API_KEY", targetResources: ["collector"], type: "secret" },
      { name: "API_KEY_FINGERPRINT_SOURCE", type: "plain" },
    ],
  }),
  session_key: alien.secret({
    providedBy: "developer",
    required: true,
    label: "Session key",
    description: "Key that signs collector sessions.",
    generate: { length: 48 },
    env: { name: "SESSION_KEY", targetResources: ["collector*"] },
  }),
  upstream_credential: alien.secret({
    providedBy: "deployer",
    required: false,
    label: "Upstream credential",
    description: "",
    platforms: ["aws", "gcp"],
    env: { name: "UPSTREAM_CREDENTIAL", targetResources: ["collector"] },
  }),
  log_level: alien.enum(["error", "warn", "info", "debug"], {
    providedBy: "developer",
    required: true,
    default: "info",
    label: "Log level",
    description: "How much the collector logs.",
    env: "LOG_LEVEL",
  }),
  batch_size: alien.integer({
    providedBy: ["deployer", "developer"],
    required: true,
    default: 500,
    min: 1,
    max: 10000,
    label: "Batch size",
    description: "Records sent per request.",
    env: "BATCH_SIZE",
  }),
  sample_ratio: alien.number({
    providedBy: "developer",
    required: true,
    default: 0.5,
    min: 0,
    max: 1.0,
    label: "Sample ratio",
    description: "Share of records the collector keeps.",
    env: { name: "SAMPLE_RATIO", targetResources: ["collector", "collector-canary"] },
  }),
  telemetry_enabled: alien.boolean({
    providedBy: "developer",
    required: true,
    default: true,
    label: "Telemetry",
    description: "Send usage telemetry to the developer.",
    env: "TELEMETRY_ENABLED",
  }),
  telemetry_endpoint: alien.string({
    providedBy: "developer",
    required: false,
    when: { input: "telemetry_enabled", equals: true },
    label: "Telemetry endpoint",
    description: "",
    placeholder: "https://telemetry.example.com",
    env: "TELEMETRY_ENDPOINT",
  }),
  local_trace_dir: alien.string({
    providedBy: "developer",
    required: false,
    when: { input: "telemetry_enabled", equals: false },
    default: "/var/lib/collector/traces",
    label: "Local trace directory",
    description: "",
    env: "LOCAL_TRACE_DIR",
  }),
  allowed_origins: alien.stringList({
    providedBy: "developer",
    required: true,
    default: ["https://app.example.com"],
    minItems: 1,
    maxItems: 10,
    label: "Allowed origins",
    description: "Origins the collector accepts reports from.",
    env: "ALLOWED_ORIGINS",
  }),
  tenant_id: alien.string({
    providedBy: "deployer",
    required: false,
    platforms: ["aws", "gcp"],
    label: "Tenant ID",
    description: "",
    env: "TENANT_ID",
  }),
  deployment_label: alien.string({
    providedBy: "developer",
    required: true,
    default: "default",
    label: "Deployment label",
    description: "Free-form label that shows up in reports.",
    env: { name: "DEPLOYMENT_LABEL", targetResources: ["collector", "collector-canary"] },
  }),
  operator_note: alien.string({
    providedBy: "developer",
    required: false,
    label: "Operator note",
    description: "",
    env: { name: "OPERATOR_NOTE", targetResources: ["collector"], type: "secret" },
  }),
})

const collector = new alien.Daemon("collector")
  .code({ type: "image", image: "ghcr.io/acme/collector:3.2.0" })
  .environment({ NODE_ROLE: "primary" })
  .build()

const collectorCanary = new alien.Daemon("collector-canary")
  .code({ type: "image", image: "ghcr.io/acme/collector:3.3.0-rc1" })
  .environment({ NODE_ROLE: "canary" })
  .build()

export default new alien.Stack("acme-collector")
  .inputs(inputs)
  .platforms(["machines", "aws", "gcp"])
  .add(collector, "live")
  .add(collectorCanary, "live")
  .build()
