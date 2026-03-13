import { defineDeploymentSuite } from "../../harness/suite.js"

defineDeploymentSuite({
  name: "Rust container - AWS",
  app: "test-apps/comprehensive-rust",
  config: "alien.config.container.ts",
  platform: "aws",
})
