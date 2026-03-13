import {
  type FunctionCode,
  type Function as FunctionConfig,
  FunctionSchema,
  type FunctionTrigger,
  type Ingress,
  type ReadinessProbe,
  type ResourceType,
} from "./generated/index.js"
import { Resource } from "./resource.js"

export type {
  Ingress,
  Function as FunctionConfig,
  FunctionOutputs,
  FunctionTrigger,
  ReadinessProbe,
  HttpMethod,
} from "./generated/index.js"
export {
  IngressSchema,
  FunctionSchema as FunctionConfigSchema,
  FunctionOutputsSchema,
  FunctionTriggerSchema,
  ReadinessProbeSchema,
  HttpMethodSchema,
} from "./generated/index.js"

/**
 * Represents a serverless function that executes code in response to triggers or direct invocations.
 * Functions are the primary compute resource in serverless applications, designed to be stateless and ephemeral.
 */
// biome-ignore lint/suspicious/noShadowRestrictedNames: intentionally shadows built-in `Function`
export class Function {
  private _config: Partial<FunctionConfig> = {
    links: [],
    triggers: [],
    environment: {},
  }

  /**
   * Creates a new Function builder.
   * @param id Identifier for the function. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]). Maximum 64 characters.
   */
  constructor(id: string) {
    this._config.id = id
  }

  /**
   * Returns a ResourceType representing any function resource.
   * Used for creating permission targets that apply to all function resources.
   * @returns The "function" resource type.
   */
  public static any(): ResourceType {
    return "function"
  }

  /**
   * Sets the code for the function, either a pre-built image or source code to be built.
   * @param code The function code configuration.
   * @returns The Function builder instance.
   */
  public code(code: FunctionCode): this {
    this._config.code = code
    return this
  }

  /**
   * Sets the memory allocated to the function in megabytes (MB).
   * Constraints: 128‑32768 MB (platform-specific limits may apply).
   * Default: 256 MB.
   * @param mb The memory in MB.
   * @returns The Function builder instance.
   */
  public memoryMb(mb: number): this {
    this._config.memoryMb = mb
    return this
  }

  /**
   * Sets the maximum execution time for the function in seconds.
   * Constraints: 1‑3600 seconds (platform-specific limits may apply).
   * Default: 30 seconds.
   * @param sec The timeout in seconds.
   * @returns The Function builder instance.
   */
  public timeoutSeconds(sec: number): this {
    this._config.timeoutSeconds = sec
    return this
  }

  /**
   * Sets the maximum number of concurrent executions allowed for the function.
   * `null` means platform default applies.
   * @param limit The concurrency limit, or `null` for platform default.
   * @returns The Function builder instance.
   */
  public concurrencyLimit(limit: number | undefined): this {
    this._config.concurrencyLimit = limit
    return this
  }

  /**
   * Controls network accessibility of the function.
   * - `public`: Function accessible from the internet.
   * - `private`: Function accessible only via cloud API calls / triggers.
   * - `vpc`: Function deployed within a VPC with specific network controls.
   * Default: `private`.
   * @param value The ingress type.
   * @returns The Function builder instance.
   */
  public ingress(value: Ingress): this {
    this._config.ingress = value
    return this
  }

  /**
   * Sets key-value pairs as environment variables for the function.
   * @param env A map of environment variable names to their values.
   * @returns The Function builder instance.
   */
  public environment(env: Record<string, string>): this {
    this._config.environment = env
    return this
  }

  /**
   * Links another resource (e.g., Storage, KV, Queue) to this function.
   * This makes the linked resource accessible to the function, often by injecting
   * environment variables or granting permissions.
   * @param resource The resource to link.
   * @returns The Function builder instance.
   */
  public link(resource: Resource): this {
    if (!this._config.links) {
      this._config.links = []
    }
    this._config.links.push(resource.ref())
    return this
  }

  /**
   * Assigns a permission profile to this function.
   * The profile defines the permissions granted to this function when interacting
   * with other cloud resources.
   * @param permissions The permission profile name.
   * @returns The Function builder instance.
   */
  public permissions(permissions: string): this {
    this._config.permissions = permissions
    return this
  }

  /**
   * Configures a readiness probe for the function.
   * The probe will be executed after provisioning/update to verify the function is ready.
   * Only works with functions that have Public ingress.
   *
   * @example
   * ```typescript
   * const probe: ReadinessProbe = {
   *   method: "GET",
   *   path: "/health"
   * };
   *
   * const func = new Function("my-api")
   *   .code({ type: "image", image: "my-api:latest" })
   *   .ingress("public")
   *   .readinessProbe(probe)
   *   .build();
   * ```
   *
   * @param probe The readiness probe configuration.
   * @returns The Function builder instance.
   */
  public readinessProbe(probe: ReadinessProbe): this {
    this._config.readinessProbe = probe
    return this
  }

  /**
   * Enables or disables ARC (Alien Remote Call) protocol for the function.
   * When enabled, the function can be invoked via ARC protocol from the control plane.
   * The necessary queue infrastructure is automatically created for the target platform.
   * Default: false.
   * @param enabled Whether to enable ARC for this function.
   * @returns The Function builder instance.
   */
  public arcEnabled(enabled: boolean): this {
    this._config.arcEnabled = enabled
    return this
  }

  /**
   * Adds a trigger to the function. Functions can have multiple triggers.
   * Each trigger will independently invoke the function when its conditions are met.
   *
   * @example
   * ```typescript
   * // Queue trigger
   * const queueTrigger: FunctionTrigger = {
   *   type: "queue",
   *   queue: myQueue.ref()
   * };
   *
   * // Schedule trigger
   * const scheduleTrigg: FunctionTrigger = {
   *   type: "schedule",
   *   cron: "0 * * * *"
   * };
   *
   * const func = new Function("my-func")
   *   .code({ type: "image", image: "my-image:latest" })
   *   .trigger(queueTrigger)
   *   .trigger(scheduleTrigger)
   *   .build();
   * ```
   *
   * @param trigger The trigger configuration.
   * @returns The Function builder instance.
   */
  public trigger(trigger: FunctionTrigger): this {
    if (!this._config.triggers) {
      this._config.triggers = []
    }
    this._config.triggers.push(trigger)
    return this
  }

  /**
   * Builds and validates the function configuration.
   * @returns An immutable Resource representing the configured function.
   * @throws Error if the function configuration is invalid (e.g., missing code).
   */
  public build(): Resource {
    const config = FunctionSchema.parse(this._config)

    return new Resource({
      type: "function",
      ...config,
    })
  }
}
