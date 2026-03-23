import { defineDeploymentSuite } from "../../harness/suite.js"

defineDeploymentSuite({
  name: "Rust function - Azure",
  app: "test-apps/comprehensive-rust",
  config: "alien.function.ts",
  platform: "azure",
})
