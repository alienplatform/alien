// Step 1 — pack the publishable packages.

import { existsSync, mkdirSync, readdirSync, rmSync } from "node:fs"
import { join, relative } from "node:path"
import { type CheckResult, type Ctx, lastLine, run } from "./shared.ts"

const PACK_TARGETS = ["sdk", "core", "bindings", "commands"] as const

export function packPackages(ctx: Ctx): CheckResult[] {
  const { scriptDir, packagesDir, tarballsDir, tarballs } = ctx
  const results: CheckResult[] = []

  rmSync(tarballsDir, { recursive: true, force: true })
  mkdirSync(tarballsDir, { recursive: true })

  function findTarball(name: string): string | undefined {
    const prefix = `alienplatform-${name}-`
    const file = readdirSync(tarballsDir).find(
      entry => entry.startsWith(prefix) && entry.endsWith(".tgz"),
    )
    return file ? join(tarballsDir, file) : undefined
  }

  for (const name of PACK_TARGETS) {
    const pkgDir = join(packagesDir, name)
    if (!existsSync(join(pkgDir, "package.json"))) {
      results.push({
        check: "pack",
        package: name,
        status: "fail",
        reason: "publishable package not present (nothing to pack)",
        evidence: pkgDir,
      })
      continue
    }

    const packed = run("pnpm", ["pack", "--pack-destination", tarballsDir], pkgDir)
    const tarball = findTarball(name)
    if (packed.status !== 0 || !tarball) {
      results.push({
        check: "pack",
        package: name,
        status: "fail",
        reason: "pnpm pack failed",
        evidence: lastLine(packed.stderr) || lastLine(packed.stdout) || `exit ${packed.status}`,
      })
      continue
    }
    tarballs.set(name, tarball)
    results.push({
      check: "pack",
      package: name,
      status: "pass",
      reason: "ok",
      evidence: relative(scriptDir, tarball),
    })
  }

  return results
}
