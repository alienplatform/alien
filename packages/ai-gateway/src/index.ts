/**
 * `@alienplatform/ai-gateway` — the thin TypeScript wrapper that starts the Alien
 * AI gateway in-process.
 *
 * The gateway itself is a single Rust implementation (the `alien-gateway` crate)
 * compiled to a napi-rs addon. This package loads that addon and starts it once
 * per process, returning the loopback base URL. The app then points a plain
 * OpenAI-compatible client at that URL, and every request and SSE stream flows
 * over the loopback HTTP socket straight into the Rust gateway — no gateway logic
 * is reimplemented here, and nothing crosses the napi boundary per request.
 *
 * @example
 * ```typescript
 * import { getAiConnection } from "@alienplatform/ai-gateway"
 * import { createOpenAICompatible } from "@ai-sdk/openai-compatible"
 * const provider = createOpenAICompatible({ name: "alien", ...(await getAiConnection("llm")) })
 * ```
 */

import { createAiClient } from "./client.js"
import { createGateway } from "./gateway.js"
import { loadAddon } from "./loader.js"

const gateway = createGateway(loadAddon)
const client = createAiClient(gateway)

/** Start the in-process AI gateway (idempotent) and return its running handle. */
export const startAiGateway = gateway.startAiGateway
/** An OpenAI-compatible client for the named AI binding (External BYO-key or ambient). */
export const ai = client.ai
/** Resolve an AI binding to `{ baseURL, apiKey? }`, starting the gateway for ambient bindings. */
export const getAiConnection = client.getAiConnection

export { Ai } from "./client.js"
export type {
  AiConnection,
  AiModel,
  ChatCompletionCreateParams,
  ResponseCreateParams,
} from "./client.js"
export { aiBindingEnvVarName, isExternalAiBinding, parseAiBinding } from "./binding.js"
export type { AiBinding, AmbientAiBinding, ExternalAiBinding } from "./binding.js"
export {
  AiTransportError,
  AiUpstreamError,
  BindingNotFoundError,
  InvalidBindingConfigError,
} from "./errors.js"
