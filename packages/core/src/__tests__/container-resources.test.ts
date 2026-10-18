import { expect, it } from "vitest"
import { Container } from "../container.js"

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

it("keeps internet egress opt-in and container ports reachable through links", () => {
  const api = new Container("api")
    .code({ type: "image", image: "example:latest" })
    .cpu(1)
    .memory("512Mi")
    .permissions("app")
    .port(3128)
    .port(3129)
    .security({
      profile: "restricted",
      runAsUser: 65532,
      runAsGroup: 65532,
      readOnlyRootFilesystem: true,
    })
    .egress("internet")
    .build()
  const caller = new Container("caller")
    .code({ type: "image", image: "example:latest" })
    .cpu(1)
    .memory("512Mi")
    .permissions("app")
    .link(api)
    .build()
  expect(api.config).toMatchObject({ ports: [{ port: 3128 }, { port: 3129 }], egress: "internet" })
  expect(caller.config).toMatchObject({ links: [{ type: "container", id: "api" }] })
  expect(caller.config.egress).toBeUndefined()
})
