/**
 * `@alienplatform/ai-gateway/native` — static-embed entry for `bun build --compile`.
 *
 * Imports the `alien-ai-gateway` executable through a literal `with { type: "file" }`
 * specifier so bun embeds it into the single-file binary, and registers its path so
 * the default loader extracts and spawns it. Unlike the default entry it does not
 * probe the filesystem: the binary must already be staged next to the built
 * `native.js` (the build owns that copy step). The specifier is kept external at
 * build time (tsdown.config.ts) so the literal survives into `dist/native.js`.
 * `bun build --compile` needs `--format=cjs`.
 */

import embeddedBinaryPath from "./alien-ai-gateway.bin" with { type: "file" }
import { createAiClient } from "./client.js"
import { createGateway } from "./gateway.js"
import { registerEmbeddedBinary, resolveGatewayBinary } from "./loader.js"

/**
 * Register the bun-embedded gateway binary with the default loader, so plain
 * `@alienplatform/ai-gateway` imports — including the SDK's re-exported `ai()`,
 * which resolves through {@link resolveGatewayBinary}, to spawn it inside a compiled
 * binary. `alien build` emits an explicit call to this from the compiled entry
 * (via `@alienplatform/sdk/native`); it's an explicit exported call, not a bare
 * side-effect import, so it survives this package's `sideEffects: false`
 * tree-shaking.
 */
export function installEmbeddedGateway(): void {
  registerEmbeddedBinary(embeddedBinaryPath)
}

const gateway = createGateway(resolveGatewayBinary)
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
