import { defineDeploymentSuite } from "../../harness/suite.js"

defineDeploymentSuite({
  name: "TypeScript container - Kubernetes",
  app: "test-apps/comprehensive-typescript",
  config: "alien.container.ts",
  platform: "kubernetes",
})
