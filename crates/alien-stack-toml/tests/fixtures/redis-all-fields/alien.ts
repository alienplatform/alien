import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  sessions_enabled: alien.boolean({
    providedBy: "deployer",
    required: true,
    label: "Create session cache",
    description: "Whether this deployment gets the session cache.",
    default: true,
  }),
})

const cacheKey = new alien.Key("cache-key").build()

const sessions = new alien.Redis("sessions")
  .displayName("Session cache")
  .description("Session store for the web tier.")
  .enabled(inputs.sessions_enabled)
  .memory("4Gi")
  .evictionPolicy("allkeys-lru")
  .persistence()
  .highAvailability()
  .encryptionKey(cacheKey)
  .build()

export default new alien.Stack("acme-redis")
  .inputs(inputs)
  .platforms(["aws", "gcp", "azure"])
  .add(cacheKey, "frozen")
  .add(sessions, "live", { remoteAccess: true })
  .build()
