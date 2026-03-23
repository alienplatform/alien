import { defineDeploymentSuite } from "../../harness/suite.js"

defineDeploymentSuite({
  name: "TypeScript container - GCP",
  app: "test-apps/comprehensive-typescript",
  config: "alien.container.ts",
  platform: "gcp",
})
