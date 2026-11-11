import * as alien from "@alienplatform/core"

const compute = new alien.ComputeCluster("compute")
  .pool("general", {
    requirements: { cpu: 4, memory: "16Gi" },
    scale: { type: "fixed", machines: 2 },
  })
  .build()

const cache = new alien.Kv("cache").build()
const dataKey = new alien.Key("data-key").build()

const ledger = new alien.Container("ledger")
  .cluster("compute")
  .pool("general")
  .code({ type: "image", image: "ghcr.io/acme/ledger:3.1.0" })
  .command(["ledger", "serve", "--verbose"])
  .commandsEnabled(true)
  .cpu({ min: 1, max: 4, default: 2 })
  .memory({ min: "2Gi", max: "16Gi", default: "4Gi" })
  .ephemeralStorage("20Gi")
  .gpu({ type: "nvidia-l4", count: 1 })
  .ports([9090, 9091])
  .publicEndpoint("api", 8080, { protocol: "http", hostLabel: "ledger", wildcardSubdomains: true })
  .egress("internet")
  .security({
    profile: "restricted",
    runAsUser: 10001,
    runAsGroup: 10002,
    readOnlyRootFilesystem: true,
    writablePaths: ["/var/cache/ledger"],
  })
  .persistentStorage("100Gi", {
    mountPath: "/var/lib/ledger",
    encryptionKey: dataKey,
    backups: { intervalHours: 6, retentionDays: 14 },
  })
  .autoScale({
    min: 2,
    desired: 3,
    max: 12,
    targetCpuPercent: 65,
    targetMemoryPercent: 75,
    targetHttpInFlightPerReplica: 100,
    maxHttpP95LatencyMs: 250,
  })
  .healthCheck({
    port: 8080,
    path: "/ready",
    method: "HEAD",
    timeoutSeconds: 2,
    failureThreshold: 5,
  })
  .kubernetesLivenessProbe({ path: "/livez", port: 8080 })
  .kubernetesReadinessProbe({ path: "/readyz", port: 8080 })
  .kubernetesSecretMount({ secretName: "ledger-tls", mountPath: "/etc/ledger/tls" })
  .stopGracePeriod(300)
  .link(cache)
  .permissions("ledger")
  .environment({ LOG_LEVEL: "debug", WORKER_THREADS: "4", TRACE: "true" })
  .build()

export default new alien.Stack("acme-ledger")
  .platforms(["aws", "gcp"])
  .add(compute, "frozen")
  .add(cache, "frozen")
  .add(dataKey, "frozen")
  .add(ledger, "live")
  .permissions({ profiles: { ledger: { cache: ["kv/data-read", "kv/data-write"] } } })
  .build()
