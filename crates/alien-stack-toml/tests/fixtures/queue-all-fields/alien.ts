import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  queue_enabled: alien.boolean({
    providedBy: "deployer",
    required: true,
    label: "Create queue",
    description: "Whether this deployment gets the queue.",
    default: false,
    platforms: ["aws"],
  }),
})

const main = new alien.Queue("main")
  .displayName("Primary queue")
  .description("The queue the acme services share.")
  .enabled(inputs.queue_enabled)
  .build()

export default new alien.Stack("acme-queue")
  .inputs(inputs)
  .platforms(["aws", "gcp", "azure"])
  .add(main, "live", { remoteAccess: true })
  .build()
