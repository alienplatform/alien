// Step 8 — bun build --compile of the SDK native-embed entry.
//
// The entry (fixture/src/compile-entry.ts) calls `installEmbeddedAddon()` from
// `@alienplatform/sdk/native`, which pulls in both `@alienplatform/bindings/native`
// and `@alienplatform/ai-gateway/native`. Each `./native` entry imports its addon
// through a literal specifier (`./alien-bindings.node`, `./alien-ai-gateway.node`)
// so bun's compiler stages it into the single-file binary — but only if that file
// is physically present next to the installed package's dist/native.js at build
// time (in production `alien build`'s TypeScript toolchain owns that staging, see
// PACKAGE_LAYOUT.md; here `run.ts` staged the host addons resolved above).
// `--format=cjs` is required: a plain ESM `bun build --compile` of this entry
// embeds the addons but crashes on load with `ReferenceError: __require is not
// defined` — the verified repro this compile-smoke step guards.

import { copyFileSync, existsSync, mkdirSync, rmSync } from "node:fs"
import { dirname, join } from "node:path"
import { type CheckResult, type Ctx, lastLine, run } from "./shared.ts"

export function compileNativeEmbed(ctx: Ctx): CheckResult[] {
  const { fixtureDir, bunAvailable, addonPath, aiAddonPath } = ctx
  if (!bunAvailable) return []

  const compiledDir = join(fixtureDir, ".compiled")
  mkdirSync(compiledDir, { recursive: true })
  const outFile = join(compiledDir, "compile-entry-bin")

  // Each package's `./native` entry imports its addon through a literal specifier
  // next to its own dist/native.js; stage the host addon there for both.
  const stages = [
    {
      pkg: "bindings",
      staged: join(fixtureDir, "node_modules", "@alienplatform", "bindings", "dist", "alien-bindings.node"),
      addonPath,
    },
    {
      pkg: "ai-gateway",
      staged: join(fixtureDir, "node_modules", "@alienplatform", "ai-gateway", "dist", "alien-ai-gateway.node"),
      addonPath: aiAddonPath,
    },
  ] as const

  for (const stage of stages) {
    if (!existsSync(dirname(stage.staged))) {
      return [
        {
          check: "compile",
          package: stage.pkg,
          status: "fail",
          reason: `installed ${stage.pkg} package is unavailable (see the install failure above)`,
          evidence: `expected package dist at ${dirname(stage.staged)}`,
        },
      ]
    }
    if (!stage.addonPath) {
      return [
        {
          check: "compile",
          package: stage.pkg,
          status: "fail",
          reason: `no ${stage.pkg} addon available to stage (see the addon-build failure above)`,
          evidence: `expected addon at ${stage.staged}`,
        },
      ]
    }
    try {
      copyFileSync(stage.addonPath, stage.staged)
    } catch (error) {
      return [
        {
          check: "compile",
          package: stage.pkg,
          status: "fail",
          reason: `failed to stage the host ${stage.pkg} addon for bun build --compile`,
          evidence: error instanceof Error ? error.message : String(error),
        },
      ]
    }
  }

  const built = run(
    "bun",
    ["build", "--compile", "--format=cjs", join("src", "compile-entry.ts"), "--outfile", outFile],
    fixtureDir,
  )
  if (built.status !== 0) {
    return [
      {
        check: "compile",
        package: "sdk-native",
        status: "fail",
        reason: "bun build --compile of the SDK native entry fails with the addons staged",
        evidence: lastLine(built.stderr) || lastLine(built.stdout) || `exit ${built.status}`,
      },
    ]
  }

  // Remove BOTH staged .node files: if the binary didn't truly embed them,
  // running with the source files gone proves that.
  for (const stage of stages) rmSync(stage.staged, { force: true })
  const ran = run(outFile, [], fixtureDir)
  return [
    {
      check: "compile",
      package: "sdk-native",
      status: ran.status === 0 ? "pass" : "fail",
      reason: ran.status === 0 ? "ok" : "compiled binary exited non-zero",
      evidence: ran.status === 0 ? lastLine(ran.stdout) : lastLine(ran.stderr),
    },
  ]
}
