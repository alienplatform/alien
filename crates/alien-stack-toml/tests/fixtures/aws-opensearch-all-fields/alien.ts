import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  search_enabled: alien.boolean({
    providedBy: "deployer",
    required: true,
    label: "Create catalog search",
    description: "Whether this deployment gets the catalog search collection.",
    default: true,
  }),
})

const catalog = new alien.experimental.AwsOpenSearch("catalog")
  .displayName("Catalog search")
  .description("Vector index over the product catalog.")
  .enabled(inputs.search_enabled)
  .collectionType("vectorSearch")
  .capacity({
    indexing: { minOcu: 2, maxOcu: 8 },
    search: { minOcu: 2, maxOcu: 32 },
  })
  .build()

const logs = new alien.experimental.AwsOpenSearch("logs")
  .capacity({ search: { maxOcu: 16 } })
  .build()

export default new alien.Stack("acme-search")
  .inputs(inputs)
  .platforms(["aws"])
  .add(catalog, "live", { remoteAccess: true })
  .add(logs, "frozen")
  .build()
