import assert from "node:assert/strict"
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs"
import { tmpdir } from "node:os"
import { dirname, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import test from "node:test"

import { packages, rewriteManifests, validateManifests } from "./npm-dev-release.mjs"

const repositoryRoot = process.env.NPM_DEV_SOURCE_ROOT
  ? resolve(process.env.NPM_DEV_SOURCE_ROOT)
  : resolve(dirname(fileURLToPath(import.meta.url)), "..")
const sha = "0123456789abcdef0123456789abcdef01234567"

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
  const versions = rewriteManifests(root, sha)
  validateManifests(root, sha)

  assert.equal(versions.get("@alienplatform/core"), "1.14.1-dev.0123456789ab")
  assert.equal(versions.get("@alienplatform/platform-api"), "1.14.3-dev.0123456789ab")

  const commands = JSON.parse(readFileSync(resolve(root, "packages/commands/package.json"), "utf8"))
  assert.equal(commands.dependencies["@alienplatform/core"], versions.get("@alienplatform/core"))

  const sdk = JSON.parse(readFileSync(resolve(root, "packages/sdk/package.json"), "utf8"))
  assert.equal(sdk.dependencies["@alienplatform/core"], versions.get("@alienplatform/core"))
  assert.equal(sdk.dependencies["@alienplatform/bindings"], versions.get("@alienplatform/bindings"))
})

test("rejects a stable or mismatched package graph after rewrite", () => {
  const root = fixture()
  rewriteManifests(root, sha)
  const path = resolve(root, "packages/commands/package.json")
  const manifest = JSON.parse(readFileSync(path, "utf8"))
  manifest.dependencies["@alienplatform/core"] = "^1.14.1"
  writeFileSync(path, `${JSON.stringify(manifest)}\n`)

  assert.throws(() => validateManifests(root, sha), /expected 1\.14\.1-dev\.0123456789ab/)
})
