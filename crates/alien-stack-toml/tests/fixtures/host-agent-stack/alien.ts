import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  enrollment_token: alien.secret({
    providedBy: "developer",
    required: true,
    label: "Enrollment token",
    description: "Token the agent uses to enroll with the fleet service.",
    minLength: 24,
    env: { name: "ENROLLMENT_TOKEN", targetResources: ["agent"] },
  }),
  signing_key: alien.secret({
    providedBy: "developer",
    required: true,
    label: "Signing key",
    description: "Key that signs reports leaving the host.",
    generate: { length: 64 },
    env: "REPORT_SIGNING_KEY",
  }),
  site_name: alien.string({
    providedBy: "developer",
    required: true,
    label: "Site name",
    description: "Short name of the site this agent reports from.",
    pattern: "^[a-z][a-z0-9-]*$",
    minLength: 2,
    maxLength: 32,
    env: { name: "SITE_NAME", targetResources: ["agent"] },
  }),
  api_endpoint: alien.string({
    providedBy: "developer",
    required: true,
    label: "Fleet service URL",
    description: "Where the agent enrolls and reports.",
    format: "url",
    default: "https://fleet.acme.example",
    env: "FLEET_URL",
  }),
  update_channel: alien.enum(["stable", "beta", "nightly"], {
    providedBy: "developer",
    required: true,
    default: "stable",
    label: "Update channel",
    description: "Release channel the agent follows.",
    env: { name: "UPDATE_CHANNEL", targetResources: ["agent"] },
  }),
  scan_interval_seconds: alien.integer({
    providedBy: ["developer", "deployer"],
    required: true,
    default: 300,
    min: 30,
    max: 86400,
    label: "Scan interval",
    description: "Seconds between full host scans.",
    env: "SCAN_INTERVAL_SECONDS",
  }),
  cpu_budget: alien.number({
    providedBy: "developer",
    required: true,
    default: 0.1,
    min: 0.01,
    max: 1,
    label: "CPU budget",
    description: "Share of one core the agent may use.",
    env: { name: "CPU_BUDGET", targetResources: ["agent"] },
  }),
  collect_process_events: alien.boolean({
    providedBy: "developer",
    required: true,
    default: true,
    label: "Collect process events",
    description: "Record process starts and exits.",
    env: "COLLECT_PROCESS_EVENTS",
  }),
  excluded_paths: alien.stringList({
    providedBy: "developer",
    required: false,
    default: ["/proc", "/sys", "/dev"],
    maxItems: 50,
    label: "Excluded paths",
    description: "",
    env: "EXCLUDED_PATHS",
  }),
  use_proxy: alien.boolean({
    providedBy: ["developer", "deployer"],
    required: true,
    default: false,
    label: "Use an outbound proxy",
    description: "Send agent traffic through a proxy.",
  }),
  proxy_url: alien.string({
    providedBy: ["developer", "deployer"],
    required: false,
    when: { input: "use_proxy", equals: true },
    format: "url",
    label: "Proxy URL",
    description: "",
    env: "HTTPS_PROXY",
  }),
  proxy_password: alien.secret({
    providedBy: "deployer",
    required: false,
    when: { input: "use_proxy", equals: true },
    platforms: ["machines"],
    label: "Proxy password",
    description: "",
    env: "PROXY_PASSWORD",
  }),
  ship_audit_logs: alien.boolean({
    providedBy: "deployer",
    required: true,
    default: false,
    platforms: ["aws"],
    label: "Ship audit logs",
    description: "Create a bucket for audit logs and link it to the agent.",
  }),
})

const auditArchive = new alien.Storage("audit-archive")
  .versioning(true)
  .enabled(inputs.ship_audit_logs)
  .build()

const agent = new alien.Daemon("agent")
  .code({ type: "source", src: ".", toolchain: { type: "rust", binaryName: "acme-host-agent" } })
  .command(["/usr/local/bin/acme-host-agent", "--foreground"])
  .cpu(0.5)
  .memory("256Mi")
  .runtime({
    privileged: true,
    pidNamespace: "host",
    networkMode: "host",
    mounts: [{ source: "/", target: "/host" }],
    user: "0",
  })
  .stopGracePeriod(300)
  .healthCheck({ port: 9477, path: "/healthz", timeoutSeconds: 2, failureThreshold: 4 })
  .environment({
    AGENT_HOST_ROOT: "/host",
    AGENT_STATE_DIR: "/host/var/lib/acme-host-agent",
    AGENT_SOCKET: "/host/run/acme-host-agent.sock",
    AGENT_BANNER: "scan # not a comment",
    AGENT_RULES_DIR: "/host/etc/acme/site-rules.d",
    AGENT_QUARANTINE_DIR: "/host/var/quarantine",
    LOG_FORMAT: "json",
    SCAN_THREADS: "4",
    EBPF_ENABLED: "true",
  })
  .link(auditArchive)
  .publicEndpoint("status", 9477, { protocol: "http", hostLabel: "status" })
  .build()

export default new alien.Stack("acme-host-agent")
  .inputs(inputs)
  .platforms(["machines", "aws"])
  .add(auditArchive, "frozen")
  .add(agent, "live")
  .permissions({ profiles: { agent: {} } })
  .build()
