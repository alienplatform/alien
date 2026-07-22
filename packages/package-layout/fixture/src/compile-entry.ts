/**
 * Static-embed entry point for the `bun build --compile` check in `run.ts`.
 *
 * A compiled Worker's generated bootstrap calls `installEmbeddedAddon()` from
 * `@alienplatform/sdk/native` once, which registers BOTH embedded native pieces:
 * the `@alienplatform/bindings` napi addon (kv/storage/queue/vault, in-process)
 * and the `@alienplatform/ai-gateway` launcher binary (the gateway runs as a
 * spawned process). The SDK's re-exported factories then resolve those with no
 * filesystem lookup, the only way native access works inside a single-file binary.
 *
 * These are STATIC imports on purpose: `bun build --compile` only follows
 * statically analyzable imports. `@alienplatform/bindings/native` is imported
 * directly too, to keep the direct-consumer path covered alongside the SDK path.
 *
 * The bindings `.node` and the `alien-ai-gateway.bin` binary are only staged by
 * the release pipeline (.github/workflows/release.yml) or by `run.ts` here, so in
 * a workspace checkout with no staged assets the `bun build --compile` step fails.
 */

import { storage } from "@alienplatform/bindings/native"
import { ai, startAiGateway } from "@alienplatform/sdk"
import { installEmbeddedAddon } from "@alienplatform/sdk/native"

// Register both embedded pieces up front, exactly as a compiled Worker bootstrap
// does: this eagerly wires the bindings addon and the ai-gateway binary path.
installEmbeddedAddon()

// Reference the factories so the compiler must stage the assets: `storage` via
// the direct bindings/native path, `ai` via the SDK re-export.
for (const [name, factory] of [
  ["bindings storage", storage],
  ["sdk ai", ai],
] as const) {
  if (typeof factory !== "function") {
    throw new Error(`expected the ${name} factory to be a function after installEmbeddedAddon`)
  }
}

// The heart of the addon-to-binary switch: prove the embedded gateway binary
// extracts, spawns, and reports a loopback URL, from inside the compiled
// single-file binary, with no gateway on disk (it came from the embed). Wrapped
// in an async IIFE because `--format=cjs` (required for the embed) has no
// top-level await.
async function main(): Promise<void> {
  const handle = await startAiGateway()
  if (!/^http:\/\/127\.0\.0\.1:\d+$/.test(handle.url)) {
    throw new Error(`expected a loopback gateway URL from the embedded binary, got: ${handle.url}`)
  }
  console.log(
    `compile-entry: bindings addon embedded; ai-gateway binary embedded, extracted, and spawned (${handle.url})`,
  )
  // The spawned gateway is unref'd; exit deterministically so the smoke never hangs.
  process.exit(0)
}

main().catch(error => {
  console.error(error)
  process.exit(1)
})
