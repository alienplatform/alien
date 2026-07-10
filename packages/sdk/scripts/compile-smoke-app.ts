/**
 * Tiny app compiled by `scripts/compile-smoke.ts` into a standalone Bun
 * executable, exercising the path a real source-built **Worker** takes:
 *
 *   1. `@alienplatform/sdk/native` — the SDK's embedded-addon bridge — installs
 *      the bun-embedded addon. A Worker depends only on the SDK; it reaches the
 *      bindings package (and its `./native` static-embed entry) transitively
 *      through the SDK, exactly as this import chain does.
 *   2. `import { kv } from "@alienplatform/sdk"` — the documented app-facing
 *      surface — must then resolve to that same addon in-process. It works only
 *      if `bun build --compile` collapses the bindings loader to a single module
 *      (one `embedded` slot) shared by both the bridge and the re-exported `kv`.
 *
 * The harness runs this with the on-disk addon removed and no dev checkout
 * reachable, so a successful kv round-trip proves the addon came from the
 * embedded copy — not a filesystem fallback.
 */

import { installEmbeddedAddon } from "@alienplatform/sdk/native"
installEmbeddedAddon()
import { kv } from "@alienplatform/sdk"

async function main() {
  const value = "hello-from-sdk-compiled-binary"
  const store = kv("smoke")

  await store.set("smoke-key", value)
  const got = await store.getText("smoke-key")

  if (got !== value) {
    console.error(`MISMATCH: expected ${JSON.stringify(value)}, got ${JSON.stringify(got)}`)
    process.exit(1)
  }

  console.log(`OK ${got}`)
}

main().catch(err => {
  console.error("UNEXPECTED", err)
  process.exit(1)
})
