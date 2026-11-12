import { readFile } from "node:fs/promises"
import { AlienError } from "@alienplatform/core"
import {
  type CommandConnection,
  type CommandConnectionProvider,
  HostedCommandConnectionProvider,
  managerCommandsUrl,
  type RefreshingConnectionProvider,
} from "./bootstrap.js"
import { CapabilityFile, ENV_ALIEN_COMMANDS_CAPABILITY_FILE } from "./capability-file.js"
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
const ENV_ALIEN_IDENTITY_PROVIDER = "ALIEN_IDENTITY_PROVIDER"
const ENV_ALIEN_MANAGER_URL = "ALIEN_MANAGER_URL"
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
  /** Set when `ALIEN_IDENTITY_PROVIDER=capabilityFile`; replaces the token variables. */
  capabilityFile?: string
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

export type ReceiverConnectionProvider = RefreshingConnectionProvider<ReceiverConnection> & {
  /** The credential is a file the host rotates; a rejection may clear later. */
  readonly credentialRotates?: boolean
  /**
   * The credential is short-lived, so the authority to submit a result is
   * fetched again from the manager just before the result is sent.
   */
  readonly refreshesResponseContext?: boolean
}

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

  // An enrolled Operator mounts a short-lived capability file and sets
  // `ALIEN_IDENTITY_PROVIDER=capabilityFile`. Like the Rust receiver, that
  // selects the file and ignores any token variables.
  let token: string | undefined
  let tokenFile: string | undefined
  let capabilityFile: string | undefined
  if (env[ENV_ALIEN_IDENTITY_PROVIDER] === "capabilityFile") {
    capabilityFile = requireEnv(env, ENV_ALIEN_COMMANDS_CAPABILITY_FILE)
    validateManagerOrigin(requireEnv(env, ENV_ALIEN_MANAGER_URL), url)
  } else {
    token = optionalNonEmpty(env, ENV_ALIEN_COMMANDS_TOKEN)
    tokenFile = optionalNonEmpty(env, ENV_ALIEN_COMMANDS_TOKEN_FILE)
    if (token === undefined && tokenFile === undefined) {
      throw new AlienError(
        CommandReceiverConfigInvalidError.create({
          envVar: ENV_ALIEN_COMMANDS_TOKEN,
          reason: `${ENV_ALIEN_COMMANDS_TOKEN} or ${ENV_ALIEN_COMMANDS_TOKEN_FILE} is required`,
        }),
      )
    }
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
    capabilityFile,
    deploymentId,
    resourceId,
    resourceType: rawType,
    ...validateRuntimeConfig(env, options),
  }
}

/**
 * A capability may only go to the manager it was issued for: the configured
 * manager URL must be HTTPS (HTTP only on loopback, no credentials, query or
 * fragment) and the commands URL must share its origin.
 */
function validateManagerOrigin(managerUrl: string, commandsUrl: string): void {
  const invalid = (envVar: string, reason: string) =>
    new AlienError(CommandReceiverConfigInvalidError.create({ envVar, reason }))
  let manager: URL
  try {
    manager = new URL(managerUrl)
  } catch {
    throw invalid(ENV_ALIEN_MANAGER_URL, `${ENV_ALIEN_MANAGER_URL} is not a valid URL`)
  }
  const loopback = ["localhost", "127.0.0.1", "[::1]"].includes(manager.hostname)
  if (
    manager.username !== "" ||
    manager.password !== "" ||
    manager.search !== "" ||
    manager.hash !== "" ||
    !(manager.protocol === "https:" || (manager.protocol === "http:" && loopback))
  ) {
    throw invalid(
      ENV_ALIEN_MANAGER_URL,
      `${ENV_ALIEN_MANAGER_URL} must use HTTPS (HTTP is allowed only on loopback) and carry no credentials, query or fragment`,
    )
  }
  const commands = new URL(commandsUrl)
  if (commands.username !== "" || commands.password !== "" || commands.origin !== manager.origin) {
    throw invalid(
      ENV_ALIEN_COMMANDS_URL,
      `${ENV_ALIEN_COMMANDS_URL} must use the configured manager origin`,
    )
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

  async read(): Promise<string> {
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
      throw unavailableTokenFile(
        `Failed to read command token file '${path}'`,
        await AlienError.from(error),
      )
    }
    if (token === "") {
      throw unavailableTokenFile(
        `${ENV_ALIEN_COMMANDS_TOKEN_FILE} '${path}' contains an empty token`,
      )
    }
    this.cachedFileToken = token
    return token
  }
}

/**
 * The host writes the token file and rotates it, so it can be missing, empty
 * or stale for a while. The receiver keeps polling with backoff.
 */
function unavailableTokenFile(reason: string, source?: AlienError): AlienError {
  const definition = CommandReceiverConfigInvalidError.create({
    envVar: ENV_ALIEN_COMMANDS_TOKEN_FILE,
    reason,
  })
  return new AlienError({
    code: definition.metadata.code,
    message: definition.metadata.message(definition.context),
    retryable: true,
    internal: definition.metadata.internal,
    httpStatusCode: definition.metadata.httpStatusCode,
    context: definition.context,
    source,
  })
}

class EnvironmentReceiverConnectionProvider implements ReceiverConnectionProvider {
  private readonly tokenSource: TokenSource | undefined
  private readonly capability: CapabilityFile | undefined
  readonly credentialRotates: boolean
  readonly refreshesResponseContext: boolean

  constructor(private readonly config: ReceiverConfig) {
    if (config.capabilityFile !== undefined) {
      this.capability = new CapabilityFile(config.capabilityFile)
    } else {
      this.tokenSource = new TokenSource(config.token, config.tokenFile)
    }
    this.credentialRotates = config.capabilityFile !== undefined || config.token === undefined
    this.refreshesResponseContext = config.capabilityFile !== undefined
  }

  async get(): Promise<ReceiverConnection> {
    return this.connection(await this.readToken())
  }

  async refresh(
    _signal?: AbortSignal,
    rejected?: ReceiverConnection,
  ): Promise<ReceiverConnection | undefined> {
    if (this.capability !== undefined) {
      if (rejected !== undefined) this.capability.reject(rejected.token)
      return this.connection(await this.capability.read())
    }
    const token = await this.tokenSource?.refresh()
    return token === undefined ? undefined : this.connection(token)
  }

  reject(rejected: ReceiverConnection): void {
    this.capability?.reject(rejected.token)
  }

  private readToken(): Promise<string> {
    return (this.capability ?? (this.tokenSource as TokenSource)).read()
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

  async get(signal?: AbortSignal): Promise<ReceiverConnection> {
    return this.receiverConnection(await this.provider.get(signal))
  }

  async refresh(signal?: AbortSignal): Promise<ReceiverConnection | undefined> {
    const connection = await this.provider.refresh(signal)
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
