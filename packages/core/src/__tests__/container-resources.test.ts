import { expect, it } from "vitest"
import { Container } from "../container.js"
import { ContainerInternalLoadBalancerSchema } from "../generated/zod/container-internal-load-balancer-schema.js"

it("clears deployment choices when both resources become fixed", () => {
  const resource = new Container("api")
    .code({ type: "image", image: "nginx:alpine" })
    .permissions("app")
    .cpu({ min: 1, max: 4, default: 2 })
    .memory({ min: "1Gi", max: "8Gi", default: "2Gi" })
    .cpu(1)
    .memory("1Gi")
    .build()
  expect(resource.config).toMatchObject({
    cpu: { min: "1", desired: "1" },
    memory: { min: "1Gi", desired: "1Gi" },
  })
  expect(resource.config).not.toHaveProperty("resourceChoices", expect.anything())
})

it.each([
  {
    ports: [3128],
    subnets: ["subnet-1"],
    allowedSources: { "3128": ["10.0.0.0/24"] },
  },
  {
    ports: [3128, 3129],
    network: "default-network",
    subnetGroup: "sandbox-gateway",
    allowedSourceGroups: { "3128": "sandbox", "3129": "runtime" },
  },
])("preserves internal listener placement through validation and build", (settings) => {
  const validated = ContainerInternalLoadBalancerSchema.parse(settings)
  const resource = new Container("gateway")
    .code({ type: "image", image: "example:latest" })
    .permissions("gateway")
    .cpu(1)
    .memory("128Mi")
    .internalLoadBalancer(validated)
    .build()
  expect(resource.config.internalLoadBalancer).toEqual(settings)
})
