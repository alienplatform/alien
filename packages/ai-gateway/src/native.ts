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
import { registerEmbeddedAddon } from "./loader.js"

/**
 * Register the bun-embedded addon with the default loader, so plain
 * `@alienplatform/ai-gateway` imports — including the SDK's re-exported `ai()`,
 * which resolves through {@link loadAddon} — use it inside a compiled binary.
 * `alien build` emits an explicit call to this from the compiled entry (via
 * `@alienplatform/sdk/native`); it's an explicit exported call, not a bare
 * side-effect import, so it survives this package's `sideEffects: false`
 * tree-shaking.
 */
export function installEmbeddedAddon(): void {
  registerEmbeddedAddon(addon)
}

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
