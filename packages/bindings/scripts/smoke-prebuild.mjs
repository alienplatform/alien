/**
 * Prebuild smoke: prove a PUBLISHED-shape install works with NO build toolchain.
 *
 * Runs from a throwaway consumer directory whose only direct dependency is
 * `@alienplatform/bindings`. The wrapper must pull `@alienplatform/core` and the
 * correct `@alienplatform/bindings-<triple>` optionalDependency from the local
 * publish-equivalent registry. No Rust, no `napi`, no
 * `ALIEN_BINDINGS_ADDON_PATH`: the loader must resolve the prebuilt `.node`
 * through the platform package (loader.ts step 2), so this only passes if the
 * prebuilt addon carries its own compiled binary.
 *
 * It imports the wrapper (which pulls `AlienError` from the external
 * `@alienplatform/core`, proving that dependency resolved) and performs a real
 * local-provider KV put/get against a temp `local-kv` binding.
 */

import { kv } from "@alienplatform/bindings"

if (process.env.ALIEN_BINDINGS_ADDON_PATH) {
  console.error("[smoke] FAIL: ALIEN_BINDINGS_ADDON_PATH is set; that bypasses the prebuild path")
  process.exit(1)
}

const runtime = typeof Bun !== "undefined" ? "bun" : "node"

async function main() {
  if (!process.env.ALIEN_DEPLOYMENT_TYPE || !process.env.ALIEN_CACHE_BINDING) {
    throw new Error("smoke binding environment must be set before the runtime starts")
  }

  const cache = kv("cache")
  await cache.set("greeting", "hi")
  const got = await cache.getText("greeting")

  if (got !== "hi") {
    throw new Error(`expected 'hi', got ${JSON.stringify(got)}`)
  }

  console.log(`[smoke:${runtime}] OK: kv put/get through the prebuilt addon returned '${got}'`)
}

main().catch(err => {
  console.error(`[smoke:${runtime}] UNEXPECTED`, err)
  process.exit(1)
})
