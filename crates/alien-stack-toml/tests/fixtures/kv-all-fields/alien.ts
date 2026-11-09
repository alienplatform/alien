import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  kv_enabled: alien.boolean({
    providedBy: "deployer",
    required: true,
    label: "Create kv",
    description: "Whether this deployment gets the kv.",
    default: false,
    platforms: ["aws"],
  }),
})

const main = new alien.Kv("main")
  .displayName("Primary kv")
  .description("The kv the acme services share.")
  .enabled(inputs.kv_enabled)
  .build()

export default new alien.Stack("acme-kv")
  .inputs(inputs)
  .platforms(["aws", "gcp", "azure"])
  .add(main, "live", { remoteAccess: true })
  .build()
