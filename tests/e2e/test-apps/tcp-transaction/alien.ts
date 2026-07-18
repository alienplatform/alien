import * as alien from "@alienplatform/core"

const service = new alien.Container("transaction-service")
  .code({
    type: "source",
    src: "./",
    toolchain: { type: "rust", binaryName: "tcp-transaction-server" },
  })
  .cpu(0.25)
  .memory("256Mi")
  .replicas(2)
  .port(7000)
  .publicEndpoint("transactions", 7000, "tcp")
  .healthCheck({ method: "GET", path: "/health", port: 8080 })
  .environment({ E2E_TCP_VERSION: "v1", RUST_LOG: "info" })
  .permissions("execution")
  .build()

export default new alien.Stack("tcp-transaction")
  .platforms(["aws", "gcp", "azure"])
  .add(service, "live")
  .permissions({ profiles: { execution: { "*": [] } } })
  .build()
