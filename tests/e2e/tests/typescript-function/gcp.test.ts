import { defineDeploymentSuite } from "../../harness/suite.js"

defineDeploymentSuite({
  name: "TypeScript function - GCP",
  app: "test-apps/comprehensive-typescript",
  config: "alien.config.function.ts",
  platform: "gcp",
})
