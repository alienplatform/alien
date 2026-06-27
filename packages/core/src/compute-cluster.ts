import {
  type ComputeCluster as ComputeClusterConfig,
  ComputeClusterSchema,
  type MachineProfile,
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
 * Hardware requirements for a compute pool.
 */
export type ComputePoolRequirements = {
  cpu: number | string
  memory: string
  ephemeralStorage?: string
  architecture?: "arm64" | "x86_64"
  nestedVirtualization?: boolean
  accelerators?: Array<{
    type: string
    count: number
  }>
}

export type ComputeChoiceRange =
  | number
  | {
      min: number
      max: number
      default: number
    }

export type ComputePoolScale =
  | {
      type: "fixed"
      machines: ComputeChoiceRange
    }
  | {
      type: "autoscale"
      min: ComputeChoiceRange
      max: ComputeChoiceRange
    }

export type ComputePoolInput = {
  requirements: ComputePoolRequirements
  scale: ComputePoolScale
}

/**
 * Declares a ComputeCluster — the setup-owned machine boundary for daemons and
 * containers. Each capacity group inside the cluster becomes a separate
 * Auto Scaling Group (AWS), Managed Instance Group (GCP), or VM Scale Set
 * (Azure). Daemons reference a cluster via `daemon.cluster(...)` and (when
 * the cluster has more than one capacity group) a specific group via
 * `daemon.pool(...)`.
 *
 * Application source declares portable pool requirements. Provider machine
 * names are selected later through deployment settings.
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

  public pool(groupId: string, config: ComputePoolInput): this {
    const { minSize, maxSize } = selectedScaleBounds(config.scale)
    this._config.capacityGroups!.push({
      groupId,
      profile: machineProfileFromRequirements(config.requirements),
      minSize,
      maxSize,
      nestedVirtualization: config.requirements.nestedVirtualization,
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

function selectedScaleBounds(scale: ComputePoolScale): { minSize: number; maxSize: number } {
  if (scale.type === "fixed") {
    const machines = defaultChoice(scale.machines)
    return { minSize: machines, maxSize: machines }
  }

  return {
    minSize: defaultChoice(scale.min),
    maxSize: defaultChoice(scale.max),
  }
}

function defaultChoice(choice: ComputeChoiceRange): number {
  if (typeof choice === "number") {
    return choice
  }

  return choice.default
}

function machineProfileFromRequirements(requirements: ComputePoolRequirements): MachineProfile {
  return {
    cpu: typeof requirements.cpu === "number" ? `${requirements.cpu}` : requirements.cpu,
    memoryBytes: parseQuantityBytes(requirements.memory),
    ephemeralStorageBytes: parseQuantityBytes(requirements.ephemeralStorage ?? "20Gi"),
    gpu: requirements.accelerators?.[0]
      ? {
          type: requirements.accelerators[0].type,
          count: requirements.accelerators[0].count,
        }
      : undefined,
  }
}

function parseQuantityBytes(value: string): number {
  const match = value.match(/^([0-9]+(?:\.[0-9]+)?)(Ki|Mi|Gi|Ti|k|M|G|T)?$/)
  if (!match) {
    throw new Error(`Invalid memory/storage quantity: ${value}`)
  }

  const amount = Number(match[1])
  const suffix = match[2]
  const multiplier =
    suffix === "Ti"
      ? 1024 ** 4
      : suffix === "Gi"
        ? 1024 ** 3
        : suffix === "Mi"
          ? 1024 ** 2
          : suffix === "Ki"
            ? 1024
            : suffix === "T"
              ? 1000 ** 4
              : suffix === "G"
                ? 1000 ** 3
                : suffix === "M"
                  ? 1000 ** 2
                  : suffix === "k"
                    ? 1000
                    : 1

  return Math.round(amount * multiplier)
}
