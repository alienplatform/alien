/**
 * `bun build --compile` smoke test for the **SDK / Worker** addon path.
 *
 * `packages/bindings/scripts/compile-smoke.ts` proves the `./native`
 * static-embed entry works when imported directly — the Container/Daemon case.
 * It does NOT cover the harder case a **Worker** hits: the app imports `kv` from
 * `@alienplatform/sdk` (never the bindings package), so the addon must be
 * installed through the SDK's `./native` bridge and then reach that re-exported
 * `kv` in-process. This script covers exactly that, against a real compiled
 * binary — the only oracle that can (a local `alien dev` run masks the bug via a
 * filesystem fallback).
 *
 * # Defeating the dev fallback
 *
 * The bindings loader has a dev/test fallback (`findLocalAddon`) that walks up
 * from the loader module to `crates/alien-bindings-node/*.node`. In a compiled
 * binary the loader keeps its original module path baked in, so that walk still
 * reaches the repo checkout and would mask a broken embed (a kv call "works" via
 * the on-disk dev addon even when nothing was embedded). A real container has no
 * checkout. `loadAddon()` consults, in order: the in-process `embedded` addon,
 * then `ALIEN_BINDINGS_ADDON_PATH`, then a prebuild package, then
 * `findLocalAddon`. Pointing `ALIEN_BINDINGS_ADDON_PATH` at a nonexistent file
 * short-circuits before `findLocalAddon` is ever reached, so the repo checkout
 * cannot rescue a binary that failed to embed — while a correctly embedded addon
 * still wins first.
 *
 * # What it asserts
 *
 * 1. Positive: the app built from `@alienplatform/sdk` + the `./native` bridge
 *    completes a real local-kv round-trip — the embedded addon wins over the
 *    (deliberately broken) override, proving it is truly embedded and shared
 *    with the SDK's re-exported `kv`.
 * 2. Negative (proves the test can actually fail): the same app WITHOUT the
 *    bridge embeds no addon, falls through to the broken override, and fails.
 *    Without this, a regression that stops embedding would pass silently.
 *
 * `--format=cjs` is required for the same Bun codegen reason documented in the
 * bindings compile-smoke; without it a binary that embeds a `.node` crashes on
 * load with "ReferenceError: __require is not defined".
 *
 * Run: `node --experimental-strip-types scripts/compile-smoke.ts`
 */

import { spawnSync } from "node:child_process"
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs"
import { tmpdir } from "node:os"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"
import { findLocalAddon, platformTriple } from "../../bindings/src/loader.ts"

const scriptDir = dirname(fileURLToPath(import.meta.url))
const sdkDir = dirname(scriptDir)
const packagesDir = dirname(sdkDir)
const workspaceRoot = dirname(packagesDir)

function run(command: string, args: string[], cwd: string, env?: NodeJS.ProcessEnv) {
  console.log(`$ ${command} ${args.join(" ")}`)
  const result = spawnSync(command, args, { cwd, encoding: "utf8", env })
  if (result.stdout) process.stdout.write(result.stdout)
  if (result.stderr) process.stderr.write(result.stderr)
  return result
}

/**
 * Build a Worker-like app directory: it declares only `@alienplatform/sdk` and
 * reaches the bindings package transitively through it (exactly a Worker's
 * dependency shape). The SDK is symlinked to the built workspace package so its
 * own transitive deps resolve through the real store — no fragile dep copying.
 */
function makeWorkerApp(source: string): string {
  const app = mkdtempSync(join(tmpdir(), "alien-sdk-compile-smoke-"))
  mkdirSync(join(app, "node_modules", "@alienplatform"), { recursive: true })
  symlinkSync(join(packagesDir, "sdk"), join(app, "node_modules", "@alienplatform", "sdk"))
  writeFileSync(
    join(app, "package.json"),
    JSON.stringify({
      name: "sdk-compile-smoke-app",
      type: "module",
      dependencies: { "@alienplatform/sdk": "*" },
    }),
  )
  writeFileSync(join(app, "app.ts"), source)
  return app
}

/** Compile `app/app.ts` to a standalone binary (embedding the staged addon). */
function compile(app: string): string {
  const outFile = join(app, "compile-smoke-bin")
  const result = run(
    "bun",
    ["build", "app.ts", "--compile", "--format=cjs", "--outfile", outFile],
    app,
  )
  if (result.status !== 0) {
    console.error("FAIL sdk-compile-smoke: `bun build --compile` failed")
    process.exit(1)
  }
  return outFile
}

/**
 * Run a compiled binary with the dev fallback defeated: `ALIEN_BINDINGS_ADDON_PATH`
 * points at a nonexistent file, so the loader cannot reach the repo's dev addon
 * (it is consulted before `findLocalAddon`). Only a truly embedded addon can
 * satisfy a binding call.
 */
function runBinary(binary: string, app: string) {
  const kvDataDir = mkdtempSync(join(tmpdir(), "alien-sdk-compile-smoke-kv-"))
  return run(binary, [], app, {
    ...process.env,
    ALIEN_DEPLOYMENT_TYPE: "local",
    ALIEN_SMOKE_BINDING: JSON.stringify({ service: "local-kv", dataDir: kvDataDir }),
    ALIEN_BINDINGS_ADDON_PATH: join(app, "does-not-exist.node"),
  })
}

function main() {
  // Build the SDK and its workspace deps (topological: core, bindings, sdk).
  const build = run("pnpm", ["--filter", "@alienplatform/sdk...", "run", "build"], workspaceRoot)
  if (build.status !== 0) {
    console.error("FAIL sdk-compile-smoke: building @alienplatform/sdk and its deps failed")
    process.exit(1)
  }

  const triple = platformTriple()
  const localAddon = findLocalAddon(triple, join(packagesDir, "bindings"))
  if (!localAddon) {
    throw new Error(
      `Could not find alien-bindings-node.${triple}.node by walking up from the bindings package. Build it first: \`npx napi build --platform --release\` in crates/alien-bindings-node.`,
    )
  }

  // Stage the addon at the node-resolved bindings dist — the same location a
  // Worker resolves `@alienplatform/bindings/native` to through the SDK, and the
  // one `bun build --compile` embeds from.
  const stagedAddon = join(packagesDir, "bindings", "dist", "alien-bindings.node")

  const positiveSource = readFileSync(join(scriptDir, "compile-smoke-app.ts"), "utf8")
  // Negative: identical round-trip, but nothing installs the embedded addon.
  const negativeSource = positiveSource
    .replace('import { installEmbeddedAddon } from "@alienplatform/sdk/native"\n', "")
    .replace("installEmbeddedAddon()\n", "")

  const apps: string[] = []
  try {
    copyFileSync(localAddon, stagedAddon)
    console.log(`staged ${localAddon} -> ${stagedAddon}`)

    const positiveApp = makeWorkerApp(positiveSource)
    const negativeApp = makeWorkerApp(negativeSource)
    apps.push(positiveApp, negativeApp)
    const positiveBin = compile(positiveApp)
    const negativeBin = compile(negativeApp)

    // Remove the staged addon before running: the only addon a passing run can
    // use is the one embedded into the binary at compile time.
    rmSync(stagedAddon)

    console.log("\n--- positive: WITH the SDK bridge, expect a successful kv round-trip ---")
    const positive = runBinary(positiveBin, positiveApp)
    if (positive.status !== 0 || !positive.stdout.includes("OK hello-from-sdk-compiled-binary")) {
      console.error("FAIL sdk-compile-smoke: kv round-trip did not succeed through the SDK bridge")
      process.exit(1)
    }

    console.log(
      "\n--- negative control: WITHOUT the bridge, expect this run to FAIL. " +
        "The error/stack trace below is INTENTIONAL — it is the proof. ---",
    )
    const negative = runBinary(negativeBin, negativeApp)
    if (negative.status === 0) {
      console.error(
        "FAIL sdk-compile-smoke: the no-bridge build unexpectedly succeeded — the test is not proving the embed",
      )
      process.exit(1)
    }
    console.log("negative control failed as expected — the embed is genuinely required.")
  } finally {
    rmSync(stagedAddon, { force: true })
    for (const app of apps) rmSync(app, { recursive: true, force: true })
  }

  console.log("PASS sdk-compile-smoke")
  process.exit(0)
}

main()
