import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  snapshotsEnabled: alien.boolean({
    providedBy: "deployer",
    required: true,
    label: "Create snapshots",
    description: "Whether this deployment gets the 'snapshots' storage bucket.",
    default: true,
    platforms: ["aws"],
  }),
  authAccessKey: alien.secret({
    providedBy: "developer",
    required: true,
    label: "Auth access key",
    description: "Sets the AUTH_ACCESS_KEY environment variable.",
    env: { name: "AUTH_ACCESS_KEY", targetResources: ["loader"] },
  }),
  tailnetAuthKey: alien.secret({
    providedBy: "developer",
    required: false,
    platforms: ["machines"],
    label: "TAILNET_AUTH_KEY",
    description: "Sets the TAILNET_AUTH_KEY environment variable.",
    env: { name: "TAILNET_AUTH_KEY", targetResources: ["loader"] },
  }),
})

const compute = new alien.ComputeCluster("compute")
  .pool("general", {
    requirements: {
      cpu: 4,
      memory: "16Gi",
      ephemeralStorage: "20Gi",
      architecture: "x86_64",
      nestedVirtualization: true,
    },
    scale: { type: "fixed", machines: { min: 1, max: 5, default: 2 } },
  })
  .build()

const snapshots = new alien.Storage("snapshots").enabled(inputs.snapshotsEnabled).build()

const loader = new alien.Daemon("loader")
  .code({ type: "source", src: ".", toolchain: { type: "rust", binaryName: "loader" } })
  .cluster("compute")
  .pool("general")
  .cpu(2)
  .memory("4Gi")
  .runtime({
    privileged: true,
    pidNamespace: "host",
    networkMode: "host",
    mounts: [{ source: "/", target: "/host" }],
    user: "0",
  })
  .stopGracePeriod(86_400)
  .healthCheck({ port: 18082, path: "/health" })
  .environment({
    LOADER_HOST_ROOT: "/host",
    LOADER_STATE_DIR: "/host/var/lib/sandbox-loader",
    LOADER_HEALTH_LISTEN: "127.0.0.1:18082",
    LOADER_GREETING: "hello # not a comment",
    RUST_LOG: "info",
  })
  .link(snapshots)
  .publicEndpoint("api", 8080, { protocol: "http", hostLabel: "@" })
  .publicEndpoint("shares", 8080, {
    protocol: "http",
    hostLabel: "shares",
    wildcardSubdomains: true,
  })
  .build()

export default new alien.Stack("sandbox-runtime")
  .inputs(inputs)
  .platforms(["machines", "aws"])
  .add(compute, "frozen")
  .add(snapshots, "frozen")
  .add(loader, "live")
  .permissions({ profiles: { loader: {} } })
  .build()
