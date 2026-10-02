// A files service that runs inside each customer's Kubernetes cluster.
//
// Your control plane stays in your cloud. This is the part that has to live
// next to the customer's data: it stores files in the customer's own
// S3-compatible bucket, and your control plane calls it through your
// manager, over a connection the cluster opens outbound.

import * as alien from "@alienplatform/core"

// Set per customer when you onboard them. The service requires it on every
// request, so the application keeps its own authentication end to end.
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

// The customer's bucket: Amazon S3, MinIO, Ceph or any S3-compatible store.
// The customer's admin names it in the Helm values when they install.
const bucket = new alien.Storage("bucket").build()

const api = new alien.Container("api")
  .code({
    type: "source",
    src: ".",
    toolchain: { type: "rust", binaryName: "files" },
  })
  .cpu(0.5)
  .memory("512Mi")
  // Reachable from your control plane through the manager, and from nowhere
  // else: the customer opens no inbound port.
  .tunnel(8080)
  .healthCheck({ path: "/health", method: "GET", timeoutSeconds: 2, failureThreshold: 3 })
  .link(bucket)
  .permissions("api")
  .build()

export default new alien.Stack("files")
  .platforms(["kubernetes"])
  .inputs(inputs)
  .add(bucket, "frozen")
  .add(api, "live")
  .permissions({
    profiles: {
      api: { bucket: ["storage/data-read", "storage/data-write"] },
    },
  })
  .build()
