import { defineDeploymentSuite } from "../../harness/suite.js"

defineDeploymentSuite({
  name: "TypeScript function - Kubernetes",
  app: "test-apps/comprehensive-typescript",
  config: "alien.config.function.ts",
  platform: "kubernetes",
})
