import * as alien from "@alienplatform/core"

const uploads = new alien.Storage("uploads").build()

export default new alien.Stack("byob-storage")
  .add(uploads, "frozen", { remoteAccess: true })
  .build()
