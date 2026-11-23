// EXAMPLE: one sandbox per conversation, inside the customer's cloud.
//
// The vendor's control plane (control-plane/, runs anywhere) drives this stack with commands:
//
//   runner     holds the sandbox binding: opens, resumes and pauses one sandbox per thread
//              and runs each agent turn in it.
//   agent      the Sandbox. Its only ways out are two links: the model and the connector.
//              It holds placeholder keys, never real credentials.
//   model      forwards the agent's model calls to the deployment's own AI service
//              (Bedrock on AWS) with the container's identity.
//   connector  the only way to the customer's internal API. Private, reachable from a
//              sandbox only through its link. Reads pass, writes wait for approval.

import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  internalApiUrl: alien.string({
    providedBy: "deployer",
    required: true,
    label: "Internal API URL",
    description: "Private address of the internal API the connector calls.",
    placeholder: "http://10.40.10.50:8080",
    format: "url",
    env: { name: "INTERNAL_API_URL", targetResources: ["connector"] },
  }),
})

const llm = new alien.AI("llm").build()

// Serves the Anthropic Messages API to the sandboxes, backed by the AI binding.
const model = new alien.Container("model")
  .code({ type: "source", src: "./services/model", toolchain: { type: "typescript" } })
  .port(8080)
  .cpu(0.25)
  .memory("256Mi")
  .link(llm)
  .permissions("model")
  .build()

// The sandboxes' only way to the internal API. No public endpoint.
const connector = new alien.Container("connector")
  .code({ type: "source", src: "./services/connector", toolchain: { type: "typescript" } })
  .port(8080)
  .cpu(0.25)
  .memory("256Mi")
  // Turn tokens and held writes live in this process's memory.
  .replicas(1)
  .commandsEnabled(true)
  .permissions("app")
  .build()

const agent = new alien.Sandbox("agent")
  .code(
    process.env.SANDBOX_IMAGE
      ? { type: "image", image: process.env.SANDBOX_IMAGE }
      : { type: "source", src: "./sandbox", toolchain: { type: "docker", dockerfile: "Dockerfile" } },
  )
  .egress("internet")
  // No internet: only the two links. A sandbox the runner creates gets this policy.
  .defaultPolicy({ allow: ["link:model", "link:connector"] })
  .link(model)
  .link(connector)
  // A thread's sandbox ends after 8 hours (the AWS ceiling); the router pauses it while idle.
  .lifecycle({ maxLifetimeSeconds: 8 * 3600 })
  .build()

const runner = new alien.Container("runner")
  .code({ type: "source", src: "./services/runner", toolchain: { type: "typescript" } })
  .cpu(0.5)
  .memory("512Mi")
  .replicas(1)
  .commandsEnabled(true)
  .environment({ MODEL: process.env.MODEL ?? "claude-sonnet-4-6" })
  .link(agent)
  .permissions("runner")
  .build()

// AWS builds a source sandbox from the release, which needs the live lifecycle; local
// development takes a frozen one. Release for AWS with SANDBOX_LIFECYCLE=live.
const sandboxLifecycle = process.env.SANDBOX_LIFECYCLE === "live" ? "live" : "frozen"

export default new alien.Stack("agent-sandboxes")
  .platforms(["local", "aws"])
  .inputs(inputs)
  .add(llm, "live")
  .add(model, "live")
  .add(connector, "live")
  .add(agent, sandboxLifecycle)
  .add(runner, "live")
  .permissions({
    profiles: {
      runner: { "*": ["sandbox/execute", "sandbox/management"] },
      model: { "*": ["ai/invoke"] },
      app: { "*": [] },
    },
  })
  .build()
