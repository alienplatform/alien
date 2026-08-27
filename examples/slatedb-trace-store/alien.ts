import * as alien from "@alienplatform/core"

const data = new alien.Storage("data").lifecycleRules([{ prefix: "staging/v1/", days: 7 }]).build()
const ingestion = new alien.Queue("ingestion").build()

const code = {
  type: "source" as const,
  src: ".",
  toolchain: { type: "rust" as const, binaryName: "slatedb-trace-store" },
}

const api = new alien.Container("api")
  .code(code)
  .cpu(0.5)
  .memory("512Mi")
  .port(8080)
  .autoScale({
    min: 2,
    desired: 2,
    max: 4,
    targetHttpInFlightPerReplica: 100,
  })
  .publicEndpoint("api", 8080, "http")
  .healthCheck({ path: "/health", method: "GET", timeoutSeconds: 2, failureThreshold: 3 })
  .environment({ TRACE_STORE_MODE: "api", PORT: "8080", RUST_LOG: "info" })
  .permissions("api")
  .link(data)
  .link(ingestion)
  .build()

const writer = new alien.Container("writer")
  .code(code)
  .cpu(1)
  .memory("1Gi")
  .port(8081)
  .replicas(1)
  .healthCheck({ path: "/health", method: "GET", timeoutSeconds: 2, failureThreshold: 3 })
  .environment({ TRACE_STORE_MODE: "writer", PORT: "8081", RUST_LOG: "info" })
  .permissions("writer")
  .link(data)
  .link(ingestion)
  .build()

export default new alien.Stack("slatedb-trace-store")
  .platforms(["aws", "gcp", "azure"])
  .add(data, "frozen")
  .add(ingestion, "frozen")
  .add(api, "live")
  .add(writer, "live")
  .permissions({
    profiles: {
      api: {
        data: ["storage/data-read", "storage/data-write"],
        ingestion: ["queue/data-write"],
      },
      writer: {
        data: ["storage/data-read", "storage/data-write"],
        ingestion: ["queue/data-read", "queue/data-write"],
      },
    },
  })
  .build()
