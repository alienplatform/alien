import * as alien from "@alienplatform/core"

const events = new alien.Kv("events").build()

const stackInputs = alien.inputs({
  webhookSigningSecret: alien.secret({
    providedBy: "developer",
    required: true,
    label: "Webhook signing secret",
    description: "Shared secret used by the API to verify incoming webhook signatures.",
    minLength: 16,
    env: {
      name: "WEBHOOK_SIGNING_SECRET",
      targetResources: ["api"],
    },
  }),
  deliveryCallbackUrl: alien.string({
    providedBy: "deployer",
    required: true,
    label: "Delivery callback URL",
    description: "HTTPS endpoint that receives processed webhook delivery notifications.",
    placeholder: "https://hooks.example.com/alien",
    format: "url",
    env: {
      name: "DELIVERY_CALLBACK_URL",
      targetResources: ["api"],
    },
  }),
})

const api = new alien.Worker("api")
  .code({ type: "source", src: "./", toolchain: { type: "typescript" } })
  .commandsEnabled(true)
  .publicEndpoint("api")
  .link(events)
  .permissions("execution")
  .build()

export default new alien.Stack("webhook-api")
  .inputs(stackInputs)
  .add(events, "frozen")
  .add(api, "live")
  .permissions({
    profiles: {
      execution: {
        "*": ["kv/data-read", "kv/data-write"],
      },
    },
  })
  .build()
