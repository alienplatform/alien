/**
 * Deployment — handle to a deployed application for testing
 *
 * Supports two deployment modes:
 * - Dev mode: manages a child `alien dev` process (local platform)
 * - API mode: manages a deployment via platform API (cloud platforms)
 */

import { execFile } from "node:child_process"
import type { ChildProcess } from "node:child_process"
import { readFileSync } from "node:fs"
import { resolve } from "node:path"
import { promisify } from "node:util"
import { CommandsClient } from "@alienplatform/sdk/commands"
import type { DeploymentInit, Platform, UpgradeOptions } from "./types.js"

const execFileAsync = promisify(execFile)

export class Deployment {
  readonly id: string
  readonly name: string
  readonly url: string
  readonly platform: Platform

  /** Whether the deployment has been destroyed */
  destroyed = false

  private process?: ChildProcess
  private commandsUrl: string
  private appPath: string

  // API mode fields
  private apiUrl?: string
  private apiKey?: string

  constructor(params: DeploymentInit) {
    this.id = params.id
    this.name = params.name
    this.url = params.url
    this.platform = params.platform
    this.commandsUrl = params.commandsUrl
    this.process = params.process
    this.appPath = params.appPath
    this.apiUrl = params.apiUrl
    this.apiKey = params.apiKey
  }

  /**
   * Invoke a command on the deployment
   */
  async invokeCommand(name: string, params: any): Promise<any> {
    const token = this.apiKey ?? ""
    const arc = new CommandsClient({
      managerUrl: this.commandsUrl,
      deploymentId: this.id,
      token,
      allowLocalStorage: this.platform === "local",
    })

    return arc.invoke(name, params)
  }

  /**
   * Set an external secret using platform-native tools
   */
  async setExternalSecret(
    vaultName: string,
    secretKey: string,
    secretValue: string,
  ): Promise<void> {
    const { setExternalSecret } = await import("./external-secrets.js")
    const stateDir = this.appPath ? `${this.appPath}/.alien` : undefined
    await setExternalSecret(
      this.platform,
      this.name, // resourcePrefix (cloud: naming prefix, local: unused)
      vaultName,
      secretKey,
      secretValue,
      undefined, // namespace
      stateDir,
      this.id, // deploymentId (local: used for vault path)
    )
  }

  /**
   * Upgrade the deployment by creating a new release and updating the deployment.
   * Only works for API-mode deployments (cloud platforms).
   */
  async upgrade(options: UpgradeOptions = {}): Promise<void> {
    if (!this.apiUrl || !this.apiKey) {
      throw new Error("upgrade() requires a cloud deployment (not local dev mode)")
    }

    const headers = {
      Authorization: `Bearer ${this.apiKey}`,
      "Content-Type": "application/json",
    }

    // Build new artifacts
    const cliPath = this.resolveCliPath()
    const buildArgs = ["build", "--platform", this.platform]
    await execFileAsync(cliPath, buildArgs, { cwd: this.appPath })

    // Read the built stack
    const stackPath = resolve(this.appPath, ".alien", "stack.json")
    const stack = JSON.parse(readFileSync(stackPath, "utf-8"))

    // Create a new release
    const releaseResp = await fetch(`${this.apiUrl}/v1/releases`, {
      method: "POST",
      headers,
      body: JSON.stringify({ stack }),
    })

    if (!releaseResp.ok) {
      const body = await releaseResp.text()
      throw new Error(`Failed to create release for upgrade: ${releaseResp.status} ${body}`)
    }

    const release = (await releaseResp.json()) as { id: string }

    // Update the deployment with the new release and optional env vars
    const patchBody: Record<string, unknown> = { releaseId: release.id }
    if (options.environmentVariables?.length) {
      patchBody.environmentVariables = options.environmentVariables
    }

    const patchResp = await fetch(`${this.apiUrl}/v1/deployments/${this.id}`, {
      method: "PATCH",
      headers,
      body: JSON.stringify(patchBody),
    })

    if (!patchResp.ok) {
      const body = await patchResp.text()
      throw new Error(`Failed to update deployment for upgrade: ${patchResp.status} ${body}`)
    }

    // Wait for deployment to pick up the new release
    const timeout = 300_000 // 5 minutes
    const start = Date.now()
    while (Date.now() - start < timeout) {
      const resp = await fetch(`${this.apiUrl}/v1/deployments/${this.id}`, {
        headers: { Authorization: `Bearer ${this.apiKey}` },
      })
      if (resp.ok) {
        const data = (await resp.json()) as { status: string; releaseId: string }
        if (data.releaseId === release.id && data.status === "running") {
          return
        }
        if (data.status === "error" || data.status.includes("failed")) {
          throw new Error(`Deployment failed during upgrade with status: ${data.status}`)
        }
      }
      await new Promise(r => setTimeout(r, 5000))
    }

    throw new Error("Timeout waiting for deployment to pick up upgrade")
  }

  /**
   * Destroy the deployment.
   *
   * For dev mode: kills the `alien dev` process.
   * For API mode: calls DELETE on platform API.
   */
  async destroy(): Promise<void> {
    if (this.destroyed) return

    // Dev mode — kill the process
    if (this.process) {
      if (!this.process.killed) {
        this.process.kill("SIGTERM")

        await new Promise<void>(resolve => {
          const timeout = setTimeout(() => {
            if (!this.process!.killed) {
              this.process!.kill("SIGKILL")
            }
            resolve()
          }, 5000)

          this.process!.once("exit", () => {
            clearTimeout(timeout)
            resolve()
          })
        })
      }
      this.destroyed = true
      return
    }

    // API mode — delete via platform API
    if (this.apiUrl && this.apiKey) {
      const headers = { Authorization: `Bearer ${this.apiKey}` }

      // Delete via platform API
      const resp = await fetch(`${this.apiUrl}/v1/deployments/${this.id}`, {
        method: "DELETE",
        headers,
      })

      if (!resp.ok && resp.status !== 404) {
        const body = await resp.text()
        throw new Error(`Failed to destroy deployment: ${resp.status} ${body}`)
      }

      // Poll until deployment is gone or destroyed
      const timeout = 300_000 // 5 minutes
      const start = Date.now()
      while (Date.now() - start < timeout) {
        const checkResp = await fetch(`${this.apiUrl}/v1/deployments/${this.id}`, { headers })
        if (checkResp.status === 404) break
        if (checkResp.ok) {
          const data = (await checkResp.json()) as { status: string }
          if (data.status === "destroyed" || data.status === "deleted") break
        }
        await new Promise(r => setTimeout(r, 5000))
      }
    }

    this.destroyed = true
  }

  private resolveCliPath(): string {
    const raw = process.env.ALIEN_CLI_PATH?.trim()
    if (raw) {
      return raw.includes("/") || raw.includes("\\") ? resolve(raw) : raw
    }
    return "alien"
  }
}
