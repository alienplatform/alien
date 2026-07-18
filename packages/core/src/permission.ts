import type { StackInputRef } from "./input.js"

const gatedPermissionSymbol = Symbol("alien.gatedPermission")

/**
 * A permission-set reference gated on a stack input's resolved value.
 * Created with permission(id).enabled(input); consumed by Stack.permissions(),
 * which splits it into a plain profile entry plus a deploy-time gate.
 */
export interface GatedPermission {
  readonly [gatedPermissionSymbol]: true
  readonly id: string
  readonly inputId: string
  readonly enabledValue: string
}

export interface PermissionRef {
  /**
   * Keep this permission set only while the input resolves to the given value.
   * @param input The gating stack input (must declare an env mapping).
   * @param whenValue Value that keeps the permission set included. Defaults to true.
   */
  enabled(input: StackInputRef, whenValue?: string | boolean | number): GatedPermission
}

/**
 * Reference a permission set by ID for gating (e.g., permission("queue/data-write")).
 * Ungated references stay plain strings in the profile.
 */
export function permission(id: string): PermissionRef {
  return {
    enabled(input: StackInputRef, whenValue: string | boolean | number = true): GatedPermission {
      return {
        [gatedPermissionSymbol]: true,
        id,
        inputId: input.id,
        enabledValue: String(whenValue),
      }
    },
  }
}

export function isGatedPermission(value: unknown): value is GatedPermission {
  return typeof value === "object" && value !== null && gatedPermissionSymbol in value
}

/**
 * Marker a resource builder stashes when `.enabled(input)` is called. Stack
 * build lowers it into permission gates on the sets the profiles grant for that
 * resource, so gating a resource is sugar over gating its permission grants.
 */
export const resourceGateSymbol = Symbol("alien.resourceGate")

export interface ResourceGate {
  readonly inputId: string
}

/**
 * Stash a resource gate on a built resource's config so `Stack.build()` can
 * lower it. A no-op when the resource is not gated; the symbol is non-enumerable
 * so it never reaches the serialized config — it is consumed at build time.
 */
export function applyResourceGate(config: object, gate: ResourceGate | undefined): void {
  if (gate) {
    Object.defineProperty(config, resourceGateSymbol, {
      value: gate,
      enumerable: false,
    })
  }
}
