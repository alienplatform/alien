import * as alien from "@alienplatform/core"

const records = new alien.Key("records")
  .displayName("Records key")
  .description("Customer-managed key for the records at rest.")
  .build()

export default new alien.Stack("acme-key")
  .platforms(["aws", "gcp", "azure"])
  .add(records, "frozen", { remoteAccess: true })
  .build()
