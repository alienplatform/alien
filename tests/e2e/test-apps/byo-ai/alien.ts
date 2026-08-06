import * as alien from "@alienplatform/core"

const models = new alien.AI("customer-models").build()

export default new alien.Stack("byo-ai").add(models, "frozen", { remoteAccess: true }).build()
