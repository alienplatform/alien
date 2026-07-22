/**
 * `@alienplatform/ai-gateway/native` — static-embed entry for `bun build --compile`.
 *
 * Imports the native addon through the literal specifier `./alien-ai-gateway.node`
 * so bun's compiler can detect the reference and embed it into the single-file
 * executable. Unlike the default entry it does not probe the filesystem — the
 * addon must already be staged next to the built `native.js` (the build owns that
 * copy step). The specifier is kept external at build time (tsdown.config.ts) so
 * the literal survives into `dist/native.js`. `bun build --compile` needs
 * `--format=cjs`.
 */

import addon from "./alien-ai-gateway.node"
import { createAiClient } from "./client.js"
import { createGateway } from "./gateway.js"

const gateway = createGateway(() => addon)
const client = createAiClient(gateway)

export const startAiGateway = gateway.startAiGateway
export const ai = client.ai
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
