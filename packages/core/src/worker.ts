import {
  type Ingress,
  type ReadinessProbe,
  type ResourceType,
  type WorkerCode,
  type Worker as WorkerConfig,
  WorkerSchema,
  type WorkerTrigger,
} from "./generated/index.js"
import { Resource } from "./resource.js"

export type {
  Ingress,
  Worker as WorkerConfig,
  WorkerOutputs,
  WorkerTrigger,
  ReadinessProbe,
  HttpMethod,
} from "./generated/index.js"
export {
  IngressSchema,
  WorkerSchema as WorkerConfigSchema,
  WorkerOutputsSchema,
  WorkerTriggerSchema,
  ReadinessProbeSchema,
  HttpMethodSchema,
} from "./generated/index.js"

/**
 * Represents a serverless worker that executes code in response to triggers or direct invocations.
 * Workers are the primary compute resource in serverless applications, designed to be stateless and ephemeral.
 */
export class Worker {
  private _config: Partial<WorkerConfig> = {
    links: [],
    triggers: [],
    environment: {},
  }

  /**
   * Creates a new Worker builder.
   * @param id Identifier for the worker. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]). Maximum 64 characters.
   */
  constructor(id: string) {
    this._config.id = id
  }

  /**
   * Returns a ResourceType representing any worker resource.
   * Used for creating permission targets that apply to all worker resources.
   * @returns The "worker" resource type.
   */
  public static any(): ResourceType {
    return "worker"
  }

  /**
   * Sets the code for the worker, either a pre-built image or source code to be built.
   * @param code The worker code configuration.
   * @returns The Worker builder instance.
   */
  public code(code: WorkerCode): this {
    this._config.code = code
    return this
  }

  /**
   * Sets the memory allocated to the worker in megabytes (MB).
   * Constraints: 128‑32768 MB (platform-specific limits may apply).
   * Default: 256 MB.
   * @param mb The memory in MB.
   * @returns The Worker builder instance.
   */
  public memoryMb(mb: number): this {
    this._config.memoryMb = mb
    return this
  }

  /**
   * Sets the maximum execution time for the worker in seconds.
   * Constraints: 1‑3600 seconds (platform-specific limits may apply).
   * Default: 30 seconds.
   * @param sec The timeout in seconds.
   * @returns The Worker builder instance.
   */
  public timeoutSeconds(sec: number): this {
    this._config.timeoutSeconds = sec
    return this
  }

  /**
   * Sets the maximum number of concurrent executions allowed for the worker.
   * `null` means platform default applies.
   * @param limit The concurrency limit, or `null` for platform default.
   * @returns The Worker builder instance.
   */
  public concurrencyLimit(limit: number | undefined): this {
    this._config.concurrencyLimit = limit
    return this
  }

  /**
   * Controls network accessibility of the worker.
   * - `public`: Worker accessible from the internet.
   * - `private`: Worker accessible only via cloud API calls / triggers.
   * - `vpc`: Worker deployed within a VPC with specific network controls.
   * Default: `private`.
   * @param value The ingress type.
   * @returns The Worker builder instance.
   */
  public ingress(value: Ingress): this {
    this._config.ingress = value
    return this
  }

  /**
   * Sets key-value pairs as environment variables for the worker.
   * @param env A map of environment variable names to their values.
   * @returns The Worker builder instance.
   */
  public environment(env: Record<string, string>): this {
    this._config.environment = env
    return this
  }

  /**
   * Links another resource (e.g., Storage, KV, Queue) to this worker.
   * This makes the linked resource accessible to the worker, often by injecting
   * environment variables or granting permissions.
   * @param resource The resource to link.
   * @returns The Worker builder instance.
   */
  public link(resource: Resource): this {
    if (!this._config.links) {
      this._config.links = []
    }
    this._config.links.push(resource.ref())
    return this
  }

  /**
   * Assigns a permission profile to this worker.
   * The profile defines the permissions granted to this worker when interacting
   * with other cloud resources.
   * @param permissions The permission profile name.
   * @returns The Worker builder instance.
   */
  public permissions(permissions: string): this {
    this._config.permissions = permissions
    return this
  }

  /**
   * Configures a readiness probe for the worker.
   * The probe will be executed after provisioning/update to verify the worker is ready.
   * Only works with workers that have Public ingress.
   *
   * @example
   * ```typescript
   * const probe: ReadinessProbe = {
   *   method: "GET",
   *   path: "/health"
   * };
   *
   * const func = new Worker("my-api")
   *   .code({ type: "image", image: "my-api:latest" })
   *   .ingress("public")
   *   .readinessProbe(probe)
   *   .build();
   * ```
   *
   * @param probe The readiness probe configuration.
   * @returns The Worker builder instance.
   */
  public readinessProbe(probe: ReadinessProbe): this {
    this._config.readinessProbe = probe
    return this
  }

  /**
   * Enables or disables the Commands protocol for the worker.
   * When enabled, the runtime polls the manager for pending commands and executes registered handlers.
   * Default: false.
   * @param enabled Whether to enable commands for this worker.
   * @returns The Worker builder instance.
   */
  public commandsEnabled(enabled: boolean): this {
    this._config.commandsEnabled = enabled
    return this
  }

  /**
   * Adds a trigger to the worker. Workers can have multiple triggers.
   * Each trigger will independently invoke the worker when its conditions are met.
   *
   * @example
   * ```typescript
   * // Queue trigger
   * const queueTrigger: WorkerTrigger = {
   *   type: "queue",
   *   queue: myQueue.ref()
   * };
   *
   * // Schedule trigger
   * const scheduleTrigg: WorkerTrigger = {
   *   type: "schedule",
   *   cron: "0 * * * *"
   * };
   *
   * const func = new Worker("my-func")
   *   .code({ type: "image", image: "my-image:latest" })
   *   .trigger(queueTrigger)
   *   .trigger(scheduleTrigger)
   *   .build();
   * ```
   *
   * @param trigger The trigger configuration.
   * @returns The Worker builder instance.
   */
  public trigger(trigger: WorkerTrigger): this {
    if (!this._config.triggers) {
      this._config.triggers = []
    }
    this._config.triggers.push(trigger)
    return this
  }

  /**
   * Builds and validates the worker configuration.
   * @returns An immutable Resource representing the configured worker.
   * @throws Error if the worker configuration is invalid (e.g., missing code).
   */
  public build(): Resource {
    const config = WorkerSchema.parse(this._config)

    return new Resource({
      type: "worker",
      ...config,
    })
  }
}
