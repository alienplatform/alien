import { defineDeploymentSuite } from "../../harness/suite.js"

defineDeploymentSuite({
  name: "Rust function - AWS",
  app: "test-apps/comprehensive-rust",
  config: "alien.function.ts",
  platform: "aws",
})
