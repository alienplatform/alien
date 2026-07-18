import { PermissionGateInvalidError } from "./common-errors.js"
import { AlienError } from "./error.js"
import {
  type PermissionGate,
  type PermissionSetReference,
  type PermissionsConfig,
  type Platform,
  type ResourceEntry,
  type ResourceLifecycle,
  type Stack as StackConfig,
  type StackInputDefinition,
  StackSchema,
} from "./generated/index.js"
import { type StackInputCollection, getStackInputDefinitions } from "./input.js"
import {
  type GatedPermission,
  type ResourceGate,
  isGatedPermission,
  resourceGateSymbol,
} from "./permission.js"
import type { Resource } from "./resource.js"

/** Wrap a permission-gate validation failure in an AlienError with a stable code. */
function gateError(reason: string) {
  return new AlienError(PermissionGateInvalidError.create({ reason }))
}

/**
 * Options for adding a resource to a stack.
 */
export interface AddResourceOptions {
  /**
   * Enable remote bindings for this resource (BYOB use case).
   * When true, binding params are synced to StackState for external access.
   * Default: false (prevents sensitive data in synced state).
   */
  remoteAccess?: boolean
}

export type {
  Stack as StackConfig,
  StackState,
  StackStatus,
  StackResourceState,
  ResourceStatus,
  PermissionSet,
  ManagementPermissions,
  PermissionsConfig,
  StackInputDefaultValue,
  StackInputDefinition,
  StackInputEnvironmentMapping,
  StackInputEnvironmentVariableType,
  StackInputKind,
  StackInputProvider,
  StackInputValidation,
} from "./generated/index.js"
export {
  StackSchema,
  StackStateSchema,
  StackStatusSchema,
  StackResourceStateSchema,
  ResourceStatusSchema,
} from "./generated/index.js"

/**
 * Represents a collection of cloud resources that are managed together.
 * Stacks are the top-level organizational unit in an Alien application.
 */
export class Stack {
  private _config: Partial<StackConfig> = {
    resources: {},
    permissions: undefined,
  }

  /**
   * Creates a new Stack builder.
   * @param id Identifier for the stack. Must contain only alphanumeric characters, hyphens, and underscores ([A-Za-z0-9-_]). Maximum 128 characters.
   */
  constructor(id: string) {
    this._config.id = id
  }

  /**
   * Adds a resource to the stack with a specified lifecycle.
   * @param resource The resource to add (e.g., Worker, Storage).
   * @param lifecycle The lifecycle state of the resource (e.g., Frozen, Live).
   * @param options Optional configuration for the resource entry.
   * @returns The Stack builder instance.
   */
  public add(resource: Resource, lifecycle: ResourceLifecycle, options?: AddResourceOptions): this {
    const entry: ResourceEntry = {
      // Keep the resource's config object by reference: a `.enabled()` marker
      // rides on it as a non-enumerable symbol that `lowerResourceGates()` reads
      // at build time. Cloning it here would silently drop the gate.
      config: resource.config,
      lifecycle,
      dependencies: [], // Additional dependencies beyond what the resource itself defines
    }
    if (options?.remoteAccess) {
      entry.remoteAccess = true
    }
    this._config.resources![resource.config.id] = entry
    return this
  }

  /**
   * Declare which platforms this stack supports.
   * When omitted, the stack supports all platforms.
   */
  public platforms(platforms: Platform[]): this {
    this._config.supportedPlatforms = platforms
    return this
  }

  /**
   * Configure permissions for this stack.
   * Profile entries may be gated with permission(id).enabled(input).
   * @param config Permission configuration
   * @returns The Stack builder instance.
   */
  public permissions(config: PermissionsConfigInput): this {
    // A gated ref is split into a plain profile entry plus a PermissionGate;
    // the deploy-time mutation re-applies the gate once inputs are resolved.
    const gates: PermissionGate[] = []
    const profiles: PermissionsConfig["profiles"] = {}
    for (const [profileName, profile] of Object.entries(config.profiles)) {
      const resources: Record<string, PermissionSetReference[]> = {}
      for (const [resourceKey, references] of Object.entries(profile)) {
        resources[resourceKey] = references.map(reference => {
          if (isGatedPermission(reference)) {
            gates.push({
              profile: profileName,
              resource: resourceKey,
              permissionSetId: reference.id,
              inputId: reference.inputId,
              enabledValue: reference.enabledValue,
            })
            return reference.id
          }
          return reference
        })
      }
      profiles[profileName] = resources
    }

    this._config.permissions = {
      profiles,
      ...(config.management !== undefined ? { management: config.management } : {}),
      ...(gates.length > 0 ? { gates } : {}),
    }
    return this
  }

  /**
   * Configure values that must be provided before setup or deployment can proceed.
   * @param inputs Stack input definitions created with alien.inputs({...}).
   * @returns The Stack builder instance.
   */
  public inputs(inputs: StackInputCollection | readonly StackInputDefinition[]): this {
    this._config.inputs = getStackInputDefinitions(inputs)
    return this
  }

  /**
   * Gets the stack ID without building/validating the stack.
   * @returns The stack ID.
   */
  public get id(): string {
    return this._config.id!
  }

  /**
   * Builds and validates the stack configuration.
   * @returns The complete and validated stack configuration.
   * @throws Error if the stack configuration is invalid.
   */
  public build(): StackConfig {
    this.lowerResourceGates()
    this.validatePermissionGates()
    return StackSchema.parse(this._config)
  }

  /**
   * Lower each resource's `.enabled(input)` marker into permission gates on the
   * sets a profile grants for that resource — its own key, or the wildcard `"*"`
   * key when it is unambiguous — matched by the resource's type prefix. A
   * resource's cloud footprint on the setup surface is its IAM, so gating those
   * grants is what turns it off; the synthesized gates then run through the same
   * validation as hand-written ones.
   */
  private lowerResourceGates(): void {
    const resources = this._config.resources ?? {}

    // Resources marked with `.enabled()`, keyed to their gating input.
    const gateInput = new Map<string, string>()
    for (const [resourceId, entry] of Object.entries(resources)) {
      const marker = (entry.config as { [resourceGateSymbol]?: ResourceGate })[resourceGateSymbol]
      if (marker) {
        gateInput.set(resourceId, marker.inputId)
      }
    }
    if (gateInput.size === 0) {
      return
    }

    const profiles = this._config.permissions?.profiles ?? {}
    for (const [resourceId, inputId] of gateInput) {
      const type = resources[resourceId]!.config.type
      const prefix = `${type}/`
      // A set granted under the wildcard `"*"` key is shared by every resource
      // of this type, so gating it from one resource's marker would silently
      // gate the others' grant too. Only lower a wildcard grant when every
      // resource of this type is gated on the same input (the common single
      // resource of its type is the safe case); otherwise fail loudly.
      const wildcardIsUnambiguous = Object.entries(resources)
        .filter(([, entry]) => entry.config.type === type)
        .every(([id]) => gateInput.get(id) === inputId)

      let gated = 0
      for (const [profileName, profileResources] of Object.entries(profiles)) {
        for (const resourceKey of [resourceId, "*"]) {
          for (const reference of profileResources[resourceKey] ?? []) {
            const setId = typeof reference === "string" ? reference : reference.id
            // Only the resource's own data/access sets are gated; provisioning
            // stays put so the resource can still be created or torn down.
            if (!setId.startsWith(prefix) || setId.endsWith("/provision")) {
              continue
            }
            if (resourceKey === "*" && !wildcardIsUnambiguous) {
              throw gateError(
                `Resource "${resourceId}" gates "${setId}", which its profile grants under the wildcard "*" shared with other ${type} resources that are not gated on the same input. Gate each resource on the same input, or grant the set under the resource's own key so the gate is unambiguous.`,
              )
            }
            this._config.permissions!.gates ??= []
            const gates = this._config.permissions!.gates
            const gate: PermissionGate = {
              profile: profileName,
              resource: resourceKey,
              permissionSetId: setId,
              inputId,
              enabledValue: "true",
            }
            const existing = gates.find(
              g =>
                g.profile === gate.profile &&
                g.resource === gate.resource &&
                g.permissionSetId === gate.permissionSetId,
            )
            // A grant can be gated on only one input: the emitter's gate lookup
            // keeps only the first match while the deploy-time mutation applies
            // every gate, so anything but an identical gate on the same
            // profile/resource/set must fail.
            if (
              existing &&
              (existing.inputId !== gate.inputId || existing.enabledValue !== gate.enabledValue)
            ) {
              throw gateError(
                `Resource "${resourceId}" gates "${setId}" on input "${inputId}" = "${gate.enabledValue}", but a gate for the same profile/resource/set already targets input "${existing.inputId}" = "${existing.enabledValue}". A grant can be gated on only one input.`,
              )
            }
            if (!existing) {
              gates.push(gate)
            }
            gated += 1
          }
        }
      }
      if (gated === 0) {
        throw gateError(
          `Resource "${resourceId}" is gated with .enabled(), but no profile grants a "${prefix}" permission set for it, so the gate would do nothing.`,
        )
      }
    }
  }

  /**
   * Gate values reach the deployment engine through the input's env mapping,
   * so a gate on an env-less or undeclared input would silently never apply.
   */
  private validatePermissionGates(): void {
    const gates = this._config.permissions?.gates
    if (!gates || gates.length === 0) {
      return
    }

    const inputs = this._config.inputs ?? []
    for (const gate of gates) {
      const input = inputs.find(candidate => candidate.id === gate.inputId)
      if (!input) {
        throw gateError(
          `Permission gate on "${gate.permissionSetId}" references input "${gate.inputId}", which is not declared on the stack. Pass it via stack.inputs(...).`,
        )
      }
      if (!input.providedBy?.includes("deployer")) {
        throw gateError(
          `Permission gate on "${gate.permissionSetId}" targets input "${gate.inputId}", which the deployer cannot set (its providedBy does not include "deployer"); the gate would never apply.`,
        )
      }
      // A platform-scoped input only applies where its platforms list allows,
      // so the emitter would render the grant unconditionally on any target the
      // stack supports but the input excludes.
      const stackPlatforms = this._config.supportedPlatforms
      if (input.platforms && stackPlatforms) {
        const uncovered = stackPlatforms.filter(p => !input.platforms!.includes(p))
        if (uncovered.length > 0) {
          throw gateError(
            `Permission gate on "${gate.permissionSetId}" targets input "${gate.inputId}", which is restricted to platforms [${input.platforms.join(", ")}] and does not cover the stack's [${uncovered.join(", ")}], where the gate would not apply.`,
          )
        }
      }
      if (!input.env || input.env.length === 0) {
        throw gateError(
          `Permission gate on "${gate.permissionSetId}" requires input "${gate.inputId}" to declare an env mapping.`,
        )
      }
      // The gate is an equality test against the input's resolved value, so the
      // input's kind must be able to drive it and the value must fall in its
      // domain — otherwise the grant silently emits unconditionally or never.
      const value = gate.enabledValue
      if (input.kind === "secret" || input.kind === "stringList") {
        throw gateError(
          `Permission gate on "${gate.permissionSetId}" cannot gate on input "${gate.inputId}" of kind "${input.kind}"; gate on a string, boolean, number, integer, or enum input.`,
        )
      }
      if (input.kind === "boolean" && value !== "true" && value !== "false") {
        throw gateError(
          `Permission gate on "${gate.permissionSetId}" targets boolean input "${gate.inputId}" but its enabled value is "${value}"; pass true or false to .enabled().`,
        )
      }
      if (input.kind === "number" || input.kind === "integer") {
        const parsed = Number(value.trim())
        if (value.trim() === "" || Number.isNaN(parsed)) {
          throw gateError(
            `Permission gate on "${gate.permissionSetId}" targets ${input.kind} input "${gate.inputId}" but its enabled value "${value}" is not a ${input.kind}.`,
          )
        }
        if (input.kind === "integer" && !Number.isInteger(parsed)) {
          throw gateError(
            `Permission gate on "${gate.permissionSetId}" targets integer input "${gate.inputId}" but its enabled value "${value}" is not an integer.`,
          )
        }
      }
      if (input.kind === "enum") {
        const values = input.validation?.values ?? []
        if (values.length > 0 && !values.includes(value)) {
          throw gateError(
            `Permission gate on "${gate.permissionSetId}" targets enum input "${gate.inputId}" with enabled value "${value}", which is not one of [${values.join(", ")}].`,
          )
        }
      }
    }

    // Same one-gate-per-triple rule as lowerResourceGates, here for gates written
    // straight into the profile: the emitter keeps only the first match while the
    // mutation applies every gate, so divergent gates split setup from deploy.
    const seenGate = new Map<string, PermissionGate>()
    for (const gate of gates) {
      const key = JSON.stringify([gate.profile, gate.resource, gate.permissionSetId])
      const prev = seenGate.get(key)
      if (prev && (prev.inputId !== gate.inputId || prev.enabledValue !== gate.enabledValue)) {
        throw gateError(
          `Permission set "${gate.permissionSetId}" in profile "${gate.profile}" has two conflicting gates on "${gate.resource}": input "${prev.inputId}" = "${prev.enabledValue}" and input "${gate.inputId}" = "${gate.enabledValue}". A grant can be gated on only one input.`,
        )
      }
      if (!prev) {
        seenGate.set(key, gate)
      }
    }

    // A set granted under both a resource key and the wildcard "*" is emitted
    // with both origin keys, and the setup emitter falls back to an unconditional
    // grant unless both carry the same gate. So a set gated on one key but
    // ungated or divergently gated on the other is a silent fail-open. The
    // resource-level .enabled() lowering never produces this (same input on every
    // key it gates), but a hand-written gate can.
    const profiles = this._config.permissions?.profiles ?? {}
    const gateFor = (profile: string, resource: string, setId: string) =>
      gates.find(
        g => g.profile === profile && g.resource === resource && g.permissionSetId === setId,
      )
    for (const [profileName, profileResources] of Object.entries(profiles)) {
      const wildcard = profileResources["*"]
      if (!wildcard) {
        continue
      }
      const wildcardIds = new Set(wildcard.map(r => (typeof r === "string" ? r : r.id)))
      for (const [resourceKey, references] of Object.entries(profileResources)) {
        if (resourceKey === "*") {
          continue
        }
        for (const reference of references) {
          const setId = typeof reference === "string" ? reference : reference.id
          if (!wildcardIds.has(setId)) {
            continue
          }
          const resourceGate = gateFor(profileName, resourceKey, setId)
          const wildcardGate = gateFor(profileName, "*", setId)
          const divergent =
            resourceGate && wildcardGate
              ? resourceGate.inputId !== wildcardGate.inputId ||
                resourceGate.enabledValue !== wildcardGate.enabledValue
              : Boolean(resourceGate) !== Boolean(wildcardGate)
          if (divergent) {
            throw gateError(
              `Permission set "${setId}" in profile "${profileName}" is granted under both resource "${resourceKey}" and the wildcard "*", but the two are gated differently; the setup emitter merges them and would emit the grant unconditionally. Gate both grants on the same input and value, or grant the set under only one key.`,
            )
          }
        }
      }
    }
  }
}

/**
 * Permission profile entries accepted by Stack.permissions():
 * plain references plus gated ones created with permission(id).enabled(input).
 */
export interface PermissionsConfigInput {
  profiles: Record<string, Record<string, (PermissionSetReference | GatedPermission)[]>>
  management?: PermissionsConfig["management"]
}
