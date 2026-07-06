/**
 * Tiny app compiled by `scripts/compile-smoke.ts` into a standalone Bun
 * executable. Imports the static-embed entry (`@alienplatform/bindings/native`,
 * resolved here via the relative path to `dist/native.js` — the exact file
 * `alien build`'s bundler will resolve that import specifier to) and proves a
 * real local-kv round-trip works from inside the compiled binary, with no
 * `.node` file present on disk next to it (the addon must be embedded).
 */

import { kv } from "../dist/native.js"

async function main() {
  const value = "hello-from-compiled-binary"
  const store = kv("smoke-kv")

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
