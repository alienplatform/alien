import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  token: alien.secret({
    providedBy: "developer",
    required: true,
    label: "Token",
    description: "Token the workloads present to the control plane.",
    env: "TOKEN",
  }),
})

const api = new alien.Container("api")
  .code({ type: "image", image: "ghcr.io/acme/api:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .permissions("api")
  .build()

const worker = new alien.Container("worker")
  .code({ type: "image", image: "ghcr.io/acme/worker:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .permissions("worker")
  .build()

export default new alien.Stack("acme-token")
  .platforms(["aws"])
  .inputs(inputs)
  .add(api, "live")
  .add(worker, "live")
  .permissions({ profiles: { api: {}, worker: {} } })
  .build()
