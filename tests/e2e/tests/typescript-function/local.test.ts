import { defineDeploymentSuite } from "../../harness/suite.js"

defineDeploymentSuite({
  name: "TypeScript function - Local",
  app: "test-apps/comprehensive-typescript",
  config: "alien.function.ts",
  platform: "local",
})
