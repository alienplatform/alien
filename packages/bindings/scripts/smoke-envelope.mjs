/**
 * Dual-runtime smoke: prove end-to-end envelope recovery through the REAL napi
 * addon. Runs under both `node` and `bun` (see the `smoke:both` invocation).
 *
 * It intentionally sets no `ALIEN_BINDINGS_ADDON_PATH`, exercising the loader's
 * documented dev fallback (walk up to `crates/alien-bindings-node`). It resolves
 * a storage binding whose `ALIEN_<NAME>_BINDING` env var is absent, performs the
 * first operation, and asserts the thrown error is a typed
 * BindingNotConfiguredError carrying `{ binding, envVar }` recovered from the
 * addon's JSON error envelope.
 */

import { AlienError, storage } from "../dist/index.js"

const runtime = typeof Bun !== "undefined" ? "bun" : "node"

async function main() {
  // Zero binding env: no deployment type, no credentials, no ALIEN_FILES_BINDING.
  // Construction must still succeed and the first op must report the missing
  // binding before any platform/credential resolution.
  const files = storage("files")

  try {
    await files.head("does-not-matter.txt")
    console.error(`[${runtime}] FAIL: expected an error, got none`)
    process.exit(1)
  } catch (err) {
    const isAlien = err instanceof AlienError
    const code = err?.code
    const binding = err?.context?.binding
    const envVar = err?.context?.envVar
    const ok =
      isAlien &&
      code === "BINDING_NOT_CONFIGURED" &&
      binding === "files" &&
      envVar === "ALIEN_FILES_BINDING"

    console.log(
      `[${runtime}] instanceof AlienError=${isAlien} code=${code} binding=${binding} envVar=${envVar} ok=${ok}`,
    )
    process.exit(ok ? 0 : 1)
  }
}

main().catch(err => {
  console.error(`[${runtime}] UNEXPECTED`, err)
  process.exit(1)
})
