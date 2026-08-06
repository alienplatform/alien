import { AlienError } from "@alienplatform/core"
import * as z from "zod/v4"
import {
  CommandBootstrapConfigInvalidError,
  CommandBootstrapFailedError,
  MalformedResponseError,
  PlatformHttpError,
} from "./errors.js"
import type { CommandTargetType } from "./protocol.js"
import { parseWireResponse } from "./wire.js"

const DEFAULT_PLATFORM_URL = "https://api.alien.dev"
const BOOTSTRAP_PATH = "/v1/commands/bootstrap"
const REFRESH_SKEW_MS = 30_000

const BootstrapConnectionSchema = z.object({
  managerUrl: z.url(),
  token: z.string().min(1),
  expiresAt: z.iso.datetime({ offset: true }),
})

const CommandTargetSchema = z.object({
  resourceId: z.string().min(1),
  resourceType: z.enum(["container", "daemon"]),
})

const SenderBootstrapResponseSchema = BootstrapConnectionSchema.extend({
  target: z.undefined().optional(),
})

const ReceiverBootstrapResponseSchema = BootstrapConnectionSchema.extend({
  target: CommandTargetSchema,
})

interface CommandConnectionBase {
  managerUrl: string
  token: string
  expiresAt: Date
}

export interface SenderCommandConnection extends CommandConnectionBase {
  role: "sender"
}

export interface ReceiverCommandConnection extends CommandConnectionBase {
  role: "receiver"
  target: {
    resourceId: string
    resourceType: Exclude<CommandTargetType, "worker">
  }
}

export type CommandConnection = SenderCommandConnection | ReceiverCommandConnection

interface HostedConnectionOptionsBase {
  deploymentId: string
  apiKey: string
  platformUrl?: string
  fetch?: typeof fetch
}

type HostedConnectionOptions = HostedConnectionOptionsBase &
  ({ role: "sender"; target?: never } | { role: "receiver"; target: string })

type BootstrapRequest =
  | { deploymentId: string; role: "sender" }
  | { deploymentId: string; role: "receiver"; target: string }

export interface RefreshingConnectionProvider<Connection> {
  get(): Promise<Connection>
  /**
   * Refresh credentials after an authentication failure. Returns `undefined`
   * when this connection is fixed and the request must not be retried.
   */
  refresh(): Promise<Connection | undefined>
}

export type CommandConnectionProvider = RefreshingConnectionProvider<CommandConnection>

/**
 * Execute an authenticated request and retry it once when the server rejects
 * credentials that the provider can refresh.
 */
export async function requestWithRefreshingConnection<
  Connection,
  Result extends { response: Response },
>(
  provider: RefreshingConnectionProvider<Connection>,
  request: (connection: Connection) => Promise<Result>,
): Promise<Result> {
  let result = await request(await provider.get())
  if (result.response.status !== 401) return result

  const refreshed = await provider.refresh()
  if (refreshed === undefined) return result

  result = await request(refreshed)
  return result
}

export class FixedCommandConnectionProvider implements CommandConnectionProvider {
  constructor(private readonly connection: CommandConnection) {}

  get(): Promise<CommandConnection> {
    return Promise.resolve(this.connection)
  }

  refresh(): Promise<undefined> {
    return Promise.resolve(undefined)
  }
}

/**
 * Refreshable hosted-Platform connection. Refreshes before expiration and
 * coalesces concurrent refreshes so one reused client does not stampede the
 * bootstrap endpoint.
 */
export class HostedCommandConnectionProvider implements CommandConnectionProvider {
  private readonly request: BootstrapRequest
  private readonly apiKey: string
  private readonly platformUrl: string
  private readonly fetchImpl: typeof fetch
  private current: CommandConnection | undefined
  private refreshInFlight: Promise<CommandConnection> | undefined

  constructor(options: HostedConnectionOptions) {
    const deploymentId = requireNonEmpty(options.deploymentId, "deploymentId")
    this.apiKey = requireNonEmpty(options.apiKey, "apiKey")
    if (options.role === "sender" && options.target !== undefined) {
      throw invalidConfig("target", "target is only valid for receiver bootstrap")
    }
    this.request =
      options.role === "sender"
        ? { deploymentId, role: "sender" }
        : {
            deploymentId,
            role: "receiver",
            target: requireNonEmpty(options.target, "target"),
          }
    this.platformUrl = validatePlatformUrl(options.platformUrl ?? DEFAULT_PLATFORM_URL)
    this.fetchImpl = options.fetch ?? globalThis.fetch
  }

  async get(): Promise<CommandConnection> {
    if (
      this.current !== undefined &&
      this.current.expiresAt.getTime() - Date.now() > REFRESH_SKEW_MS
    ) {
      return this.current
    }
    return this.refresh()
  }

  async refresh(): Promise<CommandConnection> {
    if (this.refreshInFlight !== undefined) {
      return this.refreshInFlight
    }

    const refresh = this.bootstrap()
    this.refreshInFlight = refresh
    try {
      const connection = await refresh
      this.current = connection
      return connection
    } finally {
      if (this.refreshInFlight === refresh) {
        this.refreshInFlight = undefined
      }
    }
  }

  private async bootstrap(): Promise<CommandConnection> {
    const url = buildBootstrapUrl(this.platformUrl)

    let response: Response
    try {
      response = await this.fetchImpl(url, {
        method: "POST",
        headers: {
          Authorization: `Bearer ${this.apiKey}`,
          "Content-Type": "application/json",
        },
        body: JSON.stringify(this.request),
      })
    } catch (error) {
      throw (await AlienError.from(error)).withContext(
        CommandBootstrapFailedError.create({
          deploymentId: this.request.deploymentId,
          role: this.request.role,
          reason: error instanceof Error ? error.message : String(error),
        }),
      )
    }

    if (!response.ok) {
      const context = {
        method: "POST",
        url,
        status: response.status,
        statusText: response.statusText,
      }
      const definition = PlatformHttpError.create(context)
      throw new AlienError({
        code: definition.metadata.code,
        message: definition.metadata.message(context),
        retryable: response.status === 408 || response.status === 429 || response.status >= 500,
        internal: definition.metadata.internal,
        httpStatusCode: definition.metadata.httpStatusCode,
        context,
      })
    }

    let value: unknown
    try {
      value = await response.json()
    } catch (error) {
      throw new AlienError(
        MalformedResponseError.create({
          method: "POST",
          url,
          reason: error instanceof Error ? error.message : String(error),
        }),
      )
    }
    if (this.request.role === "sender") {
      const parsed = parseWireResponse(SenderBootstrapResponseSchema, value, "POST", url)
      return {
        role: "sender",
        managerUrl: parsed.managerUrl,
        token: parsed.token,
        expiresAt: new Date(parsed.expiresAt),
      }
    }

    const parsed = parseWireResponse(ReceiverBootstrapResponseSchema, value, "POST", url)
    return {
      role: "receiver",
      managerUrl: parsed.managerUrl,
      token: parsed.token,
      expiresAt: new Date(parsed.expiresAt),
      target: parsed.target,
    }
  }
}

export function managerCommandsUrl(managerUrl: string): string {
  let url: URL
  try {
    url = new URL(managerUrl)
  } catch {
    throw invalidConfig("managerUrl", `managerUrl is not a valid URL: ${managerUrl}`)
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw invalidConfig("managerUrl", "managerUrl must use HTTP or HTTPS")
  }
  const basePath = url.pathname.replace(/\/+$/, "")
  url.pathname = `${basePath}/v1`
  return url.toString()
}

function buildBootstrapUrl(platformUrl: string): string {
  const url = new URL(platformUrl)
  const basePath = url.pathname.replace(/\/+$/, "")
  url.pathname = `${basePath}${BOOTSTRAP_PATH}`
  return url.toString()
}

function validatePlatformUrl(value: string): string {
  let url: URL
  try {
    url = new URL(value)
  } catch {
    throw invalidConfig("platformUrl", `platformUrl is not a valid URL: ${value}`)
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw invalidConfig("platformUrl", "platformUrl must use HTTP or HTTPS")
  }
  return value
}

function requireNonEmpty(value: string | undefined, field: string): string {
  if (value === undefined || value.trim() === "") {
    throw invalidConfig(field, `${field} must not be empty`)
  }
  return value
}

function invalidConfig(field: string, reason: string) {
  return new AlienError(CommandBootstrapConfigInvalidError.create({ field, reason }))
}
