/**
 * deploy() — single entry point for deploying an Alien application for testing
 *
 * Auto-detects the deployment method based on the target platform:
 * - local (default): spawns `alien dev` and watches its status file contract
 * - aws / gcp / azure: builds + creates release/deployment via platform API — reads ALIEN_API_KEY from env
 */

import { execFile, spawn } from "node:child_process"
import { existsSync, readFileSync, rmSync } from "node:fs"
import { dirname, join, resolve } from "node:path"
import { promisify } from "node:util"
import { AlienError } from "@alienplatform/core"
import type { DevStatus } from "@alienplatform/core"
import getPort from "get-port"
import { Deployment } from "./deployment.js"
import { TestingOperationFailedError, withTestingContext } from "./errors.js"
import type { DeployOptions } from "./types.js"

const execFileAsync = promisify(execFile)

type DevStatusAgent = DevStatus["agents"][string] & {
  commandsUrl?: string | null
}

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
 * Deploy an Alien application for testing.
 *
 * Uses local dev mode by default. Set `platform` to a cloud provider
 * to deploy via the platform API (requires ALIEN_API_KEY env var).
 */
export async function deploy(options: DeployOptions): Promise<Deployment> {
  const platform = options.platform ?? "local"

  if (platform === "local") {
    return deployViaDev(options)
  }

  // Cloud platforms: aws, gcp, azure
  return deployViaApi(options)
}

// ---------------------------------------------------------------------------
// Method: dev — spawns `alien dev` as a child process
// ---------------------------------------------------------------------------

async function deployViaDev(options: DeployOptions): Promise<Deployment> {
  const verbose = options.verbose ?? process.env.VERBOSE === "true"
  const port = await getPort()
  const cliPath = getAlienCliPath(options.app)
  const statusFile = join(options.app, ".alien", "testing-dev-status.json")

  const args = ["dev", "--port", String(port), "--status-file", statusFile]

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
  const alienStateDir = join(options.app, ".alien")
  rmSync(alienStateDir, { recursive: true, force: true })

  const childEnv: Record<string, string> = {
    ...(process.env as Record<string, string>),
  }

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
  proc.on("exit", code => {
    exited = true
    exitCode = code
  })
  proc.on("error", err => {
    exited = true
    exitCode = 1
    stderr += `\nFailed to spawn alien CLI: ${err.message}`
  })

  try {
    const status = await waitForDevStatusReady(
      statusFile,
      () => exited,
      () => exitCode,
      () => stderr,
    )

    const agent = findPrimaryAgent(status)

    const publicUrl = findPublicUrl(agent.resources)
    if (!publicUrl) {
      throw new AlienError(
        TestingOperationFailedError.create({
          operation: "resolve-public-url",
          message: "No public URL found in deployment resources",
          details: { resources: agent.resources },
        }),
      )
    }

    if (verbose) {
      console.log(`[testing] Public URL: ${publicUrl}`)
      if (agent.commandsUrl) {
        console.log(`[testing] Commands URL: ${agent.commandsUrl}`)
      }
    }

    if (!agent.commandsUrl) {
      throw new AlienError(
        TestingOperationFailedError.create({
          operation: "resolve-commands-url",
          message: "alien dev status file did not include a commands URL",
          details: { agent },
        }),
      )
    }

    return new Deployment({
      id: agent.id,
      name: agent.name,
      url: publicUrl,
      platform: options.platform ?? "local",
      commandsUrl: agent.commandsUrl,
      process: proc,
      appPath: options.app,
    })
  } catch (error) {
    proc.kill("SIGTERM")
    throw await withTestingContext(
      error,
      "deploy",
      "Failed while waiting for alien dev to become ready",
      { statusFile, appPath: options.app, platform: options.platform ?? "local" },
    )
  }
}

// ---------------------------------------------------------------------------
// Method: api — direct platform API calls (reads ALIEN_API_KEY from env)
// ---------------------------------------------------------------------------

interface PlatformDeploymentResponse {
  id: string
  name: string
  status: string
  releaseId: string
  url?: string
  commandsUrl?: string
}

interface PlatformReleaseResponse {
  id: string
  version: number
}

async function deployViaApi(options: DeployOptions): Promise<Deployment> {
  const platform = options.platform ?? "local"
  const verbose = options.verbose ?? process.env.VERBOSE === "true"

  const apiKey = process.env.ALIEN_API_KEY
  if (!apiKey) {
    throw new Error(
      `Cloud deployment (platform: '${platform}') requires the ALIEN_API_KEY environment variable to be set`,
    )
  }

  const apiUrl = process.env.ALIEN_API_URL ?? "https://api.alien.dev"

  const headers = {
    Authorization: `Bearer ${apiKey}`,
    "Content-Type": "application/json",
  }

  // 1. Build the application
  if (verbose) console.log("[testing:api] Building application...")
  const cliPath = getAlienCliPath(options.app)
  const buildArgs = ["build", "--platform", platform]
  if (options.config) {
    buildArgs.push("--config", options.config)
  }
  await execFileAsync(cliPath, buildArgs, { cwd: options.app })

  // 2. Read the built stack
  const stackPath = resolve(options.app, ".alien", "stack.json")
  if (!existsSync(stackPath)) {
    throw new Error(`Build did not produce stack.json at ${stackPath}`)
  }
  const stack = JSON.parse(readFileSync(stackPath, "utf-8"))

  // 3. Create a release
  if (verbose) console.log("[testing:api] Creating release...")
  const releaseResp = await fetch(`${apiUrl}/v1/releases`, {
    method: "POST",
    headers,
    body: JSON.stringify({ stack }),
  })

  if (!releaseResp.ok) {
    const body = await releaseResp.text()
    throw new Error(`Failed to create release: ${releaseResp.status} ${body}`)
  }

  const release = (await releaseResp.json()) as PlatformReleaseResponse

  // 4. Create a deployment
  if (verbose) console.log("[testing:api] Creating deployment...")
  const deploymentName = `e2e-${Date.now()}`
  const deployBody: Record<string, unknown> = {
    releaseId: release.id,
    platform,
    name: deploymentName,
  }

  if (options.environmentVariables?.length) {
    deployBody.environmentVariables = options.environmentVariables
  }

  const deployResp = await fetch(`${apiUrl}/v1/deployments`, {
    method: "POST",
    headers,
    body: JSON.stringify(deployBody),
  })

  if (!deployResp.ok) {
    const body = await deployResp.text()
    throw new Error(`Failed to create deployment: ${deployResp.status} ${body}`)
  }

  const deployment = (await deployResp.json()) as PlatformDeploymentResponse

  // 5. Poll until running
  if (verbose) console.log(`[testing:api] Waiting for deployment ${deployment.id} to be running...`)
  const running = await waitForPlatformDeploymentRunning(apiUrl, apiKey, deployment.id, verbose)

  return new Deployment({
    id: running.id,
    name: running.name,
    url: running.url!,
    platform,
    commandsUrl: running.commandsUrl!,
    appPath: options.app,
    apiUrl,
    apiKey,
  })
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/**
 * Wait for `alien dev` to report a ready local session via its status file.
 */
async function waitForDevStatusReady(
  statusFile: string,
  hasExited: () => boolean,
  getExitCode: () => number | null,
  getStderr: () => string,
): Promise<DevStatus> {
  const timeout = 900_000
  const pollInterval = 500
  const start = Date.now()

  while (Date.now() - start < timeout) {
    if (hasExited()) {
      throw new AlienError(
        TestingOperationFailedError.create({
          operation: "wait-for-ready",
          message: "alien dev exited before the local session became ready",
          details: {
            exitCode: getExitCode(),
            stderrTail: getStderr().slice(-1000),
          },
        }),
      )
    }

    if (!existsSync(statusFile)) {
      await new Promise(r => setTimeout(r, pollInterval))
      continue
    }

    try {
      const status = JSON.parse(readFileSync(statusFile, "utf-8")) as DevStatus
      if (status.status === "error") {
        throw new AlienError(
          TestingOperationFailedError.create({
            operation: "wait-for-ready",
            message: "alien dev reported an error status",
            details: { status },
          }),
        )
      }

      if (status.status === "ready" && Object.keys(status.agents ?? {}).length > 0) {
        return status
      }
    } catch (error) {
      if (error instanceof AlienError && error.code === "TESTING_OPERATION_FAILED") {
        throw error
      }
      // Status file may be mid-write; retry.
    }

    await new Promise(r => setTimeout(r, pollInterval))
  }

  throw new AlienError(
    TestingOperationFailedError.create({
      operation: "wait-for-ready",
      message: `Timeout waiting for alien dev to report readiness (${timeout / 1000}s)`,
      details: { statusFile, timeoutMs: timeout },
    }),
  )
}

function findPrimaryAgent(status: DevStatus): DevStatusAgent {
  const agent = Object.values(status.agents ?? {})[0] as DevStatusAgent | undefined
  if (!agent) {
    throw new AlienError(
      TestingOperationFailedError.create({
        operation: "resolve-agent",
        message: "alien dev reported readiness but no agents were present in the status file",
        details: { status },
      }),
    )
  }

  return agent
}

/**
 * Wait for a platform API deployment to reach "running" status.
 */
async function waitForPlatformDeploymentRunning(
  apiUrl: string,
  apiKey: string,
  deploymentId: string,
  verbose: boolean,
): Promise<PlatformDeploymentResponse> {
  const timeout = 900_000 // 15 minutes
  const pollInterval = 5000
  const start = Date.now()
  const headers = { Authorization: `Bearer ${apiKey}` }

  while (Date.now() - start < timeout) {
    try {
      const resp = await fetch(`${apiUrl}/v1/deployments/${deploymentId}`, {
        headers,
        signal: AbortSignal.timeout(10000),
      })

      if (resp.ok) {
        const data = (await resp.json()) as PlatformDeploymentResponse
        if (verbose) {
          console.log(`[testing] Deployment ${deploymentId} status: ${data.status}`)
        }

        if (data.status === "running") {
          if (!data.url) {
            throw new Error(`Deployment is running but has no URL: ${JSON.stringify(data)}`)
          }
          return data
        }

        if (data.status === "error" || data.status.includes("failed")) {
          throw new Error(`Deployment failed with status: ${data.status}`)
        }
      }
    } catch (error) {
      if (error instanceof Error && error.message.includes("failed with status")) {
        throw error
      }
      // Network errors expected during provisioning
    }

    await new Promise(r => setTimeout(r, pollInterval))
  }

  throw new Error(
    `Timeout waiting for deployment ${deploymentId} to reach running status (${timeout / 1000}s)`,
  )
}

/**
 * Find the public URL from deployment resources
 */
function findPublicUrl(
  resources: Record<string, { url: string; resourceType?: string | null }>,
): string | undefined {
  for (const [name, resource] of Object.entries(resources)) {
    if (
      resource.url &&
      (name.includes("router") || name.includes("gateway") || name.includes("proxy"))
    ) {
      return resource.url
    }
  }

  const publicResources = Object.entries(resources).filter(
    ([_, r]) => (r.resourceType === "container" || r.resourceType === "function") && r.url,
  )
  if (publicResources.length > 0) {
    return publicResources[publicResources.length - 1]![1].url
  }

  return undefined
}
