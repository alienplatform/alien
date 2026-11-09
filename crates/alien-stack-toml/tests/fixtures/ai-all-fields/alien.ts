import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  ai_enabled: alien.boolean({
    providedBy: "deployer",
    required: true,
    label: "Create ai",
    description: "Whether this deployment gets the ai.",
    default: false,
    platforms: ["aws"],
  }),
})

const main = new alien.AI("main")
  .displayName("Primary ai")
  .description("The ai the acme services share.")
  .enabled(inputs.ai_enabled)
  .build()

export default new alien.Stack("acme-ai")
  .inputs(inputs)
  .platforms(["aws", "gcp", "azure"])
  .add(main, "live", { remoteAccess: true })
  .build()
