import * as alien from "@alienplatform/core"

const reports = new alien.Storage("reports").build()

export default new alien.Stack("acme-management-auto")
  .platforms(["aws"])
  .add(reports, "frozen")
  .permissions({
    profiles: { reader: { reports: ["storage/data-read"] } },
    management: "auto",
  })
  .build()
