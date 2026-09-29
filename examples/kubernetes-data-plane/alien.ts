// A data plane that runs in each customer's Kubernetes cluster and keeps
// their data in the object storage they already operate.

import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  accessToken: alien.secret({
    providedBy: "developer",
    required: true,
    label: "Access token",
    description: "Token your control plane sends in the Authorization header.",
    minLength: 32,
    env: { name: "ACCESS_TOKEN", targetResources: ["api"] },
  }),
})

// The customer's S3-compatible bucket (S3, MinIO, Ceph, ...), supplied at install time.
const objects = new alien.Storage("objects").build()

const api = new alien.Container("api")
  .code({
    type: "source",
    src: ".",
    toolchain: { type: "rust", binaryName: "data-plane" },
  })
  .cpu(0.5)
  .memory("512Mi")
  // Reachable from your control plane through the manager. No inbound access
  // to the customer's network is needed.
  .tunnel(8080)
  .healthCheck({ path: "/health", method: "GET", timeoutSeconds: 2, failureThreshold: 3 })
  .link(objects)
  .permissions("api")
  .build()

export default new alien.Stack("data-plane")
  .platforms(["kubernetes"])
  .inputs(inputs)
  .add(objects, "frozen")
  .add(api, "live")
  .permissions({
    profiles: {
      api: { objects: ["storage/data-read", "storage/data-write"] },
    },
  })
  .build()
