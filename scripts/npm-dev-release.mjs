#!/usr/bin/env node

import { createHash } from "node:crypto"
import { readFileSync, writeFileSync } from "node:fs"
import { basename, resolve } from "node:path"

export const packages = [
  { path: "packages/core/package.json", publish: true },
  { path: "packages/commands/package.json", publish: true },
  { path: "packages/bindings/package.json", publish: true },
  { path: "packages/sdk/package.json", publish: true },
  { path: "packages/testing/package.json", publish: true },
  { path: "client-sdks/platform/typescript/package.json", publish: true },
  { path: "client-sdks/manager/typescript/package.json", publish: true },
  { path: "crates/alien-bindings-node/package.json", publish: false, versionFrom: "@alienplatform/bindings" },
  ...["darwin-arm64", "darwin-x64", "linux-x64-gnu", "linux-arm64-gnu"].map(triple => ({
    path: `packages/bindings/npm/${triple}/package.json`,
    publish: true,
    versionFrom: "@alienplatform/bindings",
  })),
]

const dependencyFields = ["dependencies", "devDependencies", "peerDependencies", "optionalDependencies"]

function readJson(path) {
  return JSON.parse(readFileSync(path, "utf8"))
}

function writeJson(path, value) {
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`)
}

function baseVersion(version, name) {
  const match = /^(\d+\.\d+\.\d+)(?:-[0-9A-Za-z.-]+)?$/.exec(version)
  if (!match) throw new Error(`${name} has unsupported version ${version}`)
  return match[1]
}

export function computeVersions(root, sha) {
  if (!/^[0-9a-f]{40}$/.test(sha)) throw new Error(`Expected a full lowercase git SHA, got ${sha}`)
  const shortSha = sha.slice(0, 12)
  const manifests = packages.map(entry => ({ ...entry, manifest: readJson(resolve(root, entry.path)) }))
  const versions = new Map()

  for (const { manifest, versionFrom } of manifests) {
    if (versionFrom) continue
    versions.set(manifest.name, `${baseVersion(manifest.version, manifest.name)}-dev.${shortSha}`)
  }
  for (const { manifest, versionFrom } of manifests) {
    if (!versionFrom) continue
    const version = versions.get(versionFrom)
    if (!version) throw new Error(`${manifest.name} references unknown version source ${versionFrom}`)
    versions.set(manifest.name, version)
  }
  return versions
}

export function rewriteManifests(root, sha) {
  const versions = computeVersions(root, sha)
  for (const { path } of packages) {
    const absolutePath = resolve(root, path)
    const manifest = readJson(absolutePath)
    manifest.version = versions.get(manifest.name)
    for (const field of dependencyFields) {
      if (!manifest[field]) continue
      for (const dependency of Object.keys(manifest[field])) {
        const version = versions.get(dependency)
        if (version) manifest[field][dependency] = version
      }
    }
    writeJson(absolutePath, manifest)
  }
  return versions
}

export function validateManifests(root, sha) {
  const expected = computeVersions(root, sha)
  for (const { path } of packages) {
    const manifest = readJson(resolve(root, path))
    if (manifest.version !== expected.get(manifest.name)) {
      throw new Error(`${manifest.name} version is ${manifest.version}; expected ${expected.get(manifest.name)}`)
    }
    if (!/-dev\.[0-9a-f]{12}$/.test(manifest.version)) {
      throw new Error(`${manifest.name} version is not an immutable dev prerelease: ${manifest.version}`)
    }
    for (const field of dependencyFields) {
      for (const [dependency, range] of Object.entries(manifest[field] ?? {})) {
        const version = expected.get(dependency)
        if (version && range !== version) {
          throw new Error(`${manifest.name} ${field}.${dependency} is ${range}; expected ${version}`)
        }
      }
    }
  }
  return expected
}

function printVersions(versions) {
  process.stdout.write(`${JSON.stringify(Object.fromEntries([...versions].sort()), null, 2)}\n`)
}

const [command, argument] = process.argv.slice(2)
if (command === "rewrite") {
  printVersions(rewriteManifests(process.cwd(), argument))
} else if (command === "validate") {
  printVersions(validateManifests(process.cwd(), argument))
} else if (command === "digest") {
  const files = process.argv.slice(3)
  const output = files.sort().map(path => ({
    file: basename(path),
    sha256: createHash("sha256").update(readFileSync(path)).digest("hex"),
  }))
  process.stdout.write(`${JSON.stringify(output, null, 2)}\n`)
} else if (import.meta.url === `file://${process.argv[1]}`) {
  throw new Error("Usage: npm-dev-release.mjs <rewrite|validate|digest> <git-sha|tarballs...>")
}
