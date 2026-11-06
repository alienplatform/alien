import { AlienError } from "@alienplatform/core"
import * as z from "zod/v4"
import { managerCommandsUrl } from "./bootstrap.js"
import { CommandBootstrapConfigInvalidError, PlatformHttpError } from "./errors.js"

export interface OperationConnectionOptions {
  deploymentId: string
  project: string
  workspace: string
  plugin: string
  operation: string
  apiKey: string
  platformUrl?: string
  fetch?: typeof fetch
  /** Explicit consent to read output whose frozen policy requires confirmation. */
  confirmSensitiveOutput?: boolean
}

const ConnectionSchema = z.object({
  managerUrl: z.url(),
  token: z.string().min(1),
  command: z.string().min(1),
  targetResourceId: z.string().min(1),
  deadline: z.iso.datetime({ offset: true }),
  operationResultContract: z.unknown(),
})
export type OperationConnection = z.infer<typeof ConnectionSchema>

function invalid(reason: string) {
  return new AlienError(CommandBootstrapConfigInvalidError.create({ field: "operation", reason }))
}

export class OperationConnections {
  private readonly fetchImpl: typeof fetch
  constructor(private readonly options: OperationConnectionOptions) {
    this.fetchImpl = options.fetch ?? globalThis.fetch
  }

  private async metadata(path: string, body: unknown): Promise<unknown> {
    const url = new URL(this.options.platformUrl ?? "https://api.alien.dev")
    if (url.username || url.password || url.search || url.hash) throw invalid("Invalid API origin")
    if (
      url.protocol !== "https:" &&
      !(url.protocol === "http:" && ["localhost", "127.0.0.1", "[::1]"].includes(url.hostname))
    )
      throw invalid("API origin requires HTTPS")
    url.pathname = `${url.pathname.replace(/\/+$/, "")}${path}`
    url.searchParams.set("workspace", this.options.workspace)
    url.searchParams.set("project", this.options.project)
    const response = await this.fetchImpl(url, {
      method: "POST",
      redirect: "error",
      signal: AbortSignal.timeout(30_000),
      headers: {
        Authorization: `Bearer ${this.options.apiKey}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(body),
    })
    if (!response.ok)
      throw new AlienError(
        PlatformHttpError.create({
          method: "POST",
          url: url.toString(),
          status: response.status,
          statusText: response.statusText,
        }),
      )
    return response.json()
  }

  async prepare(): Promise<OperationConnection> {
    const result = z
      .object({
        status: z.enum(["prepared", "pending-approval"]),
        connection: ConnectionSchema.optional(),
      })
      .parse(
        await this.metadata("/v1/operations/invoke", {
          deploymentId: this.options.deploymentId,
          plugin: this.options.plugin,
          operation: this.options.operation,
          delivery: "direct",
        }),
      )
    if (result.status !== "prepared" || !result.connection)
      throw invalid("This operation requires approval; no inputs were sent or operation dispatched")
    managerCommandsUrl(result.connection.managerUrl)
    return result.connection
  }

  async status(commandId: string, selected: OperationConnection) {
    const result = z
      .object({ managerUrl: z.url(), token: z.string().min(1), commandId: z.literal(commandId) })
      .parse(
        await this.metadata(`/v1/commands/${encodeURIComponent(commandId)}/connection`, {
          confirmSensitiveOutput: this.options.confirmSensitiveOutput ?? false,
        }),
      )
    if (managerCommandsUrl(result.managerUrl) !== managerCommandsUrl(selected.managerUrl))
      throw invalid(
        "The selected instance changed; result retrieval requires a new explicit connection",
      )
    return result
  }
}
