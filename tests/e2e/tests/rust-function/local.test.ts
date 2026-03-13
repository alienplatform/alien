import { defineDeploymentSuite } from "../../harness/suite.js"

defineDeploymentSuite({
  name: "Rust function - Local",
  app: "test-apps/comprehensive-rust",
  config: "alien.config.function.ts",
  platform: "local",
})
