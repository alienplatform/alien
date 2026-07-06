/**
 * Inject the per-platform prebuild packages as exact-version
 * `optionalDependencies` of the `@alienplatform/bindings` wrapper, pinned to the
 * wrapper's own version.
 *
 * Run by the release pipeline (publish-bindings) after the wrapper's version has
 * been rewritten to the release version and before the wrapper is published.
 * Chosen over `napi prepublish`, which rewrites/regenerates more than we need —
 * an explicit, exact pin is deterministic and reviewable, and guarantees a
 * published wrapper can only ever resolve the matching-version platform addon.
 *
 * Idempotent: rewrites `optionalDependencies` wholesale from the fixed triple
 * list, so re-running never accumulates stale entries.
 */

import { readFileSync, writeFileSync } from "node:fs"
import { fileURLToPath } from "node:url"

const TRIPLES = ["darwin-arm64", "darwin-x64", "linux-x64-gnu", "linux-arm64-gnu"]

const wrapperManifest = fileURLToPath(new URL("../package.json", import.meta.url))
const pkg = JSON.parse(readFileSync(wrapperManifest, "utf8"))
const { version } = pkg

pkg.optionalDependencies = Object.fromEntries(
  TRIPLES.map(triple => [`@alienplatform/bindings-${triple}`, version]),
)

writeFileSync(wrapperManifest, `${JSON.stringify(pkg, null, 2)}\n`)

console.log(`Injected optionalDependencies (pinned to ${version}):`)
console.log(JSON.stringify(pkg.optionalDependencies, null, 2))
