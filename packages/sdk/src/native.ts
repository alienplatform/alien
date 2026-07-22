/**
 * `@alienplatform/sdk/native` — the embedded-addon bridge for compiled Workers.
 *
 * A Worker depends only on `@alienplatform/sdk`; `@alienplatform/bindings` and
 * `@alienplatform/ai-gateway` are transitive dependencies reached *through* the
 * SDK, so a Worker's own `node_modules` cannot resolve their `/native` subpaths
 * directly. But it can resolve the SDK, and the SDK can resolve both (each is a
 * direct dependency). This subpath is that one hop: it installs both
 * bun-embedded addons so a compiled Worker bootstrap can register them without
 * naming packages it can't see.
 *
 * `bun build --compile` follows the static re-export chains
 * (`@alienplatform/sdk/native` → `@alienplatform/{bindings,ai-gateway}/native` →
 * the staged `alien-bindings.node` / `alien-ai-gateway.node`) and embeds both
 * addons into the single-file binary. The SDK keeps both packages external (see
 * tsdown.config.ts), so the compiled binary has exactly one module for each —
 * the same ones the SDK's re-exported `kv`/`storage`/`queue`/`vault` and
 * `ai`/`getAiConnection` resolve through — so the registrations here are visible
 * to those factories.
 *
 * This module is imported only by a generated compiled entry (never in dev), so
 * the transitive static `import addon from "./alien-*.node"` inside each
 * `/native` only runs where that addon has been staged.
 */

import { installEmbeddedAddon as installBindingsAddon } from "@alienplatform/bindings/native"
import { installEmbeddedAddon as installAiGatewayAddon } from "@alienplatform/ai-gateway/native"

/** Register both bun-embedded addons (bindings + ai-gateway) with their default loaders. */
export function installEmbeddedAddon(): void {
  installBindingsAddon()
  installAiGatewayAddon()
}
