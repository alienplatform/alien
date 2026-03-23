import { defineDeploymentSuite } from "../../harness/suite.js"

defineDeploymentSuite({
  name: "Rust function - GCP",
  app: "test-apps/comprehensive-rust",
  config: "alien.function.ts",
  platform: "gcp",
})
