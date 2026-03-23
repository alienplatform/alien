import { defineDeploymentSuite } from "../../harness/suite.js"

defineDeploymentSuite({
  name: "TypeScript container - Local",
  app: "test-apps/comprehensive-typescript",
  config: "alien.container.ts",
  platform: "local",
})
