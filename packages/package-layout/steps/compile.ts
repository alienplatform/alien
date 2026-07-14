// Step 8 — bun build --compile of the ./native embed entry.
//
// The `./native` entry (bindings/src/native.ts) imports the addon through
// the literal `./alien-bindings.node` specifier so bun's compiler can stage
// it into the single-file binary — but only if that file is physically
// present next to the installed package's dist/native.js at build time
// (in production `alien build`'s TypeScript toolchain owns that staging,
// see packages/bindings/PACKAGE_LAYOUT.md; here we stage the
// addon `run.ts` itself resolved above). `--format=cjs` is required: a
// plain ESM `bun build --compile` of this entry embeds the addon but
// crashes on load with `ReferenceError: __require is not defined` — see
// this compile-smoke step for the verified repro.

import { copyFileSync, existsSync, mkdirSync, rmSync } from "node:fs"
import { dirname, join } from "node:path"
import { type CheckResult, type Ctx, lastLine, run } from "./shared.ts"

export function compileNativeEmbed(ctx: Ctx): CheckResult[] {
  const { fixtureDir, bunAvailable, addonPath } = ctx
  if (!bunAvailable) return []

  const compiledDir = join(fixtureDir, ".compiled")
  mkdirSync(compiledDir, { recursive: true })
  const outFile = join(compiledDir, "compile-entry-bin")
  const stagedAddonPath = join(
    fixtureDir,
    "node_modules",
    "@alienplatform",
    "bindings",
    "dist",
    "alien-bindings.node",
  )

  if (!existsSync(dirname(stagedAddonPath))) {
    return [
      {
        check: "compile",
        package: "bindings",
        status: "fail",
        reason: "installed bindings package is unavailable (see the install failure above)",
        evidence: `expected package dist at ${dirname(stagedAddonPath)}`,
      },
    ]
  }

  if (addonPath) {
    try {
      copyFileSync(addonPath, stagedAddonPath)
    } catch (error) {
      return [
        {
          check: "compile",
          package: "bindings",
          status: "fail",
          reason: "failed to stage the host addon for bun build --compile",
          evidence: error instanceof Error ? error.message : String(error),
        },
      ]
    }
  }

  const built = addonPath
    ? run(
        "bun",
        [
          "build",
          "--compile",
          "--format=cjs",
          join("src", "compile-entry.ts"),
          "--outfile",
          outFile,
        ],
        fixtureDir,
      )
    : undefined

  if (!built) {
    return [
      {
        check: "compile",
        package: "bindings",
        status: "fail",
        reason: "no addon available to stage (see the addon-build failure above)",
        evidence: `expected addon at ${stagedAddonPath}`,
      },
    ]
  }
  if (built.status !== 0) {
    return [
      {
        check: "compile",
        package: "bindings",
        status: "fail",
        reason: "bun build --compile of ./native entry fails with the addon staged",
        evidence: lastLine(built.stderr) || lastLine(built.stdout) || `exit ${built.status}`,
      },
    ]
  }

  // Remove the staged .node now: if the binary didn't truly embed it,
  // running with the source file gone proves that (mirrors
  // this compile-smoke step).
  rmSync(stagedAddonPath, { force: true })
  const ran = run(outFile, [], fixtureDir)
  return [
    {
      check: "compile",
      package: "bindings",
      status: ran.status === 0 ? "pass" : "fail",
      reason: ran.status === 0 ? "ok" : "compiled binary exited non-zero",
      evidence: ran.status === 0 ? lastLine(ran.stdout) : lastLine(ran.stderr),
    },
  ]
}
