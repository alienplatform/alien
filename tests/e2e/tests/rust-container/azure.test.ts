import { defineDeploymentSuite } from "../../harness/suite.js"

defineDeploymentSuite({
  name: "Rust container - Azure",
  app: "test-apps/comprehensive-rust",
  config: "alien.config.container.ts",
  platform: "azure",
})
