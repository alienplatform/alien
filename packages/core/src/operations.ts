import type {
  OperationApproval,
  OperationSettingValue,
  OperationsConfig,
  PluginOperationsConfig,
} from "./generated/index.js"
import type { StackInputRef } from "./input.js"
import { Resource } from "./resource.js"

export type {
  CustomPluginOperationsConfig,
  OperationApproval,
  OperationApprovalDecision,
  OperationSettingValue,
  OperationsConfig,
  PluginOperationsConfig,
} from "./generated/index.js"
export { OperationsConfigSchema } from "./generated/index.js"

/** A plugin setting: a literal, a stack input, or stack resources such as buckets. */
export type OperationSettingInput = string | StackInputRef | Resource | readonly Resource[]

/** Approval rules by operation pattern: an operation name, `*`, or a prefix ending in `*`. */
export type OperationApprovalInput = Record<string, OperationApproval>

/** One plugin's settings, plus its approval rules under `approval`. */
export interface PluginOperationsInput {
  approval?: OperationApprovalInput
  [setting: string]: OperationSettingInput | OperationApprovalInput | undefined
}

/** A published custom plugin: `"name@version"`, or the same with settings and approval. */
export type CustomPluginOperationsInput =
  | string
  | ({ name: string; version: string } & PluginOperationsInput)

/** Built-in plugins by name, and published custom plugins under `plugins`. */
export interface OperationsInput {
  plugins?: readonly CustomPluginOperationsInput[]
  [plugin: string]: PluginOperationsInput | readonly CustomPluginOperationsInput[] | undefined
}

/** Convert the `alien.ts` form into the stack's operations config. */
export function toOperationsConfig(input: OperationsInput): OperationsConfig {
  const { plugins: custom = [], ...builtins } = input
  const config: OperationsConfig = {}
  const entries = Object.entries(builtins).filter(([, value]) => value !== undefined)
  if (entries.length > 0) {
    config.plugins = Object.fromEntries(
      entries.map(([name, value]) => [name, toPluginConfig(name, value as PluginOperationsInput)]),
    )
  }
  if (custom.length > 0) {
    config.custom = custom.map(toCustomPlugin)
  }
  if (!config.plugins && !config.custom) {
    throw new Error("operations() needs at least one plugin")
  }
  return config
}

function toCustomPlugin(entry: CustomPluginOperationsInput) {
  if (typeof entry === "string") {
    const at = entry.lastIndexOf("@")
    if (at <= 0 || at === entry.length - 1) {
      throw new Error(`Custom plugin '${entry}' must be written as 'name@version'`)
    }
    return { name: entry.slice(0, at), version: entry.slice(at + 1) }
  }
  const { name, version, ...rest } = entry
  return { name, version, ...toPluginConfig(name, rest) }
}

function toPluginConfig(plugin: string, input: PluginOperationsInput): PluginOperationsConfig {
  const { approval, ...settings } = input
  const config: PluginOperationsConfig = {}
  const values = Object.entries(settings).filter(([, value]) => value !== undefined)
  if (values.length > 0) {
    config.settings = Object.fromEntries(
      values.map(([key, value]) => [
        key,
        toSettingValue(plugin, key, value as OperationSettingInput),
      ]),
    )
  }
  if (approval && Object.keys(approval).length > 0) {
    config.approval = approval
  }
  return config
}

function toSettingValue(
  plugin: string,
  key: string,
  value: OperationSettingInput,
): OperationSettingValue {
  if (typeof value === "string") return value
  if (value instanceof Resource) return { resources: [value.config.id] }
  if (Array.isArray(value)) {
    return {
      resources: value.map(resource => {
        if (!(resource instanceof Resource)) {
          throw new Error(`Setting '${plugin}.${key}' lists something that is not a resource`)
        }
        return resource.config.id
      }),
    }
  }
  if (typeof value === "object" && value !== null && "id" in value && "kind" in value) {
    return { input: (value as StackInputRef).id }
  }
  throw new Error(`Setting '${plugin}.${key}' must be a string, a stack input, or stack resources`)
}
