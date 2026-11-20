import {
  type EgressPolicy,
  type ResourceRef,
  type ResourceType,
  type SandboxCode,
  type Sandbox as SandboxConfig,
  type SandboxEgress,
  type SandboxImageMount,
  type SandboxLifecyclePolicy,
  type SandboxLimits,
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
  SandboxImageConnection,
  SandboxImageMount,
  SandboxImageMountType,
  SandboxImagePermissions,
  SandboxLifecyclePolicy,
  SandboxLimits,
  SandboxOutputs,
  SandboxPrivilegedSupervisor,
} from "./generated/index.js"
export { SandboxSchema as SandboxConfigSchema } from "./generated/index.js"

/**
 * A private service a sandbox image may reach directly: a stack resource, never a raw address.
 */
export interface SandboxImageConnectionDeclaration {
  /** The resource whose private network endpoints the image connects to. */
  to: Resource | ResourceRef
  /** TCP port. Never 53. */
  port: number
}

/**
 * What a sandbox's own image may do beyond the bounded root image grant.
 */
export interface SandboxImagePermissionsDeclaration {
  /** Mounts the image may ask Alien's agent for: `nfs`, or `tmpfs` with a `size`. */
  mounts?: SandboxImageMount[]
  /** Private services the image's own sockets may reach without the egress gateway. */
  connections?: SandboxImageConnectionDeclaration[]
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
   *
   * `onPause` and `onResume` are programs (argv, absolute path first) Alien's agent runs inside
   * the image, with the image's own grant, around a pause that keeps memory. They are best effort
   * and killed at a deadline: the image must also survive a stop with no warning. The program
   * must be one of the image's own files, not on a mount `imagePermissions` declares. AWS only.
   */
  public lifecycle(lifecycle: SandboxLifecyclePolicy): this {
    this._config.lifecycle = lifecycle
    return this
  }

  /**
   * Lets the sandbox's own image mount file systems and reach private services directly.
   *
   * Mounts (`nfs`, `tmpfs`, and `bind` to place a declared mount again) are performed by Alien's
   * agent on the image's behalf at the declared absolute paths; the image never gets `SYS_ADMIN`.
   * Each connection names a stack resource with private network endpoints and is narrowed to the
   * private addresses that resource publishes, on the declared TCP port. Connections bypass the egress gateway and its connection log, and isolation between
   * workloads inside the image is the image's own responsibility; setup tells the customer both.
   *
   * AWS Lambda MicroVM sandboxes only. Other platforms refuse it when the stack is planned.
   */
  public imagePermissions(permissions: SandboxImagePermissionsDeclaration): this {
    this._config.imagePermissions = {
      mounts: permissions.mounts,
      connections: permissions.connections?.map(connection => ({
        to: connection.to instanceof Resource ? connection.to.ref() : connection.to,
        port: connection.port,
      })),
    }
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
