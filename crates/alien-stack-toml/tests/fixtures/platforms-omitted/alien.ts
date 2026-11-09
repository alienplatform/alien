import * as alien from "@alienplatform/core"

const data = new alien.Storage("data").build()

export default new alien.Stack("acme-any-platform").add(data, "frozen").build()
