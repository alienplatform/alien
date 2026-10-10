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
