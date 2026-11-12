import * as alien from "@alienplatform/core"

const compute = new alien.ComputeCluster("compute")
  .pool("edge", {
    requirements: { cpu: 2, memory: "8Gi" },
    scale: { type: "fixed", machines: 4 },
  })
  .build()

const reports = new alien.Storage("reports").build()

const agent = new alien.Daemon("agent")
  .cluster("compute")
  .pool("edge")
  .code({
    type: "source",
    src: "agent",
    toolchain: { type: "rust", binaryName: "acme-agent" },
  })
  .command(["acme-agent", "--mode", "resident"])
  .commandsEnabled(true)
  .cpu(0.5)
  .memory("512Mi")
  .stopGracePeriod(120)
  .priority("low")
  .healthCheck({
    port: 9100,
    path: "/healthz",
    method: "HEAD",
    timeoutSeconds: 3,
    failureThreshold: 4,
  })
  .runtime({
    networkMode: "host",
    pidNamespace: "host",
    privileged: true,
    user: "0",
    mounts: [
      { source: "/var/lib/acme", target: "/state", options: "ro" },
      { source: "/etc/acme", target: "/config" },
    ],
  })
  .link(reports)
  .permissions("agent")
  .environment({ AGENT_MODE: "resident", AGENT_BATCH: "50" })
  .publicEndpoint("status", 9100, {
    protocol: "http",
    hostLabel: "status",
    wildcardSubdomains: true,
  })
  .publicEndpoint("control", 9100, { protocol: "http", hostLabel: "control" })
  .build()

export default new alien.Stack("acme-agent")
  .platforms(["machines", "aws"])
  .add(compute, "frozen")
  .add(reports, "frozen")
  .add(agent, "live")
  .permissions({ profiles: { agent: { reports: ["storage/data-write"] } } })
  .build()
