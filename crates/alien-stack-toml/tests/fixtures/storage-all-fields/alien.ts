import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  reports_enabled: alien.boolean({
    providedBy: "deployer",
    required: true,
    label: "Create reports bucket",
    description: "Whether this deployment gets the reports bucket.",
    default: true,
  }),
})

const bucketKey = new alien.Key("bucket-key").displayName("Bucket key").build()
const uploads = new alien.Queue("uploads").build()
const exports = new alien.Queue("exports").build()

const reports = new alien.Storage("reports")
  .displayName("Reports")
  .description("Customer-facing report archive.")
  .enabled(inputs.reports_enabled)
  .versioning(true)
  .publicRead(true)
  .encryptionKey(bucketKey)
  .corsAllowedOrigins(["https://app.acme.example", "https://admin.acme.example"])
  .lifecycleRules([{ prefix: "tmp/", days: 7 }, { days: 365 }])
  .notify(uploads, { prefix: "incoming/", suffix: ".csv" })
  .notify(exports, { events: ["created"], prefix: "outgoing/" })
  .build()

export default new alien.Stack("acme-storage")
  .inputs(inputs)
  .platforms(["aws", "gcp"])
  .add(bucketKey, "frozen")
  .add(uploads, "live")
  .add(exports, "live")
  .add(reports, "live", { remoteAccess: true })
  .build()
