import assert from "node:assert/strict"
import { execFileSync } from "node:child_process"
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { dirname, resolve } from "node:path"
import test from "node:test"
import { fileURLToPath } from "node:url"

import { packages, rewriteManifests, validateManifests } from "./npm-dev-release.mjs"

const repositoryRoot = process.env.NPM_DEV_SOURCE_ROOT
  ? resolve(process.env.NPM_DEV_SOURCE_ROOT)
  : resolve(dirname(fileURLToPath(import.meta.url)), "..")
const sha = "0123456789abcdef0123456789abcdef01234567"

// The gateway's per-platform manifests are generated rather than checked in, so the
// package list cannot be copied until this has written them.
execFileSync(process.execPath, [
  resolve(repositoryRoot, "packages/ai-gateway/scripts/generate-prebuilds.mjs"),
  resolve(repositoryRoot, "packages/ai-gateway"),
])

function fixture() {
  const root = mkdtempSync(resolve(tmpdir(), "alien-npm-dev-"))
  for (const { path } of packages) {
    const source = resolve(repositoryRoot, path)
    const target = resolve(root, path)
    mkdirSync(dirname(target), { recursive: true })
    copyFileSync(source, target)
  }
  return root
}

test("rewrites every published package and internal edge to commit-addressed versions", () => {
  const root = fixture()
  const coreBase = JSON.parse(
    readFileSync(resolve(root, "packages/core/package.json"), "utf8"),
  ).version.replace(/-.*/, "")
  const platformBase = JSON.parse(
    readFileSync(resolve(root, "client-sdks/platform/typescript/package.json"), "utf8"),
  ).version.replace(/-.*/, "")
  const versions = rewriteManifests(root, sha)
  validateManifests(root, sha)

  assert.equal(versions.get("@alienplatform/core"), `${coreBase}-dev.${sha}`)
  assert.equal(versions.get("@alienplatform/platform-api"), `${platformBase}-dev.${sha}`)

  const commands = JSON.parse(readFileSync(resolve(root, "packages/commands/package.json"), "utf8"))
  assert.equal(commands.dependencies["@alienplatform/core"], versions.get("@alienplatform/core"))

  const sdk = JSON.parse(readFileSync(resolve(root, "packages/sdk/package.json"), "utf8"))
  assert.equal(sdk.dependencies["@alienplatform/core"], versions.get("@alienplatform/core"))
  assert.equal(sdk.dependencies["@alienplatform/bindings"], versions.get("@alienplatform/bindings"))
  assert.equal(
    sdk.dependencies["@alienplatform/ai-gateway"],
    versions.get("@alienplatform/ai-gateway"),
  )
})

test("rejects a stable or mismatched package graph after rewrite", () => {
  const root = fixture()
  rewriteManifests(root, sha)
  const path = resolve(root, "packages/commands/package.json")
  const manifest = JSON.parse(readFileSync(path, "utf8"))
  manifest.dependencies["@alienplatform/core"] = "^1.14.1"
  writeFileSync(path, `${JSON.stringify(manifest)}\n`)

  // Derived, not literal: a hardcoded version stops matching the next time core is bumped.
  const coreVersion = JSON.parse(
    readFileSync(resolve(root, "packages/core/package.json"), "utf8"),
  ).version
  assert.throws(() => validateManifests(root, sha), {
    message: `@alienplatform/commands dependencies.@alienplatform/core is ^1.14.1; expected ${coreVersion}`,
  })
})

test("rejects abbreviated commit identities", () => {
  assert.throws(() => rewriteManifests(fixture(), sha.slice(0, 12)), /full lowercase git SHA/)
})
