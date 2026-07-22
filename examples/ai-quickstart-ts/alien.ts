import * as alien from "@alienplatform/core"

// A model-less AI resource. The customer's cloud serves the inference; the
// embedded gateway injects the workload's ambient identity, so no API keys.
const llm = new alien.AI("llm").build()

const api = new alien.Worker("api")
  .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
  .publicEndpoint("api")
  // Linking injects ALIEN_LLM_BINDING and exposes the gateway as ALIEN_AI_GATEWAY_URL.
  .link(llm)
  .permissions("execution")
  .build()

export default new alien.Stack("ai-quickstart")
  .platforms(["aws", "gcp", "azure"])
  .add(llm, "live")
  .add(api, "live")
  .permissions({
    profiles: {
      execution: {
        "*": ["ai/invoke"],
      },
    },
  })
  .build()
