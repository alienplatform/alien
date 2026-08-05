import * as alien from "@alienplatform/core"

const enterpriseKey = new alien.Key("enterprise-key").build()
const storageKey = new alien.Key("storage-key").build()
const storage = new alien.Storage("customer-data")
  .encryptionKey(storageKey)
  .build()

export default new alien.Stack("byo-encryption-key")
  .add(enterpriseKey, "frozen", { remoteAccess: true })
  .add(storageKey, "frozen")
  .add(storage, "frozen")
  .build()
