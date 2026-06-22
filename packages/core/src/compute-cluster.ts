import {
  type CapacityGroup,
  type ComputeCluster as ComputeClusterConfig,
  ComputeClusterSchema,
  type ResourceType,
} from "./generated/index.js"
import { Resource } from "./resource.js"

export type {
  ComputeCluster as ComputeClusterConfig,
  CapacityGroup,
  MachineProfile,
} from "./generated/index.js"
export {
  ComputeClusterSchema as ComputeClusterConfigSchema,
  CapacityGroupSchema,
  MachineProfileSchema,
} from "./generated/index.js"

/**
 * Capacity-group configuration accepted by `ComputeCluster.capacityGroup()`.
 *
 * `groupId` is supplied as the first arg to `.capacityGroup()`, so this type
 * is the rest of the field set with optional `minSize`/`maxSize` defaults.
 */
export type CapacityGroupInput = Omit<CapacityGroup, "groupId"> & {
  minSize?: number
  maxSize?: number
}

/**
 * Declares a ComputeCluster — the setup-owned machine boundary for daemons and
 * containers. Each capacity group inside the cluster becomes a separate
 * Auto Scaling Group (AWS), Managed Instance Group (GCP), or VM Scale Set
 * (Azure). Daemons reference a cluster via `daemon.cluster(...)` and (when
 * the cluster has more than one capacity group) a specific group via
 * `daemon.pool(...)`.
 *
 * When omitted from the stack, the preflight auto-generates a ComputeCluster
 * with a single "general" capacity group sized for the workloads in scope.
 * Declare one explicitly when you need hardware constraints — nested
 * virtualization, GPUs, specific instance families — or to pin sizes for
 * cost control.
 */
export class ComputeCluster {
  private _config: Partial<ComputeClusterConfig> = {
    capacityGroups: [],
  }

  constructor(id: string) {
    this._config.id = id
  }

  /**
   * Returns the resource type for permission targets that apply to all
   * compute-cluster resources.
   */
  public static any(): ResourceType {
    return "compute-cluster"
  }

  /**
   * Adds a capacity group to the cluster.
   *
   * @param groupId Unique identifier for the group within the cluster.
   * @param config Group-level config: instance type, profile, sizing, and
   *   the `nestedVirtualization` flag.
   */
  public capacityGroup(groupId: string, config: CapacityGroupInput): this {
    const { minSize = 1, maxSize = 1, ...rest } = config
    this._config.capacityGroups!.push({
      groupId,
      minSize,
      maxSize,
      ...rest,
    })
    return this
  }

  /**
   * Sets the container CIDR block used for inter-container networking inside
   * the cluster. Each machine gets a /24 subnet carved from this range.
   * Defaults to 10.244.0.0/16 if not specified.
   */
  public containerCidr(cidr: string): this {
    this._config.containerCidr = cidr
    return this
  }

  /**
   * Builds and validates the cluster configuration.
   */
  public build(): Resource {
    const config = ComputeClusterSchema.parse(this._config)
    return new Resource({
      type: "compute-cluster",
      ...config,
    })
  }
}
