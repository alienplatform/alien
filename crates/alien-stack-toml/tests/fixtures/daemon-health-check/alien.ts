import * as alien from "@alienplatform/core"

const defaults = new alien.Daemon("defaults")
  .code({ type: "image", image: "ghcr.io/acme/defaults:1.0.0" })
  .healthCheck({ path: "/health", method: "GET", timeoutSeconds: 1, failureThreshold: 3 })
  .build()

const pathOnly = new alien.Daemon("path-only")
  .code({ type: "image", image: "ghcr.io/acme/path-only:1.0.0" })
  .healthCheck({ path: "/ready", method: "GET", timeoutSeconds: 1, failureThreshold: 3 })
  .build()

const everything = new alien.Daemon("everything")
  .code({ type: "image", image: "ghcr.io/acme/everything:1.0.0" })
  .healthCheck({
    path: "/status",
    port: 8081,
    method: "POST",
    timeoutSeconds: 5,
    failureThreshold: 2,
  })
  .build()

const unchecked = new alien.Daemon("unchecked")
  .code({ type: "image", image: "ghcr.io/acme/unchecked:1.0.0" })
  .build()

export default new alien.Stack("acme-daemon-health")
  .platforms(["machines"])
  .add(defaults, "live")
  .add(pathOnly, "live")
  .add(everything, "live")
  .add(unchecked, "live")
  .build()
