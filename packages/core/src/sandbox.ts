import {
  type EgressPolicy,
  type ResourceType,
  type SandboxCode,
  type Sandbox as SandboxConfig,
  type SandboxEgress,
  type SandboxLifecyclePolicy,
  type SandboxLimits,
  type SandboxPrivilegedSupervisor,
  SandboxSchema,
} from "./generated/index.js"
import { type Resource, ResourceBuilder } from "./resource.js"

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
  SandboxPrivilegedSupervisor,
} from "./generated/index.js"
export { SandboxSchema as SandboxConfigSchema } from "./generated/index.js"

/**
 * An isolated environment for running untrusted code.
 *
 * The declaration provisions a durable parent; individual sandboxes are created and destroyed
 * at runtime through the binding. Runtime policies control each sandbox independently.
 * Unsupported policies are refused when planning the stack.
 *
 * Limits are enforced ceilings, not scheduling hints, and are validated when the stack is
 * planned. A platform that cannot enforce them rejects the sandbox rather than ignoring them.
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

  /**
   * @deprecated Existing supervisor enforcement is preserved. The new
   * egress("internet") enables gateway enforcement and the supervisor automatically.
   */
  public privilegedSupervisor(settings: SandboxPrivilegedSupervisor): this {
    this._config.privilegedSupervisor = settings
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
