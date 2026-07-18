/** Verify the installed shape produced by the addon release smoke. */

import { existsSync, readFileSync } from "node:fs"
import { join } from "node:path"

const [expectedTriple, expectedVersion] = process.argv.slice(2)
if (!expectedTriple || !expectedVersion) {
  throw new Error("usage: verify-prebuild-install.mjs <triple> <version>")
}

const triples = {
  "darwin-arm64": { os: "darwin", cpu: "arm64" },
  "darwin-x64": { os: "darwin", cpu: "x64" },
  "linux-x64-gnu": { os: "linux", cpu: "x64", libc: "glibc" },
  "linux-arm64-gnu": { os: "linux", cpu: "arm64", libc: "glibc" },
}
const expectedPlatform = triples[expectedTriple]
if (!expectedPlatform) throw new Error(`unsupported smoke triple: ${expectedTriple}`)

const consumer = JSON.parse(readFileSync("package.json", "utf8"))
const directDependencies = Object.keys(consumer.dependencies ?? {})
if (directDependencies.length !== 1 || directDependencies[0] !== "@alienplatform/bindings") {
  throw new Error(`consumer must depend only on @alienplatform/bindings; got ${directDependencies}`)
}

const modules = join(process.cwd(), "node_modules", "@alienplatform")
const wrapperManifestPath = join(modules, "bindings", "package.json")
const wrapper = JSON.parse(readFileSync(wrapperManifestPath, "utf8"))
if (wrapper.version !== expectedVersion) {
  throw new Error(`wrapper version: expected ${expectedVersion}, got ${wrapper.version}`)
}

const expectedOptionalDependencies = Object.fromEntries(
  Object.keys(triples).map(triple => [`@alienplatform/bindings-${triple}`, expectedVersion]),
)
if (JSON.stringify(wrapper.optionalDependencies) !== JSON.stringify(expectedOptionalDependencies)) {
  throw new Error(
    `wrapper optionalDependencies do not match the release: ${JSON.stringify(wrapper.optionalDependencies)}`,
  )
}

const core = JSON.parse(readFileSync(join(modules, "core", "package.json"), "utf8"))
if (core.version !== expectedVersion) {
  throw new Error(`core version: expected ${expectedVersion}, got ${core.version}`)
}

const platformName = `@alienplatform/bindings-${expectedTriple}`
const platformDirectory = join(modules, `bindings-${expectedTriple}`)
const platformManifestPath = join(platformDirectory, "package.json")
const platform = JSON.parse(readFileSync(platformManifestPath, "utf8"))
const addonPath = join(platformDirectory, platform.main)
if (platform.name !== platformName || platform.version !== expectedVersion) {
  throw new Error(
    `platform package: expected ${platformName}@${expectedVersion}, got ${platform.name}@${platform.version}`,
  )
}
for (const [field, expected] of Object.entries(expectedPlatform)) {
  if (!platform[field]?.includes(expected)) {
    throw new Error(`${platformName} ${field}: expected ${expected}, got ${platform[field]}`)
  }
}
if (!existsSync(addonPath) || !addonPath.endsWith(`alien-bindings-node.${expectedTriple}.node`)) {
  throw new Error(`resolved platform package does not contain the expected addon: ${addonPath}`)
}

for (const triple of Object.keys(triples)) {
  if (triple === expectedTriple) continue
  if (existsSync(join(modules, `bindings-${triple}`))) {
    throw new Error(`unexpected non-target platform package was installed: ${triple}`)
  }
}

console.log(
  `[smoke] resolved ${platformName}@${platform.version} from wrapper-only dependency (${addonPath})`,
)
