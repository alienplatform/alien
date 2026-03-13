import {
  type Container as ContainerConfig,
  type ContainerCode,
  type ContainerAutoscaling,
  type ContainerGpuSpec,
  type HealthCheck,
  type ResourceSpec,
  type ResourceType,
  ContainerSchema,
} from "./generated/index.js"
import { Resource } from "./resource.js"

export type {
  Container as ContainerConfig,
  ContainerOutputs,
  ContainerCode,
  ContainerAutoscaling,
  ContainerPort,
  ExposeProtocol,
  ContainerGpuSpec,
  ContainerStatus,
  HealthCheck,
  PersistentStorage,
  ResourceSpec,
  ReplicaStatus,
} from "./generated/index.js"
export {
  ContainerSchema as ContainerConfigSchema,
  ContainerPortSchema,
  ExposeProtocolSchema,
  ContainerCodeSchema,
  ContainerAutoscalingSchema,
} from "./generated/index.js"

/**
 * Represents a long-running container workload.
 * 
 * Containers run on compute instances (EC2, GCE, Azure VMs) and are orchestrated
 * by Horizon. They're designed for always-on workloads like web services, APIs,
 * databases, and background workers.
 */
export class Container {
  private _config: Partial<ContainerConfig> = {
    links: [],
    ports: [],
    environment: {},
    stateful: false,
    // cluster is optional - if not set, ContainerClusterMutation will auto-assign
  }

  /**
   * Creates a new Container builder.
   * @param id Identifier for the container. Must be DNS-compatible: lowercase alphanumeric with hyphens.
   */
  constructor(id: string) {
    this._config.id = id
  }

  /**
   * Returns a ResourceType representing any container resource.
   * Used for creating permission targets that apply to all container resources.
   * @returns The "container" resource type.
   */
  public static any(): ResourceType {
    return "container"
  }

  /**
   * Sets the container cluster this container runs on.
   * @param clusterId The ContainerCluster resource ID.
   * @returns The Container builder instance.
   */
  public cluster(clusterId: string): this {
    this._config.cluster = clusterId
    return this
  }

  /**
   * Sets the code for the container, either a pre-built image or source code to be built.
   * @param code The container code configuration.
   * @returns The Container builder instance.
   */
  public code(code: ContainerCode): this {
    this._config.code = code
    return this
  }

  /**
   * Sets the CPU resources for the container.
   * 
   * For simplified configuration, use a single number:
   * - `.cpu(1)` sets both min and desired to 1 vCPU
   * 
   * For advanced configuration, use ResourceSpec:
   * - `.cpu({ min: "0.5", desired: "1" })`
   * 
   * @param value CPU in vCPUs (number) or ResourceSpec with min/desired.
   * @returns The Container builder instance.
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
   * Sets the memory resources for the container.
   * 
   * Format: "<number>Mi" or "<number>Gi"
   * 
   * Example: "512Mi", "2Gi", "16Gi"
   * 
   * @param size Memory size string.
   * @returns The Container builder instance.
   */
  public memory(size: string): this {
    this._config.memory = { min: size, desired: size }
    return this
  }

  /**
   * Sets minimum and maximum replica counts with autoscaling configuration.
   * 
   * @param min Minimum replicas (always running).
   * @param max Maximum replicas under load.
   * @returns The Container builder instance.
   */
  public minReplicas(min: number): this {
    if (!this._config.autoscaling) {
      this._config.autoscaling = {
        min,
        desired: min,
        max: min * 10, // Default max is 10x min
      }
    } else {
      this._config.autoscaling.min = min
      this._config.autoscaling.desired = min
    }
    return this
  }

  /**
   * Sets the maximum replica count for autoscaling.
   * @param max Maximum replicas.
   * @returns The Container builder instance.
   */
  public maxReplicas(max: number): this {
    if (!this._config.autoscaling) {
      this._config.autoscaling = {
        min: 1,
        desired: 1,
        max,
      }
    } else {
      this._config.autoscaling.max = max
    }
    return this
  }

  /**
   * Sets a fixed replica count (for stateful containers or stateless without autoscaling).
   * Cannot be used with minReplicas/maxReplicas.
   * @param count Fixed number of replicas.
   * @returns The Container builder instance.
   */
  public replicas(count: number): this {
    this._config.replicas = count
    this._config.autoscaling = undefined
    return this
  }

  /**
   * Configures autoscaling behavior.
   * @param config Autoscaling configuration.
   * @returns The Container builder instance.
   */
  public autoScale(config: ContainerAutoscaling): this {
    this._config.autoscaling = config
    return this
  }

  /**
   * Sets whether this container is stateful.
   * Stateful containers get stable ordinals and support persistent storage.
   * @param enabled Whether the container is stateful.
   * @returns The Container builder instance.
   */
  public stateful(enabled: boolean): this {
    this._config.stateful = enabled
    return this
  }

  /**
   * Adds an internal-only port to the container.
   * Automatically creates DNS records for service discovery.
   * @param port Port number (e.g., 3000, 8080, 5432).
   * @returns The Container builder instance.
   */
  public port(port: number): this {
    if (!this._config.ports) {
      this._config.ports = []
    }
    this._config.ports.push({ port })
    return this
  }

  /**
   * Adds multiple internal-only ports to the container.
   * @param ports Array of port numbers.
   * @returns The Container builder instance.
   */
  public ports(ports: number[]): this {
    if (!this._config.ports) {
      this._config.ports = []
    }
    this._config.ports.push(...ports.map(port => ({ port })))
    return this
  }

  /**
   * Exposes a specific port publicly via load balancer.
   * @param port Port number to expose.
   * @param protocol "http" for HTTPS with TLS termination, "tcp" for TCP passthrough.
   * @returns The Container builder instance.
   */
  public exposePort(port: number, protocol: "http" | "tcp"): this {
    if (!this._config.ports) {
      this._config.ports = []
    }
    
    // Find existing port or add new one
    const existingPort = this._config.ports.find(p => p.port === port)
    if (existingPort) {
      existingPort.expose = protocol
    } else {
      this._config.ports.push({ port, expose: protocol })
    }
    return this
  }

  /**
   * Convenience method to expose the first/primary port publicly.
   * Must be called after .port() or .ports().
   * @param protocol "http" for HTTPS with TLS termination, "tcp" for TCP passthrough.
   * @returns The Container builder instance.
   */
  public expose(protocol: "http" | "tcp"): this {
    if (!this._config.ports || this._config.ports.length === 0) {
      throw new Error("Cannot expose port: no ports defined. Call .port() first.")
    }
    if (!this._config.ports[0]) {
      throw new Error("Cannot expose port: ports array is empty")
    }
    this._config.ports[0].expose = protocol
    return this
  }

  /**
   * Sets environment variables for the container.
   * @param vars Key-value pairs of environment variables.
   * @returns The Container builder instance.
   */
  public environment(vars: Record<string, string>): this {
    this._config.environment = { ...this._config.environment, ...vars }
    return this
  }

  /**
   * Sets ephemeral storage size.
   * Data is lost on container restart.
   * @param size Storage size (e.g., "10Gi", "100Gi", "500Gi").
   * @returns The Container builder instance.
   */
  public ephemeralStorage(size: string): this {
    this._config.ephemeralStorage = size
    return this
  }

  /**
   * Configures persistent storage (requires stateful=true).
   * Data survives container restarts.
   * @param size Storage size (e.g., "100Gi", "500Gi", "1Ti").
   * @returns The Container builder instance.
   */
  public persistentStorage(size: string): this {
    this._config.persistentStorage = {
      size,
      mountPath: "/data",
    }
    this._config.stateful = true
    return this
  }

  /**
   * Requests GPU resources.
   * @param config GPU configuration with type and count.
   * @returns The Container builder instance.
   */
  public gpu(config: ContainerGpuSpec): this {
    this._config.gpu = config
    return this
  }

  /**
   * Configures health check for the container.
   * @param config Health check configuration.
   * @returns The Container builder instance.
   */
  public healthCheck(config: HealthCheck): this {
    this._config.healthCheck = config
    return this
  }

  /**
   * Configures readiness probe (alias for healthCheck).
   * @param config Readiness probe configuration with method and path.
   * @returns The Container builder instance.
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
   * Sets the permission profile for this container.
   * @param profile Permission profile name from stack permissions configuration.
   * @returns The Container builder instance.
   */
  public permissions(profile: string): this {
    this._config.permissions = profile
    return this
  }

  /**
   * Assigns the container to a specific compute pool.
   * @param poolName Compute pool name.
   * @returns The Container builder instance.
   */
  public pool(poolName: string): this {
    this._config.pool = poolName
    return this
  }

  /**
   * Sets the command to override the image default.
   * @param command Array of command arguments.
   * @returns The Container builder instance.
   */
  public command(command: string[]): this {
    this._config.command = command
    return this
  }

  /**
   * Links this container to another resource (Storage, Queue, KV, etc.).
   * @param resource The resource to link to.
   * @returns The Container builder instance.
   */
  public link(resource: Resource): this {
    if (!this._config.links) {
      this._config.links = []
    }
    this._config.links.push(resource.ref())
    return this
  }

  /**
   * Builds and validates the container configuration.
   * @returns An immutable Resource representing the configured container.
   * @throws Error if the container configuration is invalid.
   */
  public build(): Resource {
    const config = ContainerSchema.parse(this._config)

    return new Resource({
      type: "container",
      ...config,
    })
  }
}

