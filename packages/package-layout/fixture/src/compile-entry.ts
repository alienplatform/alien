/**
 * Static-embed entry point for the `bun build --compile` check in `run.ts`.
 *
 * A compiled Worker's generated bootstrap calls `installEmbeddedAddon()` from
 * `@alienplatform/sdk/native` once, which registers BOTH bun-embedded native
 * addons — `@alienplatform/bindings` (kv/storage/queue/vault) and
 * `@alienplatform/ai-gateway` (the `ai()` client) — with their loaders. The SDK's
 * re-exported factories then resolve those addons with no filesystem lookup,
 * which is the only way native access works inside a single-file binary.
 *
 * These are STATIC imports on purpose: `bun build --compile` only follows
 * statically analyzable imports. `@alienplatform/bindings/native` is imported
 * directly too, to keep the direct-consumer path covered alongside the SDK path.
 *
 * The per-platform `.node` prebuilds are only staged by the release pipeline
 * (.github/workflows/release.yml) or by `run.ts` here, so in a workspace checkout
 * with no staged addon the `bun build --compile` step fails.
 */

import { storage } from "@alienplatform/bindings/native"
import { ai } from "@alienplatform/sdk"
import { installEmbeddedAddon } from "@alienplatform/sdk/native"

// Register both embedded addons up front, exactly as a compiled Worker bootstrap
// does. This eagerly loads each `.node` (through each package's `/native` entry),
// so a binary that runs this after the staged `.node` files are removed proves
// both addons are embedded, not filesystem-loaded.
installEmbeddedAddon()

// Reference the factories so the compiler must stage the addons: `storage` via the
// direct bindings/native path, `ai` via the SDK's re-export (the Worker path that
// the embedded ai-gateway addon fixes).
for (const [name, factory] of [
  ["bindings storage", storage],
  ["sdk ai", ai],
] as const) {
  if (typeof factory !== "function") {
    throw new Error(`expected the ${name} factory to be a function after installEmbeddedAddon`)
  }
}

console.log("compile-entry: bindings + ai-gateway native addons embedded via the SDK")
