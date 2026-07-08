/**
 * `bun build --compile` smoke test for the `./native` static-embed entry.
 *
 * This protects the Worker/Container build path: `alien build` will
 * bundle a user's app that imports `@alienplatform/bindings/native`, stage the
 * per-platform `.node` next to the bundled `native.js`, then run
 * `bun build --compile` to produce a single-file executable. This script
 * proves that whole path actually works, end to end, against the real addon:
 *
 *   1. Build the package (`tsdown`), producing `dist/native.js`.
 *   2. Stage the locally-built dev addon next to it as `dist/alien-bindings.node`
 *      — the exact contract documented in `src/native.ts`'s header.
 *   3. `bun build --compile` a tiny app (`compile-smoke-app.mjs`) that imports
 *      `../dist/native.js` and performs one local-kv put/get round-trip.
 *   4. Run the produced binary (from a different cwd, with the staged `.node`
 *      moved out of the way) against a real `ALIEN_SMOKE_KV_BINDING` env var,
 *      and assert its output.
 *
 * # A real bug this uncovered: `bun build --compile` needs `--format=cjs`
 *
 * `src/native.ts` imports the addon with `import addon from "./alien-bindings.node"`
 * — the literal, statically analyzable specifier Bun's own docs prescribe for
 * embedding N-API addons in a compiled executable
 * (https://bun.com/docs/bundler/executables). That part is correct and
 * unchanged. But compiling it with plain `bun build --compile` (default ESM
 * output) produces a binary that embeds the `.node` file (visible as a
 * `/$bunfs/root/...` path) yet crashes on load:
 *
 *   ReferenceError: __require is not defined
 *
 * This reproduces even with no tsdown/rolldown involved — a minimal
 * `import addon from "./x.node"` compiled with `bun build --compile` alone
 * hits it (verified against Bun 1.3.14). Passing `--format=cjs` (this script
 * does) avoids the broken codegen path entirely and the binary runs cleanly.
 * The `./native` entry itself needs no change; this is purely a required flag
 * on the `bun build --compile` invocation.
 *
 * This matters beyond this script: `packages/package-layout/run.ts`'s own
 * "compile" step (`bun build --compile src/compile-entry.ts`) passes
 * `--format=cjs` for exactly this reason. Without it, that check would hit the
 * same `__require` crash the moment it stages a real addon for the compiled
 * binary to embed.
 */

import { spawnSync } from "node:child_process"
import { copyFileSync, mkdtempSync, rmSync } from "node:fs"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"
import { findLocalAddon, platformTriple } from "../src/loader.ts"

const scriptDir = dirname(fileURLToPath(import.meta.url))
const packageDir = dirname(scriptDir)

function run(command: string, args: string[], cwd: string, env?: NodeJS.ProcessEnv) {
  console.log(`$ ${command} ${args.join(" ")}`)
  const result = spawnSync(command, args, { cwd, encoding: "utf8", env })
  if (result.stdout) process.stdout.write(result.stdout)
  if (result.stderr) process.stderr.write(result.stderr)
  return result
}

function main() {
  // 1. Build the package so dist/native.js is current.
  const build = run("pnpm", ["run", "build"], packageDir)
  if (build.status !== 0) {
    console.error("FAIL compile-smoke: `pnpm run build` failed")
    process.exit(1)
  }

  // 2. Stage the locally-built dev addon next to dist/native.js, per the
  // staging contract documented in src/native.ts.
  const triple = platformTriple()
  const localAddon = findLocalAddon(triple, packageDir)
  if (!localAddon) {
    throw new Error(
      `Could not find alien-bindings-node.${triple}.node by walking up from ${packageDir}. Build it first: \`npx napi build --platform --release\` in crates/alien-bindings-node.`,
    )
  }
  const stagedAddon = join(packageDir, "dist", "alien-bindings.node")
  copyFileSync(localAddon, stagedAddon)
  console.log(`staged ${localAddon} -> ${stagedAddon}`)

  // 3. Compile the tiny app. `--format=cjs` works around the Bun bug
  // documented above; without it the binary embeds the addon but crashes on
  // load with "ReferenceError: __require is not defined".
  const workDir = mkdtempSync(join(tmpdir(), "alien-bindings-compile-smoke-"))
  const outFile = join(workDir, "compile-smoke-bin")
  const compile = run(
    "bun",
    [
      "build",
      join("scripts", "compile-smoke-app.mjs"),
      "--compile",
      "--format=cjs",
      "--outfile",
      outFile,
    ],
    packageDir,
  )
  if (compile.status !== 0) {
    console.error("FAIL compile-smoke: `bun build --compile` failed")
    process.exit(1)
  }

  // Remove the staged .node now: if the binary didn't truly embed it, running
  // from a different cwd with the original file gone proves that.
  rmSync(stagedAddon)

  // 4. Run the compiled binary from a different cwd against a real kv round-trip.
  const kvDataDir = mkdtempSync(join(tmpdir(), "alien-bindings-compile-smoke-kv-"))
  const runResult = run(outFile, [], workDir, {
    ...process.env,
    ALIEN_DEPLOYMENT_TYPE: "local",
    ALIEN_SMOKE_KV_BINDING: JSON.stringify({ service: "local-kv", dataDir: kvDataDir }),
  })

  const ok = runResult.status === 0 && runResult.stdout.includes("OK hello-from-compiled-binary")
  console.log(ok ? "PASS compile-smoke" : "FAIL compile-smoke")
  process.exit(ok ? 0 : 1)
}

main()
