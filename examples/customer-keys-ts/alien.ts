import * as alien from "@alienplatform/core"

const customerKey = new alien.Key("customer-key").build()

export default new alien.Stack("customer-keys")
  .add(customerKey, "frozen", { remoteAccess: true })
  .build()
