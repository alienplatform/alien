// The alien.ts equivalent of alien.toml. Both resolve to the same stack.
import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  signing_key: alien.secret({
    providedBy: "developer",
    required: true,
    generate: { length: 48 },
    label: "Signing key",
    description: "Key the API signs session tokens with. Alien generates it once per deployment.",
    env: { name: "SIGNING_KEY", targetResources: ["api"] },
  }),
  sso_enabled: alien.boolean({
    providedBy: "deployer",
    required: true,
    default: false,
    label: "Single sign-on",
    description: "Sign users in with your identity provider.",
  }),
  sso_issuer: alien.string({
    providedBy: "deployer",
    required: true,
    when: { input: "sso_enabled", equals: true },
    format: "url",
    label: "SSO issuer URL",
    description: "OIDC issuer of your identity provider.",
    placeholder: "https://login.example.com",
    env: { name: "SSO_ISSUER", targetResources: ["api"] },
  }),
  audit_archive: alien.boolean({
    providedBy: "deployer",
    required: true,
    platforms: ["aws", "gcp"],
    default: false,
    label: "Audit archive bucket",
    description: "Keep a separate, versioned bucket for audit exports.",
  }),
})

const dataKey = new alien.Key("data-key").build()
const newEvents = new alien.Queue("new-events").build()

const events = new alien.Storage("events")
  .versioning(true)
  .encryptionKey(dataKey)
  .lifecycleRules([{ days: 30, prefix: "tmp/" }])
  .notify(newEvents, { suffix: ".json" })
  .build()

const auditArchive = new alien.Storage("audit-archive")
  .versioning(true)
  .enabled(inputs.audit_archive)
  .build()

const db = new alien.Postgres("db")
  .description("Primary analytics database.")
  .memory("8Gi")
  .storage("100Gi")
  .highAvailability(true)
  .encryptionKey(dataKey)
  .build()

const connectors = new alien.Vault("connectors").build()

const api = new alien.Container("api")
  .code({ type: "image", image: "ghcr.io/acme/analytics-api:2.3.0" })
  .cpu({ min: 0.5, max: 4, default: 1 })
  .memory({ min: "1Gi", max: "16Gi", default: "2Gi" })
  .replicas(2)
  .publicEndpoint("api", 8080, { protocol: "http", hostLabel: "api" })
  .healthCheck({ path: "/ready", timeoutSeconds: 2 } as alien.HealthCheck)
  .link(events)
  .link(db)
  .link(connectors)
  .permissions("api")
  .environment({ LOG_FORMAT: "json" })
  .build()

const ingest = new alien.Worker("ingest")
  .code({ type: "source", src: ".", toolchain: { type: "rust", binaryName: "ingest-worker" } })
  .memoryMb(1024)
  .timeoutSeconds(300)
  .trigger({
    type: "queue",
    queue: newEvents.ref(),
    batch: { maxMessages: 10, maxWindowSeconds: 30 },
  })
  .trigger({ type: "schedule", cron: "0 3 * * *" })
  .link(events)
  .link(newEvents)
  .link(db)
  .permissions("ingest")
  .build()

export default new alien.Stack("acme-analytics")
  .platforms(["aws", "gcp", "azure"])
  .inputs(inputs)
  .add(dataKey, "frozen")
  .add(events, "frozen", { remoteAccess: true })
  .add(auditArchive, "frozen")
  .add(newEvents, "frozen")
  .add(db, "frozen")
  .add(connectors, "frozen")
  .add(api, "live")
  .add(ingest, "live")
  .permissions({
    profiles: {
      api: {
        events: ["storage/data-read"],
        db: ["postgres/data-access"],
        connectors: ["vault/data-read", "vault/data-write"],
      },
      ingest: {
        events: ["storage/data-read", "storage/data-write"],
        "new-events": ["queue/data-read"],
        db: ["postgres/data-access"],
      },
    },
  })
  .operations({ cloudwatch: { approval: { "*": "auto" } } })
  .build()
