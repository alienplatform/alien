import * as alien from "@alienplatform/core"

const api = new alien.Worker("api")
  .code({ type: "image", image: "ghcr.io/acme/api:1.0.0" })
  .publicEndpoint("api")
  .publicEndpoint("webhooks")
  .permissions("api")
  .build()

const sites = new alien.Worker("sites")
  .code({ type: "image", image: "ghcr.io/acme/sites:1.0.0" })
  .publicEndpoint("marketing", { hostLabel: "@" })
  .publicEndpoint("customers", { hostLabel: "customers", wildcardSubdomains: true })
  .publicEndpoint("docs")
  .permissions("sites")
  .build()

const privateWorker = new alien.Worker("private")
  .code({ type: "image", image: "ghcr.io/acme/private:1.0.0" })
  .permissions("private")
  .build()

export default new alien.Stack("acme-worker-endpoints")
  .platforms(["aws", "gcp"])
  .add(api, "live")
  .add(sites, "live")
  .add(privateWorker, "live")
  .permissions({ profiles: { api: {}, sites: {}, private: {} } })
  .build()
