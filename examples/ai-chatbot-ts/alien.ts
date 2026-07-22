import * as alien from "@alienplatform/core"

// A model-less AI resource. The customer's cloud serves the inference; the
// embedded gateway injects the workload's ambient identity, so no API keys.
const llm = new alien.AI("llm").build()

// A private Postgres the chatbot queries through a tool. Reachable only from
// same-stack workloads; the app resolves its connection at runtime from the
// binding, never from a checked-in secret.
const db = new alien.Postgres("db").build()

const app = new alien.Container("app")
  .code({ type: "source", src: ".", toolchain: { type: "docker", dockerfile: "Dockerfile" } })
  .cpu(0.5)
  .memory("512Mi")
  .port(3000)
  .publicEndpoint("web", 3000, "http")
  // Next's standalone server reads these; HOSTNAME=0.0.0.0 binds all interfaces.
  .environment({ PORT: "3000", HOSTNAME: "0.0.0.0" })
  // Linking injects ALIEN_LLM_BINDING (and starts the gateway, exposed as
  // ALIEN_AI_GATEWAY_URL) and ALIEN_DB_BINDING (the Postgres connection).
  .link(llm)
  .link(db)
  .permissions("app")
  .build()

export default new alien.Stack("ai-chatbot")
  .platforms(["aws", "gcp", "azure"])
  .add(llm, "live")
  .add(db, "live")
  .add(app, "live")
  .permissions({
    profiles: {
      app: {
        // invoke the model + read the DB connection-password secret and connect.
        "*": ["ai/invoke", "postgres/data-access"],
      },
    },
  })
  .build()
