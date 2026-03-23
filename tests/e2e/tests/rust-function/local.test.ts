import { defineDeploymentSuite } from "../../harness/suite.js"

defineDeploymentSuite({
  name: "Rust function - Local",
  app: "test-apps/comprehensive-rust",
  config: "alien.function.ts",
  platform: "local",
})
