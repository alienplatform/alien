import { readFileSync } from "node:fs"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"

const expected = process.argv[2]
if (!expected) {
  throw new Error("usage: validate-release-version.mjs <expected-version>")
}

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "../../..")
// The wrapper package. Per-platform binary prebuild manifests
// (packages/ai-gateway/npm/<triple>/package.json) are generated + version-stamped
// by the release pipeline, so they are validated there rather than pinned here.
const manifests = ["packages/ai-gateway/package.json"]

for (const path of manifests) {
  const actual = JSON.parse(readFileSync(join(repoRoot, path), "utf8")).version
  if (actual !== expected) {
    throw new Error(`${path}: expected version ${expected}, got ${actual}`)
  }
}

const cargo = readFileSync(join(repoRoot, "Cargo.toml"), "utf8")
const workspaceVersion = cargo.match(/\[workspace\.package\]\nversion = "([^"]+)"/)?.[1]
if (workspaceVersion !== expected) {
  throw new Error(`Cargo workspace: expected version ${expected}, got ${workspaceVersion}`)
}

console.log(`AI-gateway release manifests are locked to ${expected}`)
