/**
 * deploy() — single entry point for deploying an Alien application for testing
 *
 * Spawns `alien dev` as a child process with --no-tui. The CLI handles building,
 * creating a release, creating a deployment, and provisioning resources.
 */

import { spawn } from "node:child_process"
import { existsSync, rmSync } from "node:fs"
import { dirname, join, resolve } from "node:path"
import getPort from "get-port"
import { AlienError } from "@aliendotdev/core"
import { Deployment } from "./deployment.js"
import { TestingOperationFailedError, withTestingContext } from "./errors.js"
import type { DeployOptions, DeploymentInfo } from "./types.js"

/**
 * Get the alien CLI path with fallback discovery.
 *
 * Resolution order:
 * 1. ALIEN_CLI_PATH (if set)
 * 2. nearest ../target/debug/alien (or alien.exe) walking upward from app path
 * 3. "alien" from PATH
 */
function getAlienCliPath(appPath: string): string {
  const raw = process.env.ALIEN_CLI_PATH?.trim()
  if (raw) {
    // If the path contains a directory separator it's a file path — resolve it
    // so spawn doesn't interpret it relative to the child's cwd.
    if (raw.includes("/") || raw.includes("\\")) {
      return resolve(raw)
    }
    return raw
  }

  let current = resolve(appPath)
  while (true) {
    const unixCandidate = join(current, "target", "debug", "alien")
    if (existsSync(unixCandidate)) {
      return unixCandidate
    }
    const windowsCandidate = `${unixCandidate}.exe`
    if (existsSync(windowsCandidate)) {
      return windowsCandidate
    }

    const parent = dirname(current)
    if (parent === current) {
      break
    }
    current = parent
  }

  return "alien"
}

/**
 * Deploy an Alien application for testing
 *
 * Spawns `alien dev --no-tui` as a child process, waits for the deployment
 * to reach "running" status, then returns a Deployment handle.
 */
export async function deploy(options: DeployOptions): Promise<Deployment> {
  const verbose = options.verbose ?? process.env.VERBOSE === "true"
  const port = await getPort()
  const cliPath = getAlienCliPath(options.app)

  // Build args: alien dev --platform <p> --no-tui --port <port> [--config <f>] [--env K=V]... [--secret K=V]...
  const args = [
    "dev",
    "--platform", options.platform,
    "--no-tui",
    "--port", String(port),
  ]

  if (options.config) {
    args.push("--config", options.config)
  }

  for (const ev of options.environmentVariables ?? []) {
    const flag = ev.type === "secret" ? "--secret" : "--env"
    const targets = ev.targetResources?.length ? `:${ev.targetResources.join(",")}` : ""
    args.push(flag, `${ev.name}=${ev.value}${targets}`)
  }

  if (verbose) {
    console.log(`[testing] Spawning: ${cliPath} ${args.join(" ")}`)
  }

  // Clean .alien state directory to ensure fresh deployment state.
  // Without this, a stale dev-server.db from a previous run can cause the CLI
  // to reuse an old deployment whose ports are no longer live.
  const alienStateDir = join(options.app, ".alien")
  rmSync(alienStateDir, { recursive: true, force: true })

  // Build environment for the child process
  const childEnv: Record<string, string> = {
    ...(process.env as Record<string, string>),
  }

  // Pass platform credentials as env vars if provided
  if (options.credentials) {
    switch (options.credentials.platform) {
      case "aws":
        childEnv.AWS_ACCESS_KEY_ID = options.credentials.accessKeyId
        childEnv.AWS_SECRET_ACCESS_KEY = options.credentials.secretAccessKey
        childEnv.AWS_REGION = options.credentials.region
        if (options.credentials.sessionToken) {
          childEnv.AWS_SESSION_TOKEN = options.credentials.sessionToken
        }
        break
      case "gcp":
        childEnv.GCP_PROJECT_ID = options.credentials.projectId
        childEnv.GOOGLE_CLOUD_REGION = options.credentials.region
        if (options.credentials.serviceAccountKeyPath) {
          childEnv.GOOGLE_APPLICATION_CREDENTIALS = options.credentials.serviceAccountKeyPath
        }
        break
      case "azure":
        childEnv.AZURE_SUBSCRIPTION_ID = options.credentials.subscriptionId
        childEnv.AZURE_TENANT_ID = options.credentials.tenantId
        childEnv.AZURE_CLIENT_ID = options.credentials.clientId
        childEnv.AZURE_CLIENT_SECRET = options.credentials.clientSecret
        childEnv.AZURE_REGION = options.credentials.region
        break
      case "kubernetes":
        if (options.credentials.kubeconfigPath) {
          childEnv.KUBECONFIG = options.credentials.kubeconfigPath
        }
        break
    }
  }

  // Spawn alien dev
  const proc = spawn(cliPath, args, {
    cwd: options.app,
    env: childEnv,
    stdio: ["ignore", "pipe", "pipe"],
  })

  let stdout = ""
  let stderr = ""

  proc.stdout?.on("data", (data: Buffer) => {
    stdout += data.toString()
    if (verbose) {
      process.stdout.write(data)
    }
  })

  proc.stderr?.on("data", (data: Buffer) => {
    stderr += data.toString()
    if (verbose) {
      process.stderr.write(data)
    }
  })

  let exited = false
  let exitCode: number | null = null
  proc.on("exit", (code) => {
    exited = true
    exitCode = code
  })
  proc.on("error", (err) => {
    exited = true
    exitCode = 1
    stderr += `\nFailed to spawn alien CLI: ${err.message}`
  })

  const serverUrl = `http://localhost:${port}`

  // Wait for the deployment to reach "running" status
  try {
    const info = await waitForDeploymentRunning(serverUrl, () => exited, () => exitCode, () => stderr)

    const publicUrl = findPublicUrl(info.resources)
    if (!publicUrl) {
      throw new AlienError(
        TestingOperationFailedError.create({
          operation: "resolve-public-url",
          message: "No public URL found in deployment resources",
          details: { resources: info.resources },
        }),
      )
    }

    if (verbose) {
      console.log(`[testing] Public URL: ${publicUrl}`)
      console.log(`[testing] Commands URL: ${info.commands.url}`)
    }

    return new Deployment({
      id: info.commands.deploymentId,
      name: "default",
      url: publicUrl,
      platform: options.platform,
      commandsUrl: info.commands.url,
      process: proc,
      credentials: options.credentials,
      appPath: options.app,
    })
  } catch (error) {
    // Kill the process if we failed to deploy
    proc.kill("SIGTERM")
    throw await withTestingContext(
      error,
      "deploy",
      "Failed while waiting for deployment to become ready",
      { serverUrl, appPath: options.app, platform: options.platform },
    )
  }
}

/**
 * Wait for a deployment to reach "running" status by polling the dev server API.
 */
async function waitForDeploymentRunning(
  serverUrl: string,
  hasExited: () => boolean,
  getExitCode: () => number | null,
  getStderr: () => string,
): Promise<DeploymentInfo> {
  const timeout = 300_000
  const pollInterval = 2000
  const start = Date.now()

  // First wait for the server to be healthy
  while (Date.now() - start < timeout) {
    if (hasExited()) {
      throw new AlienError(
        TestingOperationFailedError.create({
          operation: "wait-for-health",
          message: "alien dev exited before deployment was ready",
          details: {
            exitCode: getExitCode(),
            stderrTail: getStderr().slice(-1000),
          },
        }),
      )
    }

    try {
      const resp = await fetch(`${serverUrl}/health`, { signal: AbortSignal.timeout(1000) })
      if (resp.ok) break
    } catch {
      // Expected while server is starting
    }

    await new Promise((r) => setTimeout(r, 500))
  }

  // Poll deployments until one is running
  while (Date.now() - start < timeout) {
    if (hasExited()) {
      throw new AlienError(
        TestingOperationFailedError.create({
          operation: "wait-for-running",
          message: "alien dev exited before deployment was ready",
          details: {
            exitCode: getExitCode(),
            stderrTail: getStderr().slice(-1000),
          },
        }),
      )
    }

    try {
      const listResp = await fetch(`${serverUrl}/v1/deployments`, {
        signal: AbortSignal.timeout(5000),
      })

      if (listResp.ok) {
        const list = (await listResp.json()) as { items: Array<{ id: string; name: string; status: string }> }
        const deployment = list.items[0]

        if (deployment) {
          if (deployment.status === "error" || deployment.status.includes("failed")) {
            throw new AlienError(
              TestingOperationFailedError.create({
                operation: "wait-for-running",
                message: `Deployment failed with status: ${deployment.status}`,
                details: { deploymentId: deployment.id, deploymentName: deployment.name },
              }),
            )
          }

          // Get deployment info if running
          const infoResp = await fetch(`${serverUrl}/v1/deployments/${deployment.id}/info`, {
            signal: AbortSignal.timeout(5000),
          })

          if (infoResp.ok) {
            const info = (await infoResp.json()) as DeploymentInfo
            if (info.status === "running") {
              return info
            }
          }
        }
      }
    } catch (error) {
      // Rethrow deployment failure errors
      if (error instanceof AlienError && error.code === "TESTING_OPERATION_FAILED") {
        throw error
      }
      // Network errors are expected while things are starting
    }

    await new Promise((r) => setTimeout(r, pollInterval))
  }

  throw new AlienError(
    TestingOperationFailedError.create({
      operation: "wait-for-running",
      message: `Timeout waiting for deployment to reach running status (${timeout / 1000}s)`,
      details: { serverUrl, timeoutMs: timeout },
    }),
  )
}

/**
 * Find the public URL from deployment resources
 */
function findPublicUrl(
  resources: Record<string, { resourceType: string; publicUrl?: string }>,
): string | undefined {
  // Prefer router/gateway/proxy resources
  for (const [name, resource] of Object.entries(resources)) {
    if (resource.publicUrl && (name.includes("router") || name.includes("gateway") || name.includes("proxy"))) {
      return resource.publicUrl
    }
  }

  // Fallback to last resource with publicUrl
  const publicResources = Object.entries(resources)
    .filter(([_, r]) => (r.resourceType === "container" || r.resourceType === "function") && r.publicUrl)
  if (publicResources.length > 0) {
    return publicResources[publicResources.length - 1]![1].publicUrl
  }

  return undefined
}
