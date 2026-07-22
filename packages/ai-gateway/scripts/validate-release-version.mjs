import { readFileSync } from "node:fs"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"

const expected = process.argv[2]
if (!expected) {
  throw new Error("usage: validate-release-version.mjs <expected-version>")
}

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "../../..")
const manifests = [
  "packages/ai-gateway/package.json",
  "crates/alien-ai-gateway-node/package.json",
  "packages/ai-gateway/npm/darwin-arm64/package.json",
  "packages/ai-gateway/npm/darwin-x64/package.json",
  "packages/ai-gateway/npm/linux-x64-gnu/package.json",
  "packages/ai-gateway/npm/linux-arm64-gnu/package.json",
]

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

const addonCargo = readFileSync(join(repoRoot, "crates/alien-ai-gateway-node/Cargo.toml"), "utf8")
if (!/^version\.workspace = true$/m.test(addonCargo)) {
  throw new Error("alien-ai-gateway-node must inherit the validated Cargo workspace version")
}

console.log(`AI-gateway release manifests are locked to ${expected}`)
