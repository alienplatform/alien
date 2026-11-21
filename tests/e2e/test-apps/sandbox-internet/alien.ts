import * as alien from "@alienplatform/core"

// A Google Cloud sandbox with egress `internet`: every connection leaves through the deployment's
// egress gateway, under the rules each sandbox is created with. Not published for remote access:
// policy administration stays private to the deployment, so the test reaches it from inside the VPC.
const agents = new alien.Sandbox("agents")
  .code({
    type: "source",
    src: "./sandbox",
    toolchain: { type: "docker", dockerfile: "Dockerfile" },
  })
  .egress("internet")
  .lifecycle({})
  .build()

// On Google Cloud an internet sandbox answers only to an edge credential signed for an identity
// that holds `sandbox/execute` on it, so the plan requires a profile that does. The test calls the
// sandbox as this workload, which also creates and deletes sandboxes: `sandbox/management`.
// Creating or updating a sandbox signs its gateway policy with the deployment's egress signing
// key, which the workload reads from the secrets vault: `vault/data-read` on `secrets`.
export default new alien.Stack("sandbox-internet")
  .add(agents, "frozen")
  .permissions({
    profiles: {
      operator: {
        agents: ["sandbox/execute", "sandbox/management"],
        secrets: ["vault/data-read"],
      },
    },
  })
  .build()
