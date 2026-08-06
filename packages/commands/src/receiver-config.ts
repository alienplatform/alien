import { readFile } from "node:fs/promises"
import { AlienError } from "@alienplatform/core"
import {
  type CommandConnection,
  type CommandConnectionProvider,
  HostedCommandConnectionProvider,
  type RefreshingConnectionProvider,
  managerCommandsUrl,
} from "./bootstrap.js"
import { CommandReceiverConfigInvalidError, InvalidEnvelopeError } from "./errors.js"
import type { CommandTargetType } from "./protocol.js"

const DEFAULT_POLL_INTERVAL_MS = 5_000
const DEFAULT_MAX_LEASES = 1
const DEFAULT_LEASE_SECONDS = 60
const DEFAULT_POLL_MAX_INTERVAL_MS = 30_000
const DEFAULT_POLL_JITTER = 0.1
const DEFAULT_DRAIN_TIMEOUT_MS = 30_000

// Env variable names — identical strings to the Rust twin
// (`alien_core::runtime_environment`).
export const ENV_ALIEN_COMMANDS_URL = "ALIEN_COMMANDS_URL"
const ENV_ALIEN_COMMANDS_TOKEN = "ALIEN_COMMANDS_TOKEN"
const ENV_ALIEN_COMMANDS_TOKEN_FILE = "ALIEN_COMMANDS_TOKEN_FILE"
const ENV_ALIEN_DEPLOYMENT_ID = "ALIEN_DEPLOYMENT_ID"
const ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID = "ALIEN_COMMANDS_TARGET_RESOURCE_ID"
const ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE = "ALIEN_COMMANDS_TARGET_RESOURCE_TYPE"
const ENV_ALIEN_COMMANDS_LEASE_SECONDS = "ALIEN_COMMANDS_LEASE_SECONDS"
const ENV_ALIEN_COMMANDS_MAX_LEASES = "ALIEN_COMMANDS_MAX_LEASES"
const ENV_ALIEN_COMMANDS_POLL_INTERVAL_MS = "ALIEN_COMMANDS_POLL_INTERVAL_MS"
const ENV_ALIEN_COMMANDS_POLL_MAX_INTERVAL_MS = "ALIEN_COMMANDS_POLL_MAX_INTERVAL_MS"
const ENV_ALIEN_COMMANDS_POLL_JITTER = "ALIEN_COMMANDS_POLL_JITTER"
const ENV_ALIEN_COMMANDS_DRAIN_TIMEOUT_MS = "ALIEN_COMMANDS_DRAIN_TIMEOUT_MS"

export interface CommandReceiverRuntimeOptions {
  /** `fetch` implementation (defaults to the global `fetch`). */
  fetch?: typeof fetch
  /** Lease poll interval in ms (default 5000). Overrides the environment. */
  pollIntervalMs?: number
  /** Maximum empty/error poll interval in ms (default 30000). */
  pollMaxIntervalMs?: number
  /** Poll jitter fraction from 0 to 1 (default 0.1). */
  pollJitter?: number
  /** Requested lease duration in seconds (default 60). Overrides the environment. */
  leaseSeconds?: number
  /** Max leases requested per poll (default 1). Overrides the environment. */
  maxLeases?: number
  /** Graceful drain timeout in ms (default 30000). */
  drainTimeoutMs?: number
}

/**
 * Injected-runtime options for a command receiver. Identity and credentials
 * come from the `ALIEN_COMMANDS_*` environment variables.
 */
export interface CommandReceiverOptions extends CommandReceiverRuntimeOptions {
  /** Environment source (defaults to `process.env`). */
  env?: Record<string, string | undefined>
}

/** Hosted Alien options for an external command receiver. */
export interface HostedCommandReceiverOptions extends CommandReceiverRuntimeOptions {
  /** Deployment that owns the receiving target. */
  deploymentId: string
  /** Alien API key used to discover the manager and mint receiver-only access. */
  apiKey: string
  /** Command-enabled Container or Daemon name/id to receive for. */
  target: string
  /** Alien Platform API base URL (default: "https://api.alien.dev"). */
  platformUrl?: string
}

interface ReceiverConfig {
  url: string
  token?: string
  tokenFile?: string
  deploymentId: string
  resourceId: string
  resourceType: Exclude<CommandTargetType, "worker">
  pollIntervalMs: number
  pollMaxIntervalMs: number
  pollJitter: number
  leaseSeconds: number
  maxLeases: number
  drainTimeoutMs: number
}

export interface ReceiverRuntimeConfig {
  pollIntervalMs: number
  pollMaxIntervalMs: number
  pollJitter: number
  leaseSeconds: number
  maxLeases: number
  drainTimeoutMs: number
}

export interface ReceiverConnection {
  url: string
  token: string
  deploymentId: string
  resourceId: string
  resourceType: Exclude<CommandTargetType, "worker">
}

export type ReceiverConnectionProvider = RefreshingConnectionProvider<ReceiverConnection>

export function resolveReceiverConfig(
  options: CommandReceiverOptions | HostedCommandReceiverOptions,
): {
  connectionProvider: ReceiverConnectionProvider
  runtimeConfig: ReceiverRuntimeConfig
} {
  if (isHostedOptions(options)) {
    return {
      connectionProvider: new HostedReceiverConnectionProvider(
        new HostedCommandConnectionProvider({
          deploymentId: options.deploymentId,
          apiKey: options.apiKey,
          role: "receiver",
          target: options.target,
          platformUrl: options.platformUrl,
          fetch: options.fetch,
        }),
        options.deploymentId,
      ),
      runtimeConfig: validateRuntimeConfig({}, options),
    }
  }

  const env = options.env ?? (typeof process !== "undefined" ? process.env : {})
  const config = validateConfig(env, options)
  return {
    connectionProvider: new EnvironmentReceiverConnectionProvider(config),
    runtimeConfig: config,
  }
}

function isHostedOptions(
  options: CommandReceiverOptions | HostedCommandReceiverOptions,
): options is HostedCommandReceiverOptions {
  return "deploymentId" in options || "apiKey" in options || "target" in options
}

function requireEnv(env: Record<string, string | undefined>, name: string): string {
  const value = env[name]
  if (value === undefined) {
    throw new AlienError(
      CommandReceiverConfigInvalidError.create({ envVar: name, reason: `${name} is required` }),
    )
  }
  if (value.trim() === "") {
    throw new AlienError(
      CommandReceiverConfigInvalidError.create({
        envVar: name,
        reason: `${name} must not be empty`,
      }),
    )
  }
  return value
}

function optionalNonEmpty(
  env: Record<string, string | undefined>,
  name: string,
): string | undefined {
  const value = env[name]
  if (value?.trim() === "") {
    throw new AlienError(
      CommandReceiverConfigInvalidError.create({
        envVar: name,
        reason: `${name} must not be empty`,
      }),
    )
  }
  return value
}

function numericConfig(
  env: Record<string, string | undefined>,
  envName: string,
  override: number | undefined,
  fallback: number,
  validate: (value: number) => boolean,
): number {
  const raw = override ?? (env[envName] === undefined ? fallback : Number(env[envName]))
  if (!Number.isFinite(raw) || !validate(raw)) {
    throw new AlienError(
      CommandReceiverConfigInvalidError.create({
        envVar: envName,
        reason: `${envName} has invalid numeric value '${env[envName] ?? raw}'`,
      }),
    )
  }
  return raw
}

function validateConfig(
  env: Record<string, string | undefined>,
  options: CommandReceiverOptions,
): ReceiverConfig {
  const url = requireEnv(env, ENV_ALIEN_COMMANDS_URL)
  try {
    // eslint-disable-next-line no-new -- validating parseability only
    new URL(url)
  } catch {
    throw new AlienError(
      CommandReceiverConfigInvalidError.create({
        envVar: ENV_ALIEN_COMMANDS_URL,
        reason: `${ENV_ALIEN_COMMANDS_URL} is not a valid URL: ${url}`,
      }),
    )
  }

  const token = optionalNonEmpty(env, ENV_ALIEN_COMMANDS_TOKEN)
  const tokenFile = optionalNonEmpty(env, ENV_ALIEN_COMMANDS_TOKEN_FILE)
  if (token === undefined && tokenFile === undefined) {
    throw new AlienError(
      CommandReceiverConfigInvalidError.create({
        envVar: ENV_ALIEN_COMMANDS_TOKEN,
        reason: `${ENV_ALIEN_COMMANDS_TOKEN} or ${ENV_ALIEN_COMMANDS_TOKEN_FILE} is required`,
      }),
    )
  }
  const deploymentId = requireEnv(env, ENV_ALIEN_DEPLOYMENT_ID)
  const resourceId = requireEnv(env, ENV_ALIEN_COMMANDS_TARGET_RESOURCE_ID)

  const rawType = requireEnv(env, ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE)
  if (rawType !== "container" && rawType !== "daemon") {
    throw new AlienError(
      CommandReceiverConfigInvalidError.create({
        envVar: ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE,
        reason: `${ENV_ALIEN_COMMANDS_TARGET_RESOURCE_TYPE} must be 'container' or 'daemon', got '${rawType}'`,
      }),
    )
  }

  return {
    url,
    token,
    tokenFile,
    deploymentId,
    resourceId,
    resourceType: rawType,
    ...validateRuntimeConfig(env, options),
  }
}

function validateRuntimeConfig(
  env: Record<string, string | undefined>,
  options: CommandReceiverRuntimeOptions,
): ReceiverRuntimeConfig {
  const pollIntervalMs = numericConfig(
    env,
    ENV_ALIEN_COMMANDS_POLL_INTERVAL_MS,
    options.pollIntervalMs,
    DEFAULT_POLL_INTERVAL_MS,
    value => Number.isInteger(value) && value > 0,
  )
  const pollMaxIntervalMs = numericConfig(
    env,
    ENV_ALIEN_COMMANDS_POLL_MAX_INTERVAL_MS,
    options.pollMaxIntervalMs,
    DEFAULT_POLL_MAX_INTERVAL_MS,
    value => Number.isInteger(value) && value > 0,
  )
  if (pollMaxIntervalMs < pollIntervalMs) {
    throw new AlienError(
      CommandReceiverConfigInvalidError.create({
        envVar: ENV_ALIEN_COMMANDS_POLL_MAX_INTERVAL_MS,
        reason: `${ENV_ALIEN_COMMANDS_POLL_MAX_INTERVAL_MS} must be at least ${ENV_ALIEN_COMMANDS_POLL_INTERVAL_MS}`,
      }),
    )
  }

  return {
    pollIntervalMs,
    pollMaxIntervalMs,
    pollJitter: numericConfig(
      env,
      ENV_ALIEN_COMMANDS_POLL_JITTER,
      options.pollJitter,
      DEFAULT_POLL_JITTER,
      value => value >= 0 && value <= 1,
    ),
    leaseSeconds: numericConfig(
      env,
      ENV_ALIEN_COMMANDS_LEASE_SECONDS,
      options.leaseSeconds,
      DEFAULT_LEASE_SECONDS,
      value => Number.isInteger(value) && value > 0,
    ),
    maxLeases: numericConfig(
      env,
      ENV_ALIEN_COMMANDS_MAX_LEASES,
      options.maxLeases,
      DEFAULT_MAX_LEASES,
      value => Number.isInteger(value) && value > 0,
    ),
    drainTimeoutMs: numericConfig(
      env,
      ENV_ALIEN_COMMANDS_DRAIN_TIMEOUT_MS,
      options.drainTimeoutMs,
      DEFAULT_DRAIN_TIMEOUT_MS,
      value => Number.isInteger(value) && value >= 0,
    ),
  }
}

class TokenSource {
  private cachedFileToken: string | undefined

  constructor(
    private readonly token: string | undefined,
    private readonly tokenFile: string | undefined,
  ) {}

  async get(): Promise<string> {
    if (this.token !== undefined) return this.token
    if (this.cachedFileToken !== undefined) return this.cachedFileToken
    return this.readFileToken()
  }

  async refresh(): Promise<string | undefined> {
    if (this.token !== undefined) return undefined
    return this.readFileToken()
  }

  private async readFileToken(): Promise<string> {
    const path = this.tokenFile as string
    let token: string
    try {
      token = (await readFile(path, "utf8")).trim()
    } catch (error) {
      throw (await AlienError.from(error)).withContext(
        CommandReceiverConfigInvalidError.create({
          envVar: ENV_ALIEN_COMMANDS_TOKEN_FILE,
          reason: `Failed to read command token file '${path}'`,
        }),
      )
    }
    if (token === "") {
      throw new AlienError(
        CommandReceiverConfigInvalidError.create({
          envVar: ENV_ALIEN_COMMANDS_TOKEN_FILE,
          reason: `${ENV_ALIEN_COMMANDS_TOKEN_FILE} '${path}' contains an empty token`,
        }),
      )
    }
    this.cachedFileToken = token
    return token
  }
}

class EnvironmentReceiverConnectionProvider implements ReceiverConnectionProvider {
  private readonly tokenSource: TokenSource

  constructor(private readonly config: ReceiverConfig) {
    this.tokenSource = new TokenSource(config.token, config.tokenFile)
  }

  async get(): Promise<ReceiverConnection> {
    return this.connection(await this.tokenSource.get())
  }

  async refresh(): Promise<ReceiverConnection | undefined> {
    const token = await this.tokenSource.refresh()
    return token === undefined ? undefined : this.connection(token)
  }

  private connection(token: string): ReceiverConnection {
    return {
      url: this.config.url,
      token,
      deploymentId: this.config.deploymentId,
      resourceId: this.config.resourceId,
      resourceType: this.config.resourceType,
    }
  }
}

class HostedReceiverConnectionProvider implements ReceiverConnectionProvider {
  constructor(
    private readonly provider: CommandConnectionProvider,
    private readonly deploymentId: string,
  ) {}

  async get(): Promise<ReceiverConnection> {
    return this.receiverConnection(await this.provider.get())
  }

  async refresh(): Promise<ReceiverConnection | undefined> {
    const connection = await this.provider.refresh()
    return connection === undefined ? undefined : this.receiverConnection(connection)
  }

  private receiverConnection(connection: CommandConnection): ReceiverConnection {
    if (connection.role !== "receiver") {
      throw new AlienError(
        InvalidEnvelopeError.create({
          field: "role",
          reason: "Receiver bootstrap returned a sender connection",
        }),
      )
    }
    return {
      url: managerCommandsUrl(connection.managerUrl),
      token: connection.token,
      deploymentId: this.deploymentId,
      resourceId: connection.target.resourceId,
      resourceType: connection.target.resourceType,
    }
  }
}
