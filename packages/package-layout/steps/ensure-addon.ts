// Step 3.5: ensure the host native pieces exist for the compile-smoke.
//
// Two shapes: `@alienplatform/bindings` ships a napi addon (`.node`), and
// `@alienplatform/ai-gateway` ships a standalone launcher binary. Each resolves
// in order: an override / the per-platform prebuild (optionalDependencies, only
// injected at publish time) / a locally-built artifact. CI has neither prebuild
// nor a dev artifact, so build one ourselves and hand its path to the compile
// step for staging. Skipped (and logged) whenever an artifact is already
// available, so local runs stay fast.

import { existsSync, readFileSync } from "node:fs"
import { createRequire } from "node:module"
import { dirname, join, relative } from "node:path"
// The napi triple mapping is owned by the bindings loader; reuse it here rather
// than keeping a second copy (compile-smoke.ts imports it the same way).
import { platformTriple } from "../../bindings/src/loader.ts"
import { type CheckResult, type Ctx, lastLine, run } from "./shared.ts"

const GATEWAY_BINARY = "alien-ai-gateway"
const GATEWAY_CRATE = "alien-ai-gateway"

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
   * Resolve (or build) the bindings host dev addon. Returns the dev-addon path
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
    const build = run(
      process.execPath,
      [napiBinPath(), "build", "--platform", "--release"],
      crateDir,
    )
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

  /**
   * Resolve (or cargo-build) the host `alien-ai-gateway` launcher binary that the
   * ai-gateway package spawns. Returns its path, or `undefined` when a prebuild
   * is installed (resolves via node_modules) or the build fails.
   */
  function resolveGatewayBinary(): string | undefined {
    const prebuildInstalledDir = join(
      fixtureDir,
      "node_modules",
      "@alienplatform",
      `ai-gateway-${triple}`,
    )
    if (existsSync(prebuildInstalledDir)) {
      console.log(
        `[gateway] per-platform ai-gateway binary prebuild installed for '${triple}'; no build needed.`,
      )
      return undefined
    }
    for (const profile of ["release", "debug"]) {
      const candidate = join(repoRoot, "target", profile, GATEWAY_BINARY)
      if (existsSync(candidate)) {
        console.log(
          `[gateway] using existing ${profile} binary at ${relative(scriptDir, candidate)} (fast path, no build).`,
        )
        return candidate
      }
    }
    console.log(
      `[gateway] no prebuild and no built binary; building with \`cargo build --release --bin ${GATEWAY_BINARY} -p ${GATEWAY_CRATE}\` (CI path)...`,
    )
    const build = run(
      "cargo",
      ["build", "--release", "--bin", GATEWAY_BINARY, "-p", GATEWAY_CRATE],
      repoRoot,
    )
    const built = join(repoRoot, "target", "release", GATEWAY_BINARY)
    if (build.status === 0 && existsSync(built)) {
      console.log(`[gateway] built ${relative(scriptDir, built)}.`)
      return built
    }
    console.error(
      `[gateway] cargo build of ${GATEWAY_BINARY} failed; the compile check below will fail to embed the gateway.`,
    )
    results.push({
      check: "addon-build",
      package: "ai-gateway",
      status: "fail",
      reason: `cargo build --release --bin ${GATEWAY_BINARY} -p ${GATEWAY_CRATE} did not produce a binary on this host`,
      evidence: lastLine(build.stderr) || lastLine(build.stdout) || `exit ${build.status}`,
    })
    return undefined
  }

  ctx.addonPath = resolveAddon("bindings", "alien-bindings-node")
  ctx.aiBinaryPath = resolveGatewayBinary()
  ctx.addonEnv = ctx.addonPath ? { ALIEN_BINDINGS_ADDON_PATH: ctx.addonPath } : undefined

  return results
}
