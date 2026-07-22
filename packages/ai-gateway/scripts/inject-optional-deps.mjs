/**
 * Pin the per-platform prebuild packages as exact-version `optionalDependencies`
 * of the `@alienplatform/ai-gateway` wrapper, so a published wrapper can only
 * resolve the matching-version platform addon.
 *
 * The release pipeline runs this after rewriting the wrapper's version and before
 * publishing. Preferred over `napi prepublish`, which regenerates more than the pin.
 */

import { readFileSync, readdirSync, writeFileSync } from "node:fs"
import { fileURLToPath } from "node:url"

const TRIPLES = ["darwin-arm64", "darwin-x64", "linux-x64-gnu", "linux-arm64-gnu"]

const wrapperManifest = fileURLToPath(new URL("../package.json", import.meta.url))
const pkg = JSON.parse(readFileSync(wrapperManifest, "utf8"))
const { version } = pkg

// An unusable version silently JSON.stringify's away (undefined values are dropped), which
// would publish the wrapper with *no* optionalDependencies — every consumer install then
// resolves no addon at all. Fail the release instead.
if (typeof version !== "string" || version === "" || version === "0.0.0") {
  throw new Error(`refusing to inject optionalDependencies: package version is '${version}'`)
}

// TRIPLES must mirror the per-platform packages on disk (PACKAGE_LAYOUT.md pins them).
const onDisk = readdirSync(fileURLToPath(new URL("../npm", import.meta.url)), {
  withFileTypes: true,
})
  .filter(entry => entry.isDirectory())
  .map(entry => entry.name)
  .sort()
if (onDisk.join() !== [...TRIPLES].sort().join()) {
  throw new Error(`npm/ holds [${onDisk}], but TRIPLES is [${[...TRIPLES].sort()}]`)
}

pkg.optionalDependencies = Object.fromEntries(
  TRIPLES.map(triple => [`@alienplatform/ai-gateway-${triple}`, version]),
)

writeFileSync(wrapperManifest, `${JSON.stringify(pkg, null, 2)}\n`)

console.log(`Injected optionalDependencies (pinned to ${version}):`)
console.log(JSON.stringify(pkg.optionalDependencies, null, 2))
