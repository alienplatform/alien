import { defineDeploymentSuite } from "../../harness/suite.js"

defineDeploymentSuite({
  name: "Rust container - Kubernetes",
  app: "test-apps/comprehensive-rust",
  config: "alien.container.ts",
  platform: "kubernetes",
})
