// Step 3.5 — ensure native addons exist for the host platform.
//
// Each loader (packages/{bindings,ai-gateway}/src/loader.ts) resolves its addon
// in order: an ALIEN_*_ADDON_PATH override, the per-platform prebuild package
// (optionalDependencies — only injected at publish time by the release pipeline
// (.github/workflows/release.yml); never present when packing straight from
// workspace source), then a locally-built dev `.node` found by walking up from
// the installed package to crates/<crate>. On a developer machine that walk
// finds a `.node` built earlier, so the fixture passes without help here. CI has
// neither: no prebuild and no dev `.node` (gitignored, built per-machine) — so
// build one ourselves whenever nothing else would resolve, and hand its path to
// the compile step for staging. Skipped (and logged) whenever an addon is
// already available, so local runs stay fast.

import { existsSync, readFileSync } from "node:fs"
import { createRequire } from "node:module"
import { dirname, join, relative } from "node:path"
// The napi triple mapping is owned by the bindings loader; reuse it here rather
// than keeping a second copy (compile-smoke.ts imports it the same way). Both
// addons share the same triple mapping.
import { platformTriple } from "../../bindings/src/loader.ts"
import { type CheckResult, type Ctx, lastLine, run } from "./shared.ts"

/** Resolve the napi CLI bin from the workspace install (avoids `npx` reaching the registry). */
function napiBinPath(): string {
  const napiRequire = createRequire(import.meta.url)
  const napiPkgPath = napiRequire.resolve("@napi-rs/cli/package.json")
  const napiPkg = JSON.parse(readFileSync(napiPkgPath, "utf8")) as {
    bin: string | Record<string, string>
  }
  const napiBinRel = typeof napiPkg.bin === "string" ? napiPkg.bin : napiPkg.bin.napi
  return join(dirname(napiPkgPath), napiBinRel)
}

export function ensureAddon(ctx: Ctx): CheckResult[] {
  const { scriptDir, fixtureDir, repoRoot } = ctx
  const results: CheckResult[] = []
  const triple = platformTriple()

  /**
   * Resolve (or build) one package's host dev addon. Returns the dev-addon path
   * to stage, or `undefined` when a prebuild is already installed (it resolves
   * via node_modules) or a build fails (a failure is pushed to `results`).
   */
  function resolveAddon(pkgName: string, crateName: string): string | undefined {
    const crateDir = join(repoRoot, "crates", crateName)
    const devAddonPath = join(crateDir, `${crateName}.${triple}.node`)
    const prebuildInstalledDir = join(
      fixtureDir,
      "node_modules",
      "@alienplatform",
      `${pkgName}-${triple}`,
    )

    if (existsSync(prebuildInstalledDir)) {
      console.log(
        `[addon] per-platform ${pkgName} prebuild installed for '${triple}' — no source build needed.`,
      )
      return undefined
    }
    if (existsSync(devAddonPath)) {
      console.log(
        `[addon] using existing ${crateName} dev addon at ${relative(scriptDir, devAddonPath)} (fast path, no build).`,
      )
      return devAddonPath
    }
    console.log(
      `[addon] no prebuild and no dev addon for ${crateName} '${triple}' — building one with \`napi build --platform --release\` in crates/${crateName} (CI path)...`,
    )
    const build = run(process.execPath, [napiBinPath(), "build", "--platform", "--release"], crateDir)
    if (build.status === 0 && existsSync(devAddonPath)) {
      console.log(`[addon] built ${relative(scriptDir, devAddonPath)}.`)
      return devAddonPath
    }
    console.error(
      `[addon] ${crateName} source build failed; the runtime/compile checks below will fail to load the addon.`,
    )
    results.push({
      check: "addon-build",
      package: pkgName,
      status: "fail",
      reason: `napi build --platform --release did not produce a .node for ${crateName} on this host`,
      evidence: lastLine(build.stderr) || lastLine(build.stdout) || `exit ${build.status}`,
    })
    return undefined
  }

  ctx.addonPath = resolveAddon("bindings", "alien-bindings-node")
  ctx.aiAddonPath = resolveAddon("ai-gateway", "alien-ai-gateway-node")
  ctx.addonEnv = ctx.addonPath ? { ALIEN_BINDINGS_ADDON_PATH: ctx.addonPath } : undefined

  return results
}
