/**
 * Deployment — handle to a deployed application running via `alien dev`
 */

import { CommandsClient } from "@aliendotdev/commands-client"
import type { ChildProcess } from "node:child_process"
import type { Platform, PlatformCredentials } from "./types.js"

/**
 * Deployment class — represents a running deployment managed by an `alien dev` process
 */
export class Deployment {
  readonly id: string
  readonly name: string
  readonly url: string
  readonly platform: Platform

  private process: ChildProcess
  private commandsUrl: string
  private credentials?: PlatformCredentials
  private appPath?: string

  constructor(params: {
    id: string
    name: string
    url: string
    platform: Platform
    commandsUrl: string
    process: ChildProcess
    credentials?: PlatformCredentials
    appPath?: string
  }) {
    this.id = params.id
    this.name = params.name
    this.url = params.url
    this.platform = params.platform
    this.commandsUrl = params.commandsUrl
    this.process = params.process
    this.credentials = params.credentials
    this.appPath = params.appPath
  }

  /**
   * Invoke a command on the deployment
   */
  async invokeCommand(name: string, params: any): Promise<any> {
    const arc = new CommandsClient({
      managerUrl: this.commandsUrl,
      deploymentId: this.id,
      token: "", // dev mode doesn't require auth
      allowLocalStorage: this.platform === "local",
    })

    return arc.invoke(name, params)
  }

  /**
   * Set an external secret using platform-native tools
   */
  async setExternalSecret(vaultName: string, secretKey: string, secretValue: string): Promise<void> {
    const { setExternalSecret } = await import("./external-secrets.js")
    const stateDir = this.appPath ? `${this.appPath}/.alien` : undefined
    await setExternalSecret(
      this.platform,
      this.name, // resourcePrefix
      vaultName,
      secretKey,
      secretValue,
      this.credentials,
      undefined, // namespace
      stateDir,
      this.name,
    )
  }

  /**
   * Destroy the deployment by killing the `alien dev` process.
   *
   * Sends SIGTERM first, then SIGKILL after 5s if still alive.
   * When `alien dev` exits it cleans up resources.
   */
  async destroy(): Promise<void> {
    if (this.process.killed) return

    this.process.kill("SIGTERM")

    await new Promise<void>((resolve) => {
      const timeout = setTimeout(() => {
        if (!this.process.killed) {
          this.process.kill("SIGKILL")
        }
        resolve()
      }, 5000)

      this.process.once("exit", () => {
        clearTimeout(timeout)
        resolve()
      })
    })
  }
}
