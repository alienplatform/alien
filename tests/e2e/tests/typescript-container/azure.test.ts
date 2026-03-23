import { defineDeploymentSuite } from "../../harness/suite.js"

defineDeploymentSuite({
  name: "TypeScript container - Azure",
  app: "test-apps/comprehensive-typescript",
  config: "alien.container.ts",
  platform: "azure",
})
