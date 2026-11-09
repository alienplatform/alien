import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  ledger_enabled: alien.boolean({
    providedBy: "deployer",
    required: true,
    label: "Create ledger database",
    description: "Whether this deployment gets the ledger database.",
    default: true,
  }),
})

const dbKey = new alien.Key("db-key").build()

const ledger = new alien.Postgres("ledger")
  .displayName("Ledger database")
  .description("Transactional store for the ledger service.")
  .enabled(inputs.ledger_enabled)
  .version("16")
  .cpu("2")
  .memory("8Gi")
  .storage("100Gi")
  .highAvailability()
  .encryptionKey(dbKey)
  .build()

// The builder has no method for the backend selector, so the generated config field is set
// directly.
;(ledger.config as { backend?: { type: string } }).backend = { type: "auroraServerlessV2" }

export default new alien.Stack("acme-postgres")
  .inputs(inputs)
  .platforms(["aws", "gcp", "azure"])
  .add(dbKey, "frozen")
  .add(ledger, "live", { remoteAccess: true })
  .build()
