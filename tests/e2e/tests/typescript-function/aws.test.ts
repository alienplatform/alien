import { defineDeploymentSuite } from "../../harness/suite.js"

defineDeploymentSuite({
  name: "TypeScript function - AWS",
  app: "test-apps/comprehensive-typescript",
  config: "alien.config.function.ts",
  platform: "aws",
})
