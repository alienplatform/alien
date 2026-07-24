/**
 * Parsing for the `ALIEN_<NAME>_BINDING` env var an `ai` resource projects. The client only
 * routes on the binding: a BYO-key (`external`) binding is validated strictly here because the
 * client itself uses its fields, while every other service tag — the ambient variants, including
 * ones added to the platform after this SDK shipped — is passed through for the Rust gateway to
 * validate and serve. Mirrors the Rust `AiBinding` (serde tag "service", lowercase, camelCase
 * fields).
 */

import { AlienError, InvalidBindingConfigError } from "@alienplatform/core"
import * as z from "zod/v4"

// Strict: the external binding is the control-plane → workload trust boundary the client
// reads directly, so an unexpected key fails loudly rather than being silently dropped.
const externalAiBindingSchema = z
  .object({
    service: z.literal("external"),
    provider: z.string(),
    apiKey: z.string(),
  })
  .strict()

export type ExternalAiBinding = z.infer<typeof externalAiBindingSchema>

/** A binding served by the gateway; `service` may be a variant this SDK predates. */
export interface AmbientAiBinding {
  service: string
  [key: string]: unknown
}

export type AiBinding = ExternalAiBinding | AmbientAiBinding

/** Narrow an `AiBinding` to the BYO-key variant the client handles itself. */
export function isExternalAiBinding(binding: AiBinding): binding is ExternalAiBinding {
  return binding.service === "external"
}

/** The env var an `ai` binding is projected into: `ALIEN_<NAME>_BINDING` (uppercased, `-`→`_`). */
export function aiBindingEnvVarName(name: string): string {
  return `ALIEN_${name.toUpperCase().replace(/-/g, "_")}_BINDING`
}

/** Parse `ALIEN_<NAME>_BINDING`, or `undefined` if it is not set. Throws on malformed content. */
export async function parseAiBinding(name: string): Promise<AiBinding | undefined> {
  const raw = process.env[aiBindingEnvVarName(name)]
  if (!raw) return undefined
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch (cause) {
    throw (await AlienError.from(cause)).withContext(
      InvalidBindingConfigError.create({ message: `AI binding '${name}' is not valid JSON` }),
    )
  }
  const service =
    parsed !== null && typeof parsed === "object"
      ? (parsed as { service?: unknown }).service
      : undefined
  if (typeof service !== "string") {
    throw new AlienError(
      InvalidBindingConfigError.create({
        message: `AI binding '${name}' has no 'service' tag`,
      }),
    )
  }
  if (service !== "external") {
    return parsed as AmbientAiBinding
  }
  const result = externalAiBindingSchema.safeParse(parsed)
  if (!result.success) {
    throw (await AlienError.from(result.error)).withContext(
      InvalidBindingConfigError.create({ message: `AI binding '${name}' has an unexpected shape` }),
    )
  }
  return result.data
}
