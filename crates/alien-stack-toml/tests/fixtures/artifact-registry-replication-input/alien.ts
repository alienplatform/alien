import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  deployment_regions: alien.stringList({
    providedBy: "deployer",
    required: true,
    label: "Deployment regions",
    description: "Regions the deployments run in.",
    default: [],
  }),
})

const images = new alien.ArtifactRegistry("images")
  .replicationRegionsFrom(inputs.deployment_regions)
  .build()

export default new alien.Stack("acme-registry-input")
  .platforms(["aws"])
  .inputs(inputs)
  .add(images, "frozen")
  .build()
