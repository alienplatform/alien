/**
 * `@alienplatform/sdk/native`: the embedded-native bridge for compiled Workers.
 *
 * A Worker depends only on `@alienplatform/sdk`; `@alienplatform/bindings` and
 * `@alienplatform/ai-gateway` are transitive dependencies reached *through* the
 * SDK, so a Worker's own `node_modules` cannot resolve their `/native` subpaths
 * directly. But it can resolve the SDK, and the SDK can resolve both (each is a
 * direct dependency). This subpath is that one hop: it installs both bun-embedded
 * pieces so a compiled Worker bootstrap can register them without naming packages
 * it can't see.
 *
 * `bun build --compile` follows the static re-export chains
 * (`@alienplatform/sdk/native` → `@alienplatform/{bindings,ai-gateway}/native`)
 * and embeds each package's native asset: the `alien-bindings.node` addon
 * (bindings runs in-process) and the `alien-ai-gateway` executable (the gateway
 * runs as a spawned process). The SDK keeps both packages external (see
 * tsdown.config.ts), so the registrations here are visible to the SDK's
 * re-exported `kv`/`storage`/`queue`/`vault` and `ai`/`getAiConnection` factories.
 *
 * Imported only by a generated compiled entry (never in dev), so the transitive
 * static asset imports inside each `/native` only run where the asset was staged.
 */

import { installEmbeddedGateway } from "@alienplatform/ai-gateway/native"
import { installEmbeddedAddon as installBindingsAddon } from "@alienplatform/bindings/native"

/** Register the bun-embedded bindings addon and ai-gateway binary with their loaders. */
export function installEmbeddedAddon(): void {
  installBindingsAddon()
  installEmbeddedGateway()
}
