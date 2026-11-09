import * as alien from "@alienplatform/core"

const images = new alien.ArtifactRegistry("images")
  .displayName("Image registry")
  .description("Container images built for the acme services.")
  .replicationRegions(["us-west-2", "eu-west-1"])
  .build()

export default new alien.Stack("acme-registry")
  .platforms(["aws"])
  .add(images, "live", { remoteAccess: true })
  .build()
