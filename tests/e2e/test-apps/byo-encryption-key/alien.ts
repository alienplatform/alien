import * as alien from "@alienplatform/core"

const key = new alien.Key("enterprise-key").build()

export default new alien.Stack("byo-encryption-key")
  .add(key, "frozen", { remoteAccess: true })
  .build()
