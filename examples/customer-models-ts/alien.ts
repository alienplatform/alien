import * as alien from "@alienplatform/core"

const models = new alien.AI("models").build()

export default new alien.Stack("customer-models")
  .platforms(["aws", "gcp"])
  .add(models, "frozen", { remoteAccess: true })
  .build()
