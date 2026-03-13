import { defineDeploymentSuite } from "../../harness/suite.js"

defineDeploymentSuite({
  name: "Rust container - GCP",
  app: "test-apps/comprehensive-rust",
  config: "alien.config.container.ts",
  platform: "gcp",
})
