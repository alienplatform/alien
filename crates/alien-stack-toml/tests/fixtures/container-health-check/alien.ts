import * as alien from "@alienplatform/core"

const defaults = new alien.Container("defaults")
  .code({ type: "image", image: "ghcr.io/acme/defaults:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .healthCheck({ path: "/health", method: "GET", timeoutSeconds: 1, failureThreshold: 3 })
  .permissions("defaults")
  .build()

const pathOnly = new alien.Container("path-only")
  .code({ type: "image", image: "ghcr.io/acme/path-only:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .healthCheck({ path: "/ready", method: "GET", timeoutSeconds: 1, failureThreshold: 3 })
  .permissions("path-only")
  .build()

const everything = new alien.Container("everything")
  .code({ type: "image", image: "ghcr.io/acme/everything:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .ports([8080, 9090])
  .healthCheck({
    path: "/livez",
    port: 9090,
    method: "POST",
    timeoutSeconds: 4,
    failureThreshold: 10,
  })
  .permissions("everything")
  .build()

export default new alien.Stack("acme-health")
  .platforms(["aws"])
  .add(defaults, "live")
  .add(pathOnly, "live")
  .add(everything, "live")
  .permissions({ profiles: { defaults: {}, "path-only": {}, everything: {} } })
  .build()
