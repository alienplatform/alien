// Step 3.5 — ensure a native addon exists for the host platform.
//
// The loader (packages/bindings/src/loader.ts) resolves the addon in order:
// an ALIEN_BINDINGS_ADDON_PATH override, the per-platform prebuild package
// (optionalDependencies — only injected at publish time by `napi
// prepublish` in the release pipeline (.github/workflows/release.yml);
// never present when packing straight from workspace
// source), then a locally-built dev `.node` found by walking up from the
// installed package looking for crates/alien-bindings-node. On a developer
// machine that walk reaches this repo's real crates/alien-bindings-node and
// finds the `.node` a developer built earlier, so the fixture passes
// without any help here. CI has neither: no prebuild (04a ships those) and
// no dev `.node` (gitignored, built per-machine) — so build one ourselves
// whenever nothing else would resolve, and hand its path to every
// subprocess via the override env var. Skipped (and logged) whenever an
// addon is already available, so local runs stay fast.

import { existsSync, readFileSync } from "node:fs"
import { createRequire } from "node:module"
import { dirname, join, relative } from "node:path"
import { fileURLToPath } from "node:url"
// The napi triple mapping is owned by the bindings loader; reuse it here rather
// than keeping a second copy (compile-smoke.ts imports it the same way).
import { platformTriple } from "../../bindings/src/loader.ts"
import { type CheckResult, type Ctx, lastLine, run } from "./shared.ts"

export function ensureAddon(ctx: Ctx): CheckResult[] {
  const { scriptDir, fixtureDir, repoRoot } = ctx
  const results: CheckResult[] = []

  const bindingsNodeDir = join(repoRoot, "crates", "alien-bindings-node")
  const triple = platformTriple()
  const devAddonPath = join(bindingsNodeDir, `alien-bindings-node.${triple}.node`)
  const prebuildInstalledDir = join(
    fixtureDir,
    "node_modules",
    "@alienplatform",
    `bindings-${triple}`,
  )

  if (existsSync(prebuildInstalledDir)) {
    console.log(
      `[addon] per-platform prebuild package installed for '${triple}' — no source build needed.`,
    )
  } else if (existsSync(devAddonPath)) {
    ctx.addonPath = devAddonPath
    console.log(
      `[addon] using existing dev addon at ${relative(scriptDir, devAddonPath)} (fast path, no build).`,
    )
  } else {
    console.log(
      `[addon] no prebuild and no dev addon for '${triple}' — building one with \`napi build --platform --release\` in crates/alien-bindings-node (CI path)...`,
    )
    // crates/alien-bindings-node is not a pnpm workspace member, so it has no
    // node_modules in CI — `npx napi` there would fall through to the npm
    // registry. Resolve the napi CLI from this package's own devDependencies
    // (installed by the root frozen-lockfile `pnpm install`) and spawn its bin
    // directly with cwd set to the crate dir.
    const napiRequire = createRequire(import.meta.url)
    const napiPkgPath = napiRequire.resolve("@napi-rs/cli/package.json")
    const napiPkg = JSON.parse(readFileSync(napiPkgPath, "utf8")) as {
      bin: string | Record<string, string>
    }
    const napiBinRel = typeof napiPkg.bin === "string" ? napiPkg.bin : napiPkg.bin.napi
    const napiBinPath = join(dirname(napiPkgPath), napiBinRel)
    const build = run(
      process.execPath,
      [napiBinPath, "build", "--platform", "--release"],
      bindingsNodeDir,
    )
    if (build.status === 0 && existsSync(devAddonPath)) {
      ctx.addonPath = devAddonPath
      console.log(`[addon] built ${relative(scriptDir, devAddonPath)}.`)
    } else {
      console.error(
        "[addon] source build failed; the runtime/compile checks below will fail to load the addon.",
      )
      results.push({
        check: "addon-build",
        package: "bindings",
        status: "fail",
        reason: "napi build --platform --release did not produce a .node for this host",
        evidence: lastLine(build.stderr) || lastLine(build.stdout) || `exit ${build.status}`,
      })
    }
  }

  ctx.addonEnv = ctx.addonPath ? { ALIEN_BINDINGS_ADDON_PATH: ctx.addonPath } : undefined

  return results
}
