import {
  type DaemonCode,
  type Daemon as DaemonConfig,
  DaemonSchema,
  type ResourceType,
} from "./generated/index.js"
import { Resource } from "./resource.js"

export type { Daemon as DaemonConfig, DaemonCode, DaemonOutputs } from "./generated/index.js"
export {
  DaemonCodeSchema,
  DaemonSchema as DaemonConfigSchema,
  DaemonOutputsSchema,
} from "./generated/index.js"

/**
 * Represents a resident process that runs alongside local or Kubernetes workloads.
 *
 * Daemons are intended for long-lived background processes such as endpoint
 * agents and local side services. They are only supported on Local and
 * Kubernetes platforms.
 */
export class Daemon {
  private _config: Partial<DaemonConfig> = {
    links: [],
    environment: {},
  }

  /**
   * Creates a new Daemon builder.
   * @param id Identifier for the daemon. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]). Maximum 64 characters.
   */
  constructor(id: string) {
    this._config.id = id
  }

  /**
   * Returns a ResourceType representing any daemon resource.
   * Used for creating permission targets that apply to all daemon resources.
   * @returns The "daemon" resource type.
   */
  public static any(): ResourceType {
    return "daemon"
  }

  /**
   * Sets the code for the daemon, either a pre-built image or source code to be built.
   * @param code The daemon code configuration.
   * @returns The Daemon builder instance.
   */
  public code(code: DaemonCode): this {
    this._config.code = code
    return this
  }

  /**
   * Sets key-value pairs as environment variables for the daemon.
   * @param env A map of environment variable names to their values.
   * @returns The Daemon builder instance.
   */
  public environment(env: Record<string, string>): this {
    this._config.environment = env
    return this
  }

  /**
   * Links another resource to this daemon.
   * This makes the linked resource accessible to the daemon, often by injecting
   * environment variables or granting permissions.
   * @param resource The resource to link.
   * @returns The Daemon builder instance.
   */
  public link(resource: Resource): this {
    if (!this._config.links) {
      this._config.links = []
    }
    this._config.links.push(resource.ref())
    return this
  }

  /**
   * Assigns a permission profile to this daemon.
   * The profile defines the permissions granted to this daemon when interacting
   * with other cloud resources.
   * @param permissions The permission profile name.
   * @returns The Daemon builder instance.
   */
  public permissions(permissions: string): this {
    this._config.permissions = permissions
    return this
  }

  /**
   * Enables or disables the Commands protocol for the daemon.
   * When enabled, the runtime polls the manager for pending commands and executes registered handlers.
   * Default: false.
   * @param enabled Whether to enable commands for this daemon.
   * @returns The Daemon builder instance.
   */
  public commandsEnabled(enabled: boolean): this {
    this._config.commandsEnabled = enabled
    return this
  }

  /**
   * Builds and validates the daemon configuration.
   * @returns An immutable Resource representing the configured daemon.
   * @throws Error if the daemon configuration is invalid.
   */
  public build(): Resource {
    const config = DaemonSchema.parse(this._config)

    return new Resource({
      type: "daemon",
      ...config,
    })
  }
}
