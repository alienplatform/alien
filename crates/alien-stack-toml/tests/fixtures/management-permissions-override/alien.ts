import * as alien from "@alienplatform/core"

const reports = new alien.Storage("reports").build()
const cache = new alien.Kv("cache").build()

export default new alien.Stack("acme-management-override")
  .platforms(["aws"])
  .add(reports, "frozen")
  .add(cache, "frozen")
  .permissions({
    profiles: {},
    management: {
      override: {
        "*": ["storage/management"],
        cache: ["kv/management"],
      },
    },
  })
  .build()
