// Step 3 — npm install the consumer, assert tarball resolution.

import { readFileSync, rmSync } from "node:fs"
import { join } from "node:path"
import { type CheckResult, type Ctx, lastLine, run } from "./shared.ts"

export function installConsumer(ctx: Ctx): CheckResult[] {
  const { fixtureDir, tarballs } = ctx
  const results: CheckResult[] = []

  // Force a clean resolution against the freshly rewritten manifest.
  rmSync(join(fixtureDir, "node_modules"), { recursive: true, force: true })
  rmSync(join(fixtureDir, "package-lock.json"), { force: true })

  const install = run("npm", ["install", "--no-audit", "--no-fund"], fixtureDir)
  if (install.status !== 0) {
    results.push({
      check: "install",
      package: "fixture",
      status: "fail",
      reason: "npm install failed",
      evidence: lastLine(install.stderr) || lastLine(install.stdout) || `exit ${install.status}`,
    })
    return results
  }

  results.push({
    check: "install",
    package: "fixture",
    status: "pass",
    reason: "ok",
    evidence: lastLine(install.stdout),
  })

  const lockPath = join(fixtureDir, "package-lock.json")
  const lock = JSON.parse(readFileSync(lockPath, "utf8")) as {
    packages?: Record<string, { resolved?: string }>
  }
  const lockPackages = lock.packages ?? {}
  for (const name of ["sdk", "core"]) {
    if (!tarballs.has(name)) continue
    const entry = lockPackages[`node_modules/@alienplatform/${name}`]
    const resolved = entry?.resolved ?? "<not installed>"
    results.push({
      check: "install-resolution",
      package: name,
      status: resolved.startsWith("file:") ? "pass" : "fail",
      reason: resolved.startsWith("file:")
        ? "ok"
        : "transitive @alienplatform package did not resolve to the packed tarball",
      evidence: `node_modules/@alienplatform/${name} -> ${resolved}`,
    })
  }

  return results
}
