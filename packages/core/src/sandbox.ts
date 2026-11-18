import {
  type EgressPolicy,
  type ResourceRef,
  type ResourceType,
  type SandboxCode,
  type Sandbox as SandboxConfig,
  type SandboxEgress,
  type SandboxLifecyclePolicy,
  type SandboxLimits,
  type SandboxPersistentStorage,
  SandboxSchema,
} from "./generated/index.js"
import { Resource, ResourceBuilder } from "./resource.js"

export type {
  EgressPolicy,
  LegacySandboxEgress,
  Sandbox as SandboxConfig,
  SandboxCapabilities,
  SandboxCode,
  SandboxEgress,
  SandboxLifecyclePolicy,
  SandboxLimits,
  SandboxOutputs,
  SandboxPersistentStorage,
  SandboxPrivilegedSupervisor,
} from "./generated/index.js"
export { SandboxSchema as SandboxConfigSchema } from "./generated/index.js"

/** Options for {@link Sandbox.persistentStorage}. */
export interface SandboxPersistentStorageOptions {
  /** Absolute directory inside the sandbox, such as `/workspace`. */
  path: string
  /** The Storage the directory is replicated to: its encryption, retention and uninstall policy apply. */
  storage: Resource | ResourceRef
  /** Paths relative to `path` kept local and never replicated, such as `tmp/`. */
  exclude?: string[]
  /** Largest replicated tree, such as `"8Gi"`. A commit above it is refused whole. */
  maxSize?: string
}

/**
 * An isolated environment for running untrusted code.
 *
 * The declaration provisions a durable parent; individual sandboxes are created and destroyed
 * at runtime through the binding. Runtime policies control each sandbox independently.
 * Unsupported policies are refused when planning the stack.
 *
 * Limits are enforced ceilings, not scheduling hints, and are validated when the stack is
 * planned. A platform that cannot enforce them rejects the sandbox rather than ignoring them.
 *
 * The same declaration works on AWS, Azure and Google Cloud. On AWS a `"live"` sandbox is created
 * and updated at runtime. On Azure and Google Cloud a `"live"` sandbox is created by setup, like a
 * `"frozen"` one, and changes to it go through setup; the build says so.
 */
export class Sandbox extends ResourceBuilder {
  private _config: Partial<SandboxConfig> = {}

  /**
   * Creates a new Sandbox builder.
   * @param id Identifier for the sandbox. Must be DNS-compatible: lowercase letters, digits and hyphens, starting and ending with a letter or digit, with no consecutive hyphens.
   */
  constructor(id: string) {
    super()
    this._config.id = id
  }

  /**
   * Sets where the sandbox's root filesystem comes from.
   */
  public code(code: SandboxCode): this {
    this._config.code = code
    return this
  }

  /**
   * Names a private ECR base image the AWS build pulls. Add the sandbox as `"live"`: the registry
   * opens the repository to the customer's account only once the deployment registers.
   */
  public privateBaseImage(image: string): this {
    this._config.privateBaseImage = image
    return this
  }

  /**
   * Sets the enforced cpu, memory, disk and process ceilings.
   */
  public limits(limits: SandboxLimits): this {
    this._config.limits = limits
    return this
  }

  /**
   * "none" disables outbound networking. "internet" opts into the deployment
   * gateway, public addresses only, a fixed outbound IP, connection logs and
   * per-sandbox runtime policies. Opting in requires updating deployment setup.
   * Legacy object values retain their released backend behavior and do not
   * opt into the gateway. They are deprecated in favor of the new values.
   */
  public egress(egress: SandboxEgress): this {
    this._config.egress = egress
    return this
  }

  /** Sets the runtime policy used when create omits one; requires egress("internet"). */
  public defaultPolicy(policy: EgressPolicy): this {
    this._config.defaultPolicy = policy
    return this
  }

  /**
   * Sets the sandbox lifetime ceiling and idle behaviour.
   */
  public lifecycle(lifecycle: SandboxLifecyclePolicy): this {
    this._config.lifecycle = lifecycle
    return this
  }

  /**
   * Declares the ports eligible for a preview capability.
   *
   * A port not declared here can never be exposed, so an application cannot widen its own
   * ingress at runtime.
   */
  public previewPorts(ports: number[]): this {
    this._config.previewPorts = ports
    return this
  }

  /**
   * Declares a directory whose contents belong to a key the caller chooses, not to a sandbox.
   *
   * Alien's agent replicates the directory to `storage` within seconds of a write and restores
   * it into a new sandbox created with the same `storageKey` before the image command starts.
   * The application reads a key's files without a sandbox and erases a key through the binding.
   * Platforms without a replication transport refuse the declaration when the stack is planned.
   */
  public persistentStorage(options: SandboxPersistentStorageOptions): this {
    const storage = options.storage instanceof Resource ? options.storage.ref() : options.storage
    const persistentStorage: SandboxPersistentStorage = { path: options.path, storage }
    if (options.exclude !== undefined) persistentStorage.exclude = options.exclude
    if (options.maxSize !== undefined) persistentStorage.maxSize = options.maxSize
    this._config.persistentStorage = persistentStorage
    return this
  }

  /**
   * Returns a ResourceType representing any Sandbox resource.
   * Used for creating permission targets that apply to all sandboxes.
   * @returns The "sandbox" resource type.
   */
  public static any(): ResourceType {
    return "sandbox"
  }

  /**
   * Builds and validates the sandbox configuration.
   * @returns An immutable Resource representing the configured sandbox.
   * @throws Error if the sandbox configuration is invalid.
   */
  public build(): Resource {
    const config = SandboxSchema.parse(this._config)

    return this.resource({
      type: "sandbox",
      ...config,
    })
  }
}
