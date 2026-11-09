import * as alien from "@alienplatform/core"

const inputs = alien.inputs({
  api_token: alien.secret({
    providedBy: "developer",
    required: true,
    label: "API token",
    description: "Token the containers present to the control plane.",
    env: { name: "API_TOKEN", targetResources: ["with-inputs", "with-inputs-too"] },
  }),
  region: alien.string({
    providedBy: "deployer",
    required: true,
    label: "Region",
    description: "Region the containers report from.",
    env: { name: "REGION", targetResources: ["with-inputs"], type: "plain" },
  }),
})

const inline = new alien.Container("inline")
  .code({ type: "image", image: "ghcr.io/acme/inline:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .environment({ NODE_ENV: "production", PORT: "3000", RATIO: "0.75", DEBUG: "false" })
  .permissions("inline")
  .build()

const oneFile = new alien.Container("one-file")
  .code({ type: "image", image: "ghcr.io/acme/one-file:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .environment({
    APP_MODE: "standard",
    APP_GREETING: 'hello, "world"',
    APP_PATTERN: "^[a-z]+$ # kept",
    LOG_LEVEL: "info",
  })
  .permissions("one-file")
  .build()

const merged = new alien.Container("merged")
  .code({ type: "image", image: "ghcr.io/acme/merged:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .environment({
    APP_MODE: "standard",
    APP_GREETING: 'hello, "world"',
    APP_PATTERN: "^[a-z]+$ # kept",
    LOG_LEVEL: "error",
    APP_REGION: "eu-west-1",
    EXTRA: "1",
  })
  .permissions("merged")
  .build()

const withInputs = new alien.Container("with-inputs")
  .code({ type: "image", image: "ghcr.io/acme/with-inputs:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .environment({ MODE: "inputs" })
  .permissions("with-inputs")
  .build()

const withInputsToo = new alien.Container("with-inputs-too")
  .code({ type: "image", image: "ghcr.io/acme/with-inputs:1.0.0" })
  .cpu(1)
  .memory("1Gi")
  .permissions("with-inputs-too")
  .build()

export default new alien.Stack("acme-env")
  .platforms(["aws"])
  .inputs(inputs)
  .add(inline, "live")
  .add(oneFile, "live")
  .add(merged, "live")
  .add(withInputs, "live")
  .add(withInputsToo, "live")
  .permissions({
    profiles: {
      inline: {},
      "one-file": {},
      merged: {},
      "with-inputs": {},
      "with-inputs-too": {},
    },
  })
  .build()
