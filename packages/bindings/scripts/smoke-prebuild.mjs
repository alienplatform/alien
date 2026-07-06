/**
 * Prebuild smoke: prove a PUBLISHED-shape install works with NO build toolchain.
 *
 * Runs from a throwaway consumer directory that has installed three tarballs —
 * `@alienplatform/core`, the `@alienplatform/bindings` wrapper, and the
 * per-platform `@alienplatform/bindings-<triple>` prebuild — and nothing else.
 * No Rust, no `napi`, no `ALIEN_BINDINGS_ADDON_PATH`: the loader must resolve the
 * prebuilt `.node` through the platform package (loader.ts step 2), so this only
 * passes if the prebuilt addon carries its own compiled binary.
 *
 * It imports the wrapper (which pulls `AlienError` from the external
 * `@alienplatform/core`, proving that dependency resolved) and performs a real
 * local-provider KV put/get against a temp `local-kv` binding.
 */

import { mkdtempSync } from "node:fs"
import { tmpdir } from "node:os"
import { join } from "node:path"
import { kv } from "@alienplatform/bindings"

if (process.env.ALIEN_BINDINGS_ADDON_PATH) {
  console.error("[smoke] FAIL: ALIEN_BINDINGS_ADDON_PATH is set; that bypasses the prebuild path")
  process.exit(1)
}

const runtime = typeof Bun !== "undefined" ? "bun" : "node"

async function main() {
  const dataDir = mkdtempSync(join(tmpdir(), "alien-smoke-kv-"))
  const env = {
    ALIEN_DEPLOYMENT_TYPE: "local",
    ALIEN_CACHE_BINDING: JSON.stringify({ service: "local-kv", dataDir }),
  }

  const cache = kv("cache", { env })
  await cache.set("greeting", "hi")
  const got = await cache.getText("greeting")

  if (got !== "hi") {
    console.error(`[smoke:${runtime}] FAIL: expected 'hi', got ${JSON.stringify(got)}`)
    process.exit(1)
  }

  console.log(`[smoke:${runtime}] OK: kv put/get through the prebuilt addon returned '${got}'`)
}

main().catch(err => {
  console.error(`[smoke:${runtime}] UNEXPECTED`, err)
  process.exit(1)
})
