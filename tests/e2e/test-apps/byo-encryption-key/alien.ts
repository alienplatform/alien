import * as alien from "@alienplatform/core"

const enterpriseKey = new alien.Key("enterprise-key").build()
const storage = new alien.Storage("encrypted-storage").encryptionKey(enterpriseKey).build()

export default new alien.Stack("byo-encryption-key")
  .add(enterpriseKey, "frozen", { remoteAccess: true })
  .add(storage, "frozen")
  .build()
