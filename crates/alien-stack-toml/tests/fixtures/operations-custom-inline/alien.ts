import * as alien from "@alienplatform/core"

export default new alien.Stack("acme-pinned-operations")
  .platforms(["gcp"])
  .operations({
    plugins: [
      "acme-metrics@1.2.0",
      "@acme/diagnostics@1.4.0",
      { name: "acme-cleanup", version: "2.0.0", mode: "dry-run", approval: { "*": "auto" } },
    ],
  })
  .build()
