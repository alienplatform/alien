import { defineDeploymentSuite } from "../../harness/suite.js"

defineDeploymentSuite({
  name: "TypeScript function - AWS",
  app: "test-apps/comprehensive-typescript",
  config: "alien.function.ts",
  platform: "aws",
})
