import { defineDeploymentSuite } from "../../harness/suite.js"

defineDeploymentSuite({
  name: "Rust container - Local",
  app: "test-apps/comprehensive-rust",
  config: "alien.config.container.ts",
  platform: "local",
})
