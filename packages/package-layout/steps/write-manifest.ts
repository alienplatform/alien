// Step 2 — rewrite the consumer manifest to file: the tarballs.

import { readFileSync, writeFileSync } from "node:fs"
import { join, relative } from "node:path"
import type { CheckResult, Ctx } from "./shared.ts"

export function writeManifest(ctx: Ctx): CheckResult[] {
  const { fixtureDir, tarballs } = ctx

  // Direct dependencies the consumer imports; overrides pin every transitive
  // @alienplatform/* to a packed tarball so npm never reaches the registry for one.
  const DIRECT_DEP_PACKAGES = ["sdk", "bindings", "commands", "ai-gateway"] as const
  const OVERRIDE_PACKAGES = ["core", "sdk", "bindings", "commands", "ai-gateway"] as const

  function fileSpec(tarball: string): string {
    return `file:${relative(fixtureDir, tarball).split("\\").join("/")}`
  }

  const fixtureManifestPath = join(fixtureDir, "package.json")
  const fixtureManifest = JSON.parse(readFileSync(fixtureManifestPath, "utf8")) as Record<
    string,
    unknown
  >

  const dependencies: Record<string, string> = {}
  for (const name of DIRECT_DEP_PACKAGES) {
    const tarball = tarballs.get(name)
    if (tarball) dependencies[`@alienplatform/${name}`] = fileSpec(tarball)
  }
  const overrides: Record<string, string> = {}
  for (const name of OVERRIDE_PACKAGES) {
    const tarball = tarballs.get(name)
    if (tarball) overrides[`@alienplatform/${name}`] = fileSpec(tarball)
  }

  fixtureManifest.dependencies = dependencies
  fixtureManifest.overrides = overrides
  writeFileSync(fixtureManifestPath, `${JSON.stringify(fixtureManifest, null, 2)}\n`)
  return [
    {
      check: "write-manifest",
      package: "fixture",
      status: "pass",
      reason: "ok",
      evidence: `deps=[${Object.keys(dependencies).join(", ")}] overrides=[${Object.keys(overrides).join(", ")}]`,
    },
  ]
}
