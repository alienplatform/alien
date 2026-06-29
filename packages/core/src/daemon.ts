import {
  type DaemonCode,
  type Daemon as DaemonConfig,
  type DaemonRuntime,
  DaemonSchema,
  type ExposeProtocol,
  type HealthCheck,
  type PublicEndpoint,
  type ResourceSpec,
  type ResourceType,
} from "./generated/index.js"
import { Resource } from "./resource.js"

export type {
  Daemon as DaemonConfig,
  DaemonCode,
  DaemonOutputs,
  DaemonRuntime,
  ExposeProtocol,
  HealthCheck,
  PublicEndpoint,
  ResourceSpec,
} from "./generated/index.js"
export {
  DaemonCodeSchema,
  DaemonSchema as DaemonConfigSchema,
  DaemonOutputsSchema,
  PublicEndpointSchema,
} from "./generated/index.js"

export type DaemonPublicEndpointOptions =
  | ExposeProtocol
  | {
      protocol: ExposeProtocol
      hostLabel?: string
      wildcardSubdomains?: boolean
    }

/**
 * Represents a resident process that runs once per eligible machine or node.
 *
 * Daemons are intended for long-lived background processes such as endpoint
 * agents and local side services.
 */
export class Daemon {
  private _config: Partial<DaemonConfig> = {
    links: [],
    publicEndpoints: [],
    environment: {},
    cpu: { min: "0.1", desired: "0.1" },
    memory: { min: "128Mi", desired: "128Mi" },
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
   * Sets the ComputeCluster this daemon runs on for AWS/GCP/Azure deployments.
   * Kubernetes and Local deployments ignore this field.
   */
  public cluster(clusterId: string): this {
    this._config.cluster = clusterId
    return this
  }

  /**
   * Sets CPU resources for each daemon instance.
   */
  public cpu(value: number | ResourceSpec): this {
    if (typeof value === "number") {
      this._config.cpu = { min: value.toString(), desired: value.toString() }
    } else {
      this._config.cpu = value
    }
    return this
  }

  /**
   * Sets memory resources for each daemon instance.
   */
  public memory(size: string): this {
    this._config.memory = { min: size, desired: size }
    return this
  }

  /**
   * Sets the backend pool/capacity group for daemon placement.
   */
  public pool(pool: string): this {
    this._config.pool = pool
    return this
  }

  /**
   * Overrides the image default command.
   */
  public command(command: string[]): this {
    this._config.command = command
    return this
  }

  /**
   * Sets backend runtime options for trusted daemon infrastructure.
   *
   * Use this only for daemons that intentionally need host-level access, such
   * as a privileged loader that installs or supervises a native host process.
   */
  public runtime(runtime: DaemonRuntime): this {
    this._config.runtime = runtime
    return this
  }

  /**
   * Exposes a named public endpoint for a daemon port.
   */
  public publicEndpoint(
    name: string,
    port: number,
    options: DaemonPublicEndpointOptions = "http",
  ): this {
    if (!this._config.publicEndpoints) {
      this._config.publicEndpoints = []
    }

    const endpoint =
      typeof options === "string"
        ? { protocol: options, hostLabel: undefined, wildcardSubdomains: false }
        : options

    const publicEndpoint: PublicEndpoint = {
      name,
      port,
      protocol: endpoint.protocol,
      hostLabel: endpoint.hostLabel,
      wildcardSubdomains: endpoint.wildcardSubdomains ?? false,
    }

    this._config.publicEndpoints.push(publicEndpoint)
    return this
  }

  /**
   * Configures the HTTP health check used by public daemon endpoint load balancers.
   */
  public healthCheck(config: HealthCheck): this {
    this._config.healthCheck = config
    return this
  }

  /**
   * Configures readiness probe (alias for healthCheck).
   */
  public readinessProbe(config: { method: string; path: string }): this {
    this._config.healthCheck = {
      path: config.path,
      method: config.method,
      timeoutSeconds: 1,
      failureThreshold: 3,
    }
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
