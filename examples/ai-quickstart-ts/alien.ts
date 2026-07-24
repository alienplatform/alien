import * as alien from "@alienplatform/core"

// An AI model the app can call, served by the customer's own cloud; no API keys.
const assistant = new alien.AI("assistant").build()

const api = new alien.Worker("api")
  .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
  .publicEndpoint("api")
  .link(assistant)
  .permissions("execution")
  .build()

export default new alien.Stack("ai-quickstart")
  .platforms(["aws", "gcp", "azure"])
  .add(assistant, "live")
  .add(api, "live")
  .permissions({
    profiles: {
      execution: {
        "*": ["ai/invoke"],
      },
    },
  })
  .build()
