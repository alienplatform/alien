import {
  type ResourceType,
  type SandboxCode,
  type Sandbox as SandboxConfig,
  type SandboxEgress,
  type SandboxLifecyclePolicy,
  type SandboxLimits,
  SandboxSchema,
} from "./generated/index.js"
import { type Resource, ResourceBuilder } from "./resource.js"

export type {
  Sandbox as SandboxConfig,
  SandboxCapabilities,
  SandboxCode,
  SandboxEgress,
  SandboxLifecyclePolicy,
  SandboxLimits,
  SandboxOutputs,
} from "./generated/index.js"
export { SandboxSchema as SandboxConfigSchema } from "./generated/index.js"

/**
 * An isolated environment for running untrusted code.
 *
 * The declaration provisions a durable parent; individual sandboxes are created and destroyed
 * at runtime through the binding. A runtime egress policy may narrow this declaration.
 * Unsupported policies are refused when planning the stack.
 *
 * Limits are enforced ceilings, not scheduling hints, and are validated when the stack is
 * planned. A platform that cannot enforce them rejects the sandbox rather than ignoring them.
 */
export class Sandbox extends ResourceBuilder {
  private _config: Partial<SandboxConfig> = {}

  /**
   * Creates a new Sandbox builder.
   * @param id Identifier for the sandbox. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]). Maximum 64 characters.
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
   * Sets the outbound network policy.
   */
  public egress(egress: SandboxEgress): this {
    this._config.egress = egress
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
