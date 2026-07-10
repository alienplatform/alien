/**
 * `@alienplatform/sdk/native` — the embedded-addon bridge for compiled Workers.
 *
 * A Worker depends only on `@alienplatform/sdk`; `@alienplatform/bindings` is a
 * transitive dependency reached *through* the SDK, so a Worker's own
 * `node_modules` cannot resolve `@alienplatform/bindings/native` directly. But
 * it can resolve the SDK, and the SDK can resolve `@alienplatform/bindings/native`
 * (bindings is the SDK's direct dependency). This subpath is that one hop: it
 * re-exports `installEmbeddedAddon` so a compiled Worker bootstrap can register
 * the bun-embedded addon with the bindings loader without naming the bindings
 * package it can't see.
 *
 * `bun build --compile` follows this static re-export chain
 * (`@alienplatform/sdk/native` → `@alienplatform/bindings/native` → the staged
 * `alien-bindings.node`) and embeds the addon into the single-file binary. The
 * SDK keeps `@alienplatform/bindings` external (see tsdown.config.ts), so the
 * compiled binary has exactly one bindings module — the same one the SDK's
 * re-exported `kv`/`storage`/`queue`/`vault` resolve through — and the
 * registration performed here is visible to those factories.
 *
 * This module is imported only by a generated compiled entry (never in dev),
 * so its transitive static `import addon from "./alien-bindings.node"` inside
 * `@alienplatform/bindings/native` only runs where that addon has been staged.
 */

export { installEmbeddedAddon } from "@alienplatform/bindings/native"
