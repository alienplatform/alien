import { defineDeploymentSuite } from "../../harness/suite.js"

defineDeploymentSuite({
  name: "TypeScript function - Azure",
  app: "test-apps/comprehensive-typescript",
  config: "alien.function.ts",
  platform: "azure",
})
