/**
 * Shared test suite definition.
 *
 * Each test file calls `defineDeploymentSuite()` with its specific parameters.
 * The suite deploys the app once and runs all check functions against it.
 */

import path from "node:path"
import { fileURLToPath } from "node:url"
import { deploy, type Deployment, type Platform } from "@aliendotdev/testing"
import { describe, it, beforeAll, afterAll } from "vitest"
import * as checks from "../checks/index.js"
import { getCredentials, getE2EConfig } from "./config.js"

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const e2eRoot = path.resolve(__dirname, "..")

export interface SuiteOptions {
  name: string
  app: string
  /** Config file for cloud deployments */
  config: string
  /** Config file for local dev (defaults to alien.config.dev.ts) */
  devConfig?: string
  platform: Platform
}

export function defineDeploymentSuite(options: SuiteOptions) {
  const e2eConfig = getE2EConfig()
  const isLocal = options.platform === "local"

  describe(options.name, () => {
    let deployment: Deployment

    beforeAll(async () => {
      const credentials = getCredentials(options.platform)
      const appPath = path.resolve(e2eRoot, options.app)
      const configFile = isLocal
        ? (options.devConfig || "alien.config.dev.ts")
        : options.config

      deployment = await deploy({
        app: appPath,
        config: configFile,
        platform: options.platform,
        credentials,
        verbose: e2eConfig.verbose,
      })
    }, 900_000)

    afterAll(async () => {
      if (deployment && !e2eConfig.skipCleanup) {
        await deployment.destroy()
      }
    })

    // Health checks
    it("health check", () => checks.checkHealth(deployment))
    it("hello endpoint", () => checks.checkHello(deployment))

    // Binding checks
    it("storage binding", () => checks.checkStorage(deployment))
    it("kv binding", () => checks.checkKV(deployment))
    it("vault binding", () => checks.checkVault(deployment))

    // Queue (not available locally)
    if (!isLocal) {
      it("queue binding", () => checks.checkQueue(deployment))
    }

    // External secrets (only on cloud platforms)
    if (!isLocal) {
      it("external secrets", () => checks.checkExternalSecret(deployment), 30_000)
    }

    // Runtime features
    it("SSE", () => checks.checkSSE(deployment))
    it("environment variables", () => checks.checkEnvironmentVariable(deployment))
    it("request inspection", () => checks.checkInspect(deployment))

    // wait_until requires storage binding
    it("wait_until background tasks", () => checks.checkWaitUntil(deployment), 60_000)

    // Commands
    it("command echo", () => checks.checkCommandEcho(deployment))
    it("command small payload", () => checks.checkCommandSmallPayload(deployment))

    // Large payload command test - skip on local (local dev mode has response size limitations)
    if (!isLocal) {
      it("command large payload", () => checks.checkCommandLargePayload(deployment))
    }

    // Event handlers
    if (!isLocal) {
      it("event handlers registered", () => checks.checkStorageEventHandler(deployment))
    }
  })
}
