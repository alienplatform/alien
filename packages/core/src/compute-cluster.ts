import {
  type ComputeCluster as ComputeClusterConfig,
  ComputeClusterSchema,
  type MachineProfile,
  type PermissionProfile,
  type Platform,
  type ResourceType,
} from "./generated/index.js"
import { type Resource, ResourceMetadataBuilder } from "./resource.js"

export type {
  CapacityGroup,
  CapacityGroupScalePolicy,
  ComputeChoiceRange as GeneratedComputeChoiceRange,
  ComputeCluster as ComputeClusterConfig,
  MachineProfile,
  PermissionProfile,
} from "./generated/index.js"
export {
  CapacityGroupScalePolicySchema,
  CapacityGroupSchema,
  ComputeChoiceRangeSchema,
  ComputeClusterSchema as ComputeClusterConfigSchema,
  MachineProfileSchema,
} from "./generated/index.js"

/**
 * Hardware requirements for a compute pool.
 * On Kubernetes, CPU, memory, and storage describe administrator-managed node
 * capacity. Architecture constrains Pod placement. GPU and nested virtualization
 * requirements are rejected because this resource cannot guarantee them there.
 */
export type ComputePoolRequirements = {
  cpu: number | string
  memory: string
  /**
   * Storage on the disk holding the operating system (default "20Gi"). On
   * AWS, GCP and Azure it sizes each machine's root disk; a change replaces
   * the machines one at a time without rerunning setup.
   */
  ephemeralStorage?: string
  architecture?: "arm64" | "x86_64"
  nestedVirtualization?: boolean
  accelerators?: Array<{
    type: string
    count: number
  }>
  /**
   * An additional empty block device each machine must provide, besides the
   * disk holding the operating system: no partitions, no filesystem, not
   * mounted, and at least `size`.
   *
   * `size` is either one size for every deployment (`"1Ti"`), or a range each
   * deployment chooses from: `{ min: "0Gi", default: "200Gi", max: "4Ti" }`.
   * A deployment picks its size in its compute settings (`dataDiskGib`), in
   * the CloudFormation template, or with `alien deployments set-compute
   * --data-disk`; without a pick it uses `default`. A range that starts at
   * "0Gi" lets a deployment run the pool without a data disk, for example
   * on hosts that have no spare disk. Range sizes are whole GiB.
   *
   * On AWS, GCP and Azure, Alien attaches one new disk to every machine and
   * deletes it with the machine: an encrypted gp3 EBS volume, a pd-balanced
   * persistent disk (at least 10 GiB), or a Premium SSD managed disk at LUN 0.
   * Sizes are rounded up to a whole GiB, up to 64 TiB on AWS and GCP and
   * 32,767 GiB on Azure. Changing the size replaces the machines one at a
   * time without rerunning setup; data on the old disks is not kept. On
   * machines deployments, `join` refuses a host without a large enough
   * empty disk. Kubernetes rejects it because Alien does not attach disks
   * to nodes.
   */
  dataDisk?: {
    size: string | DataDiskSizeRange
  }
}

/** Data disk sizes a deployment may choose from, as quantities such as "50Gi" or "1Ti". */
export type DataDiskSizeRange = {
  min: string
  default: string
  max: string
}

export type ComputeChoiceRange =
  | number
  | {
      min: number
      max: number
      default: number
    }

/** Machine counts and node autoscaling apply to cloud fleets. Kubernetes uses existing node capacity. */
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
  /** Allow containers created after installation to use this pool. Only one pool may opt in. */
  dynamicContainers?: boolean
  /** Cloud fleet failure-domain spread. Advisory on existing Kubernetes clusters. */
  failureDomainSpread?: number
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
 *
 * On Kubernetes, declare the same Frozen resource to describe logical pools in
 * the deployment namespace. Alien verifies namespace access and pool references,
 * then applies architecture constraints to static and dynamic container Pods.
 * Source builds use one architecture per stack. Workers and pools without an
 * explicit architecture inherit that build constraint for Pod scheduling.
 * It creates no nodes. Hardware sizes, machine counts, autoscaling, and requested
 * failure-domain spread remain the cluster administrator's responsibility.
 * Provider instance types, selected zones, custom container CIDRs, GPU, and
 * nested virtualization requirements are unsupported and fail validation.
 */
export class ComputeCluster extends ResourceMetadataBuilder {
  private _config: Partial<ComputeClusterConfig> = {
    capacityGroups: [],
  }

  constructor(id: string) {
    super()
    this._config.id = id
  }

  /**
   * Returns the resource type for permission targets that apply to all
   * compute-cluster resources.
   */
  public static any(): ResourceType {
    return "compute-cluster"
  }

  /** Grants the node identity access to explicitly named resources only. */
  public nodePermissions(
    permissions: PermissionProfile,
    options?: { platforms: Platform[] },
  ): this {
    this._config.nodePermissions = permissions
    this._config.nodePermissionsPlatforms = options?.platforms
    return this
  }

  public pool(groupId: string, config: ComputePoolInput): this {
    if (config.dynamicContainers) {
      if (this._config.dynamicContainerPool !== undefined) {
        throw new Error("Only one compute pool may accept dynamic containers")
      }
    }
    if (
      config.failureDomainSpread !== undefined &&
      (!Number.isInteger(config.failureDomainSpread) ||
        config.failureDomainSpread < 1 ||
        config.failureDomainSpread > 255)
    ) {
      throw new Error("Compute pool failureDomainSpread must be an integer from 1 to 255")
    }
    const { minSize, maxSize } = selectedScaleBounds(config.scale)
    const dataDiskGibRange = dataDiskRangeFromRequirements(config.requirements)
    this._config.capacityGroups!.push({
      groupId,
      profile: machineProfileFromRequirements(config.requirements, dataDiskGibRange),
      minSize,
      maxSize,
      scalePolicy: scalePolicyFromInput(config.scale),
      nestedVirtualization: config.requirements.nestedVirtualization,
      dataDiskGibRange,
    })
    if (config.failureDomainSpread !== undefined) {
      this._config.failureDomainSpread ??= {}
      this._config.failureDomainSpread[groupId] = config.failureDomainSpread
    }
    if (config.dynamicContainers) {
      this._config.dynamicContainerPool = groupId
    }
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
    if (
      this._config.dynamicContainerPool !== undefined &&
      !this._config.capacityGroups?.some(
        group => group.groupId === this._config.dynamicContainerPool,
      )
    ) {
      throw new Error("Dynamic container pool must be declared in the compute cluster")
    }
    const config = ComputeClusterSchema.parse(this._config)
    return this.resource({
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

function scalePolicyFromInput(
  scale: ComputePoolScale,
): ComputeClusterConfig["capacityGroups"][number]["scalePolicy"] {
  if (scale.type === "fixed") {
    return {
      type: "fixed",
      machines: choiceRange(scale.machines),
    }
  }

  return {
    type: "autoscale",
    min: choiceRange(scale.min),
    max: choiceRange(scale.max),
  }
}

function choiceRange(choice: ComputeChoiceRange): { min: number; max: number; default: number } {
  if (typeof choice === "number") {
    return { min: choice, max: choice, default: choice }
  }
  return choice
}

function defaultChoice(choice: ComputeChoiceRange): number {
  if (typeof choice === "number") {
    return choice
  }

  return choice.default
}

function dataDiskRangeFromRequirements(
  requirements: ComputePoolRequirements,
): { min: number; max: number; default: number } | undefined {
  const size = requirements.dataDisk?.size
  if (size === undefined || typeof size === "string") {
    return undefined
  }
  const range = {
    min: parseWholeGib("dataDisk.size.min", size.min),
    max: parseWholeGib("dataDisk.size.max", size.max),
    default: parseWholeGib("dataDisk.size.default", size.default),
  }
  if (!(range.min <= range.default && range.default <= range.max)) {
    throw new Error(
      `Compute pool dataDisk.size needs min <= default <= max: ${size.min}, ${size.default}, ${size.max}`,
    )
  }
  if (range.max === 0) {
    throw new Error("Compute pool dataDisk.size.max must be greater than zero")
  }
  return range
}

function parseWholeGib(field: string, value: string): number {
  const bytes = parseQuantityBytes(value)
  const gib = bytes / 1024 ** 3
  if (!Number.isInteger(gib)) {
    throw new Error(`Compute pool ${field} must be a whole number of GiB: ${value}`)
  }
  return gib
}

function machineProfileFromRequirements(
  requirements: ComputePoolRequirements,
  dataDiskGibRange: { default: number } | undefined,
): MachineProfile {
  const dataDisk = dataDiskFromRequirements(requirements, dataDiskGibRange)
  return {
    cpu: typeof requirements.cpu === "number" ? `${requirements.cpu}` : requirements.cpu,
    memoryBytes: parseQuantityBytes(requirements.memory),
    ephemeralStorageBytes: parseQuantityBytes(requirements.ephemeralStorage ?? "20Gi"),
    architecture: requirements.architecture,
    gpu: requirements.accelerators?.[0]
      ? {
          type: requirements.accelerators[0].type,
          count: requirements.accelerators[0].count,
        }
      : undefined,
    dataDisk,
  }
}

/** The declared size, or a range's default; a default of 0 means no data disk. */
function dataDiskFromRequirements(
  requirements: ComputePoolRequirements,
  dataDiskGibRange: { default: number } | undefined,
): MachineProfile["dataDisk"] {
  const size = requirements.dataDisk?.size
  if (size === undefined) {
    return undefined
  }
  if (dataDiskGibRange) {
    return dataDiskGibRange.default > 0
      ? { sizeBytes: dataDiskGibRange.default * 1024 ** 3 }
      : undefined
  }
  return { sizeBytes: parsePositiveQuantityBytes("dataDisk.size", size as string) }
}

function parsePositiveQuantityBytes(field: string, value: string): number {
  const bytes = parseQuantityBytes(value)
  if (bytes <= 0) {
    throw new Error(`Compute pool ${field} must be greater than zero: ${value}`)
  }
  return bytes
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
