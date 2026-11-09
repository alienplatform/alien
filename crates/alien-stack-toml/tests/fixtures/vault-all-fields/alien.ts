import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  vault_enabled: alien.boolean({
    providedBy: "deployer",
    required: true,
    label: "Create vault",
    description: "Whether this deployment gets the vault.",
    default: false,
    platforms: ["aws"],
  }),
})

const main = new alien.Vault("main")
  .displayName("Primary vault")
  .description("The vault the acme services share.")
  .enabled(inputs.vault_enabled)
  .build()

export default new alien.Stack("acme-vault")
  .inputs(inputs)
  .platforms(["aws", "gcp", "azure"])
  .add(main, "live", { remoteAccess: true })
  .build()
