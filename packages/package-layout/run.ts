/**
 * Executable package proof: pack the public packages, install their tarballs in
 * an npm consumer, then exercise the shipped runtime and declaration surfaces.
 */

import { readFileSync, writeFileSync } from "node:fs"
import { dirname, join, resolve } from "node:path"
import { fileURLToPath } from "node:url"
import { compileNativeEmbed } from "./steps/compile.ts"
import { ensureAddon } from "./steps/ensure-addon.ts"
import { runImportChecks } from "./steps/import-checks.ts"
import { installConsumer } from "./steps/install-consumer.ts"
import { packPackages } from "./steps/pack-packages.ts"
import { packedContents } from "./steps/packed-contents.ts"
import { type CheckResult, type Ctx, run } from "./steps/shared.ts"
import { typecheckConsumer } from "./steps/typecheck.ts"
import { writeManifest } from "./steps/write-manifest.ts"

function createContext(): Ctx {
  const scriptDir = dirname(fileURLToPath(import.meta.url))
  const packagesDir = join(scriptDir, "..")
  const bunAvailable = run("bun", ["--version"], scriptDir).status === 0

  if (!bunAvailable) {
    console.log("[env-skip] bun is unavailable; Node checks still run (CI installs Bun).")
  }

  return {
    scriptDir,
    packagesDir,
    tarballsDir: join(scriptDir, ".tarballs"),
    fixtureDir: join(scriptDir, "fixture"),
    repoRoot: dirname(packagesDir),
    bunAvailable,
    tarballs: new Map<string, string>(),
  }
}

function report(results: CheckResult[]): number {
  console.log("")
  console.log("package-layout fixture")
  console.log("======================")

  let failures = 0
  for (const result of results) {
    if (result.status === "pass") {
      console.log(`  PASS ${result.check} package=${result.package} — ${result.evidence}`)
      continue
    }

    failures += 1
    console.error(`  FAIL ${result.check} package=${result.package}: ${result.reason}`)
    console.error(`       ${result.evidence}`)
  }

  console.log("")
  console.log(
    failures === 0
      ? `OK: ${results.length} checks passed.`
      : `FAILED: ${failures} of ${results.length} checks failed.`,
  )
  return failures === 0 ? 0 : 1
}

function main(): void {
  const ctx = createContext()
  const manifestPath = join(ctx.fixtureDir, "package.json")
  const originalManifest = readFileSync(manifestPath, "utf8")
  try {
    const results = [
      ...packPackages(ctx),
      ...writeManifest(ctx),
      ...installConsumer(ctx),
      ...ensureAddon(ctx),
      ...runImportChecks(ctx),
      ...typecheckConsumer(ctx),
      ...packedContents(ctx),
      ...compileNativeEmbed(ctx),
    ]
    process.exitCode = report(results)
  } finally {
    writeFileSync(manifestPath, originalManifest)
  }
}

const entry = process.argv[1]
if (entry && resolve(entry) === fileURLToPath(import.meta.url)) main()
