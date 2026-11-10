import * as alien from "@alienplatform/core"

const web = new alien.Container("web")
  .code({ type: "image", image: "ghcr.io/acme/web:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .port(9000)
  .publicEndpoint("site", 3000)
  .permissions("web")
  .build()

const gateway = new alien.Container("gateway")
  .code({ type: "image", image: "ghcr.io/acme/gateway:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .ports([8080, 5432])
  .publicEndpoint("api", 8080, { protocol: "http", hostLabel: "api" })
  .publicEndpoint("admin", 8080, { protocol: "http", hostLabel: "admin" })
  .publicEndpoint("apps", 8080, {
    protocol: "http",
    hostLabel: "apps",
    wildcardSubdomains: true,
  })
  .permissions("gateway")
  .build()

const database = new alien.Container("database")
  .code({ type: "image", image: "ghcr.io/acme/database:1.0.0" })
  .cpu(2)
  .memory("4Gi")
  .publicEndpoint("sql", 5432, "tcp")
  .permissions("database")
  .build()

export default new alien.Stack("acme-endpoints")
  .platforms(["aws", "gcp"])
  .add(web, "live")
  .add(gateway, "live")
  .add(database, "live")
  .permissions({ profiles: { web: {}, gateway: {}, database: {} } })
  .build()
