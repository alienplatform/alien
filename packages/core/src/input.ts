import type {
  Platform,
  StackInputDefaultValue,
  StackInputDefinition,
  StackInputEnvironmentMapping,
  StackInputEnvironmentVariableType,
  StackInputKind,
  StackInputProvider,
  StackInputSetupMethod,
  StackInputValidation,
} from "./generated/index.js"

const stackInputDraftSymbol = Symbol("alien.stackInputDraft")
const stackInputDefinitionsSymbol = Symbol("alien.stackInputDefinitions")

export type StackInputValue = string | number | boolean | string[]

type OneOrMany<T> = T | readonly T[]

export type StackInputEnvMapping =
  | string
  | {
      name: string
      targetResources?: readonly string[]
      type?: StackInputEnvironmentVariableType
    }

interface CommonInputOptions<TDefault extends StackInputValue> {
  providedBy: OneOrMany<StackInputProvider>
  required: boolean
  label: string
  description: string
  placeholder?: string
  default?: TDefault
  platforms?: readonly Platform[]
  setupMethods?: readonly StackInputSetupMethod[]
  env?: OneOrMany<StackInputEnvMapping>
}

export interface StringInputOptions extends CommonInputOptions<string> {
  minLength?: number
  maxLength?: number
  pattern?: string
  format?: string
}

export interface SecretInputOptions extends Omit<CommonInputOptions<string>, "default"> {
  minLength?: number
  maxLength?: number
  pattern?: string
  format?: string
}

export interface NumberInputOptions extends CommonInputOptions<number> {
  min?: number
  max?: number
}

export interface IntegerInputOptions extends CommonInputOptions<number> {
  min?: number
  max?: number
}

export interface BooleanInputOptions extends CommonInputOptions<boolean> {}

export interface EnumInputOptions extends CommonInputOptions<string> {
  placeholder?: string
}

export interface StringListInputOptions extends CommonInputOptions<string[]> {
  minItems?: number
  maxItems?: number
}

export interface StackInputRef<TValue extends StackInputValue = StackInputValue> {
  readonly id: string
  readonly kind: StackInputKind
  readonly __value?: TValue
}

export type StackInputCollection = {
  readonly [stackInputDefinitionsSymbol]: readonly StackInputDefinition[]
}

interface StackInputDraft<TValue extends StackInputValue = StackInputValue> {
  readonly [stackInputDraftSymbol]: true
  readonly kind: StackInputKind
  readonly options: CommonInputOptions<TValue>
  readonly validation: StackInputValidation
}

export type StackInputSet<T extends Record<string, StackInputDraft>> = {
  readonly [K in keyof T]: T[K] extends StackInputDraft<infer TValue>
    ? StackInputRef<TValue>
    : never
} & StackInputCollection

export function inputs<const T extends Record<string, StackInputDraft>>(
  drafts: T,
): StackInputSet<T> {
  const result = {} as Record<string, StackInputRef>
  const definitions: StackInputDefinition[] = []

  for (const [id, draft] of Object.entries(drafts)) {
    validateInputId(id)
    validateDraft(id, draft)

    result[id] = { id, kind: draft.kind }
    definitions.push({
      id,
      kind: draft.kind,
      providedBy: normalizeArray(draft.options.providedBy),
      required: draft.options.required,
      label: draft.options.label,
      description: draft.options.description,
      placeholder: draft.options.placeholder,
      default: toDefaultValue(draft.kind, draft.options.default),
      platforms: draft.options.platforms ? [...draft.options.platforms] : undefined,
      setupMethods: draft.options.setupMethods ? [...draft.options.setupMethods] : undefined,
      validation: Object.keys(draft.validation).length > 0 ? draft.validation : undefined,
      env: normalizeEnv(draft.options.env),
    })
  }

  Object.defineProperty(result, stackInputDefinitionsSymbol, {
    value: Object.freeze(definitions),
    enumerable: false,
  })

  return result as StackInputSet<T>
}

export function getStackInputDefinitions(
  value: StackInputCollection | readonly StackInputDefinition[],
): StackInputDefinition[] {
  if (Array.isArray(value)) {
    return [...(value as readonly StackInputDefinition[])]
  }

  return [...(value as StackInputCollection)[stackInputDefinitionsSymbol]]
}

function defineInput<TValue extends StackInputValue>(
  kind: StackInputKind,
  options: CommonInputOptions<TValue>,
  validation: StackInputValidation = {},
): StackInputDraft<TValue> {
  return {
    [stackInputDraftSymbol]: true,
    kind,
    options,
    validation,
  }
}

function defineStringInput(options: StringInputOptions): StackInputDraft<string> {
  return defineInput("string", options, {
    minLength: options.minLength,
    maxLength: options.maxLength,
    pattern: options.pattern,
    format: options.format,
  })
}

function defineSecretInput(options: SecretInputOptions): StackInputDraft<string> {
  return defineInput("secret", options, {
    minLength: options.minLength,
    maxLength: options.maxLength,
    pattern: options.pattern,
    format: options.format,
  })
}

function defineNumberInput(options: NumberInputOptions): StackInputDraft<number> {
  return defineInput("number", options, {
    min: numberToString(options.min),
    max: numberToString(options.max),
  })
}

function defineIntegerInput(options: IntegerInputOptions): StackInputDraft<number> {
  return defineInput("integer", options, {
    min: integerToString(options.min, "min"),
    max: integerToString(options.max, "max"),
  })
}

function defineBooleanInput(options: BooleanInputOptions): StackInputDraft<boolean> {
  return defineInput("boolean", options)
}

function defineEnumInput<const TValues extends readonly [string, ...string[]]>(
  values: TValues,
  options: EnumInputOptions,
): StackInputDraft<TValues[number]> {
  return defineInput("enum", options, {
    values: [...values],
  }) as StackInputDraft<TValues[number]>
}

function defineStringListInput(options: StringListInputOptions): StackInputDraft<string[]> {
  return defineInput("stringList", options, {
    minItems: options.minItems,
    maxItems: options.maxItems,
  })
}

function normalizeArray<T>(value: OneOrMany<T>): T[] {
  return Array.isArray(value) ? [...(value as readonly T[])] : [value as T]
}

function normalizeEnv(
  value: OneOrMany<StackInputEnvMapping> | undefined,
): StackInputEnvironmentMapping[] {
  if (!value) {
    return []
  }

  return normalizeArray(value).map(mapping => {
    if (typeof mapping === "string") {
      return { name: mapping }
    }

    return {
      name: mapping.name,
      targetResources: mapping.targetResources ? [...mapping.targetResources] : undefined,
      type: mapping.type,
    }
  })
}

function toDefaultValue(
  kind: StackInputKind,
  value: StackInputValue | undefined,
): StackInputDefaultValue | undefined {
  if (value === undefined) {
    return undefined
  }

  switch (kind) {
    case "string":
    case "enum":
      return { type: "string", value: String(value) }
    case "number":
      return { type: "number", value: numberToString(value as number) ?? "" }
    case "integer":
      return { type: "number", value: integerToString(value as number, "default") ?? "" }
    case "boolean":
      return { type: "boolean", value: Boolean(value) }
    case "stringList":
      return { type: "stringList", value: [...(value as string[])] }
    case "secret":
      throw new Error("Secret stack inputs cannot declare a default value")
  }
}

function validateDraft(id: string, draft: StackInputDraft): void {
  if (!draft || draft[stackInputDraftSymbol] !== true) {
    throw new Error(
      `Stack input '${id}' must be created with alien.string(), alien.secret(), or another input helper`,
    )
  }

  if (normalizeArray(draft.options.providedBy).length === 0) {
    throw new Error(`Stack input '${id}' must include at least one providedBy value`)
  }

  if (!draft.options.label.trim()) {
    throw new Error(`Stack input '${id}' must include a label`)
  }

  if (draft.options.required && !draft.options.description.trim()) {
    throw new Error(`Stack input '${id}' must include a description when required is true`)
  }

  if (
    draft.options.setupMethods &&
    !normalizeArray(draft.options.providedBy).includes("deployer")
  ) {
    throw new Error(`Stack input '${id}' setupMethods require providedBy to include deployer`)
  }

  validateValidation(id, draft.kind, draft.validation)

  for (const mapping of normalizeEnv(draft.options.env)) {
    validateEnvName(id, mapping.name)
    if (mapping.targetResources?.length === 0) {
      throw new Error(
        `Stack input '${id}' env mapping '${mapping.name}' cannot use an empty targetResources list`,
      )
    }
  }
}

function validateInputId(id: string): void {
  if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(id)) {
    throw new Error(
      `Stack input '${id}' id must start with a letter or underscore and contain only letters, digits, and underscores`,
    )
  }

  if (!/[a-z]/.test(id)) {
    throw new Error(`Stack input '${id}' id must not be all-caps env-var style`)
  }
}

function validateEnvName(id: string, name: string): void {
  if (!/^[A-Z_][A-Z0-9_]*$/.test(name)) {
    throw new Error(
      `Stack input '${id}' env name '${name}' must use uppercase letters, digits, and underscores`,
    )
  }
}

function validateValidation(
  id: string,
  kind: StackInputKind,
  validation: StackInputValidation,
): void {
  const minLength = validation.minLength ?? undefined
  const maxLength = validation.maxLength ?? undefined
  const minItems = validation.minItems ?? undefined
  const maxItems = validation.maxItems ?? undefined

  if (minLength !== undefined && maxLength !== undefined && minLength > maxLength) {
    throw new Error(`Stack input '${id}' minLength must be less than or equal to maxLength`)
  }

  if (minItems !== undefined && maxItems !== undefined && minItems > maxItems) {
    throw new Error(`Stack input '${id}' minItems must be less than or equal to maxItems`)
  }

  if (kind === "enum" && (!validation.values || validation.values.length === 0)) {
    throw new Error(`Stack input '${id}' enum values must not be empty`)
  }

  if (validation.pattern) {
    validatePortablePattern(id, validation.pattern)
  }
}

function validatePortablePattern(id: string, pattern: string): void {
  if (pattern.length === 0) {
    throw new Error(`Stack input '${id}' pattern must not be empty`)
  }

  if (pattern.startsWith("/") && pattern.endsWith("/") && pattern.length > 1) {
    throw new Error(`Stack input '${id}' pattern must not use regex delimiters`)
  }

  if (/\\[1-9pPk]/.test(pattern) || /\(\?/.test(pattern) || /\[[^\]]*\[/.test(pattern)) {
    throw new Error(
      `Stack input '${id}' pattern is not portable across TypeScript, CloudFormation, and Terraform`,
    )
  }
}

function numberToString(value: number | undefined): string | undefined {
  if (value === undefined) {
    return undefined
  }

  if (!Number.isFinite(value)) {
    throw new Error("Stack input number constraints must be finite")
  }

  return String(value)
}

function integerToString(value: number | undefined, field: string): string | undefined {
  if (value === undefined) {
    return undefined
  }

  if (!Number.isInteger(value)) {
    throw new Error(`Stack input integer ${field} must be an integer`)
  }

  return String(value)
}

export {
  defineBooleanInput as boolean,
  defineEnumInput as enum,
  defineIntegerInput as integer,
  defineNumberInput as number,
  defineSecretInput as secret,
  defineStringInput as string,
  defineStringListInput as stringList,
}
